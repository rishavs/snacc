//! LLVM type mapping and layout (Specification 010 section 15.2;
//! Specification 018 section 8).

use super::*;

/// Specification 009 section 5.2: the value/storage type for each scalar.
pub(crate) fn scalar_ty(context: &Context, ty: Ty) -> BasicTypeEnum<'_> {
    match ty {
        Ty::Float64 => context.f64_type().into(),
        Ty::Float32 => context.f32_type().into(),
        Ty::Int64 | Ty::UInt64 => context.i64_type().into(),
        Ty::UInt32 => context.i32_type().into(),
        Ty::UInt16 => context.i16_type().into(),
        Ty::Bool | Ty::Nil | Ty::Byte => context.i8_type().into(),
        Ty::Unicode => context.i32_type().into(),
        Ty::String => {
            let ptr = context.ptr_type(AddressSpace::default());
            context
                .struct_type(
                    &[
                        ptr.into(),
                        context.i64_type().into(),
                        context.i64_type().into(),
                    ],
                    false,
                )
                .into()
        }
        Ty::ViewByte | Ty::ViewUnicode => {
            let ptr = context.ptr_type(AddressSpace::default());
            context
                .struct_type(&[ptr.into(), context.i64_type().into()], false)
                .into()
        }
        Ty::Array(_) | Ty::List(_) | Ty::Map(_) | Ty::Set(_) | Ty::View(_) => {
            let ptr = context.ptr_type(AddressSpace::default());
            context
                .struct_type(
                    &[
                        ptr.into(),
                        context.i64_type().into(),
                        context.i64_type().into(),
                    ],
                    false,
                )
                .into()
        }
        // Every caller routes `Ty::User` and `Ty::Sum` through their layout
        // tables first (see `llvm_ty`).
        Ty::User(_) => unreachable!("a user-defined type resolves through the layout table"),
        Ty::Sum(_) => unreachable!("an inline sum resolves through the sum layout table"),
        // Specification 016 section 4.1/10: `Box<T>` lowers to a non-null
        // target pointer in the private Snacc ABI, exactly like `Ref<T>`'s
        // own pointer representation (`function_type`'s `ParamMode::
        // Reference` arm) -- pointers are opaque in this LLVM version, so
        // the pointee's own type never appears here regardless of `T`.
        Ty::Box(_) => context.ptr_type(AddressSpace::default()).into(),
    }
}

/// The value/storage type for any checked type. A user-defined type is looked
/// up in the layout table built by [`build_layout`]; an inline sum is looked
/// up in that same call's sum layout table, indexed by `SumId` instead of
/// `TypeId` (Specification 018 section 8 reuses named-union lowering, but a
/// sum has no `TypeId` of its own to share that table with).
pub(crate) fn llvm_ty<'ctx>(
    context: &'ctx Context,
    layout: &[BasicTypeEnum<'ctx>],
    sums: &[BasicTypeEnum<'ctx>],
    ty: Ty,
) -> BasicTypeEnum<'ctx> {
    match ty {
        Ty::User(id) => layout[id.index()],
        Ty::Sum(id) => sums[id.index()],
        scalar => scalar_ty(context, scalar),
    }
}

/// Specification 010 section 15.2. A represented type lowers to its immediate
/// representation's LLVM type and gets no named type of its own; a struct and a
/// union member lower to a named LLVM struct in field order; a union lowers to
/// `{i32 tag, member_0, ..., member_n}`, one storage field per member.
/// Specification 018 section 8 reuses that same tag-plus-fields shape for an
/// inline sum, keyed by `SumId` instead of `TypeId` since a sum has no
/// declared name of its own.
///
/// Every named type is predeclared opaque first, then bodies are set through a
/// depth-first walk, so a type is always laid out after everything it contains
/// by value. A struct field or sum member may go the other way too (a sum
/// member may be a user-defined type, and a struct field may be an inline
/// sum), so both walks share one `Layout` and resolve into each other on
/// demand.
pub(crate) fn build_layout<'ctx>(
    context: &'ctx Context,
    defs: &[TypeDef],
    sums: &[Vec<Ty>],
) -> Result<(Vec<BasicTypeEnum<'ctx>>, Vec<BasicTypeEnum<'ctx>>), String> {
    let named: Vec<Option<StructType<'ctx>>> = defs
        .iter()
        .map(|def| match def {
            TypeDef::Represented { .. } => None,
            _ => Some(context.opaque_struct_type(def.name())),
        })
        .collect();
    // A sum has no source name; `sum.<id>` is a debug-only LLVM identifier,
    // never observable from Snacc source (Specification 018 section 8).
    let sum_named: Vec<StructType<'ctx>> = (0..sums.len())
        .map(|index| context.opaque_struct_type(&format!("sum.{index}")))
        .collect();
    let mut state = Layout {
        defs,
        sums,
        named,
        sum_named,
        resolved: vec![None; defs.len()],
        visiting: vec![false; defs.len()],
        sum_resolved: vec![None; sums.len()],
        sum_visiting: vec![false; sums.len()],
    };
    for index in 0..defs.len() {
        state.resolve(context, TypeId(index as u32))?;
    }
    for index in 0..sums.len() {
        state.resolve_sum(context, SumId(index as u32))?;
    }
    let types = state
        .resolved
        .into_iter()
        .map(|ty| ty.expect("every declared type resolves to an LLVM type"))
        .collect();
    let sums = state
        .sum_resolved
        .into_iter()
        .map(|ty| ty.expect("every interned sum resolves to an LLVM type"))
        .collect();
    Ok((types, sums))
}

struct Layout<'ctx, 'a> {
    defs: &'a [TypeDef],
    sums: &'a [Vec<Ty>],
    named: Vec<Option<StructType<'ctx>>>,
    sum_named: Vec<StructType<'ctx>>,
    resolved: Vec<Option<BasicTypeEnum<'ctx>>>,
    visiting: Vec<bool>,
    sum_resolved: Vec<Option<BasicTypeEnum<'ctx>>>,
    sum_visiting: Vec<bool>,
}

impl<'ctx> Layout<'ctx, '_> {
    fn resolve(
        &mut self,
        context: &'ctx Context,
        id: TypeId,
    ) -> Result<BasicTypeEnum<'ctx>, String> {
        if let Some(ty) = self.resolved[id.index()] {
            return Ok(ty);
        }
        if std::mem::replace(&mut self.visiting[id.index()], true) {
            // The checker proves every value layout finite before returning a
            // program, so arriving here means that proof was wrong.
            return Err(internal(format!(
                "'{}' contains itself by value",
                self.defs[id.index()].name()
            )));
        }
        let ty: BasicTypeEnum<'ctx> = match &self.defs[id.index()] {
            TypeDef::Represented { target, .. } => self.resolve_ty(context, *target)?,
            TypeDef::Struct { fields, .. } | TypeDef::UnionMember { fields, .. } => {
                let types = self.resolve_all(context, fields.iter().map(|(_, ty)| *ty))?;
                self.named_ty(id)?.set_body(&types, false);
                self.named_ty(id)?.into()
            }
            TypeDef::Union { members, .. } => {
                let members: Vec<Ty> = members.iter().map(|id| Ty::User(*id)).collect();
                let mut types: Vec<BasicTypeEnum<'ctx>> = vec![context.i32_type().into()];
                types.extend(self.resolve_all(context, members)?);
                self.named_ty(id)?.set_body(&types, false);
                self.named_ty(id)?.into()
            }
        };
        self.visiting[id.index()] = false;
        self.resolved[id.index()] = Some(ty);
        Ok(ty)
    }

    /// An inline sum's `{i32 tag, member_0, ..., member_n}` layout, in the
    /// sum's canonical (sorted) member order -- the same order lowering later
    /// reads a member's position from to assign its deterministic tag
    /// (Specification 018 Phase 4 item 1). A `Nil` member is a scalar here
    /// (`Ty::Nil` lowers through `scalar_ty` like `Bool`), not a zero-field
    /// struct the way a named union's `Nil` member is: unlike a union member,
    /// an inline sum member is never itself a `TypeId`.
    fn resolve_sum(
        &mut self,
        context: &'ctx Context,
        id: SumId,
    ) -> Result<BasicTypeEnum<'ctx>, String> {
        if let Some(ty) = self.sum_resolved[id.index()] {
            return Ok(ty);
        }
        if std::mem::replace(&mut self.sum_visiting[id.index()], true) {
            // The checker's layout-cycle check walks through a sum's direct
            // members into any user-defined type among them, so this can only
            // mean that proof was wrong.
            return Err(internal("an inline sum contains itself by value"));
        }
        let members = self.sums[id.index()].clone();
        let mut types: Vec<BasicTypeEnum<'ctx>> = vec![context.i32_type().into()];
        types.extend(self.resolve_all(context, members)?);
        let named = self.sum_named[id.index()];
        named.set_body(&types, false);
        self.sum_visiting[id.index()] = false;
        self.sum_resolved[id.index()] = Some(named.into());
        Ok(named.into())
    }

    fn resolve_all(
        &mut self,
        context: &'ctx Context,
        types: impl IntoIterator<Item = Ty>,
    ) -> Result<Vec<BasicTypeEnum<'ctx>>, String> {
        types
            .into_iter()
            .map(|ty| self.resolve_ty(context, ty))
            .collect()
    }

    fn resolve_ty(
        &mut self,
        context: &'ctx Context,
        ty: Ty,
    ) -> Result<BasicTypeEnum<'ctx>, String> {
        match ty {
            Ty::User(id) => self.resolve(context, id),
            Ty::Sum(id) => self.resolve_sum(context, id),
            scalar => Ok(scalar_ty(context, scalar)),
        }
    }

    fn named_ty(&self, id: TypeId) -> Result<StructType<'ctx>, String> {
        self.named[id.index()]
            .ok_or_else(|| internal("a represented type was given a named LLVM struct"))
    }
}

/// Whether a type is lowered as a floating-point value rather than an integer.
pub(crate) fn is_float(ty: Ty) -> bool {
    match ty {
        Ty::Float64 | Ty::Float32 => true,
        Ty::Int64
        | Ty::Byte
        | Ty::UInt16
        | Ty::UInt32
        | Ty::UInt64
        | Ty::Bool
        | Ty::Nil
        | Ty::User(_)
        | Ty::Sum(_)
        | Ty::Box(_) => false,
        Ty::String
        | Ty::Unicode
        | Ty::ViewByte
        | Ty::ViewUnicode
        | Ty::Array(_)
        | Ty::List(_)
        | Ty::View(_)
        | Ty::Map(_)
        | Ty::Set(_) => false,
    }
}

/// Whether an integer type's division and ordering are unsigned. Signedness is
/// read from the checked type, never inferred from an LLVM bit width.
pub(crate) fn is_unsigned(ty: Ty) -> bool {
    match ty {
        Ty::Byte | Ty::UInt16 | Ty::UInt32 | Ty::UInt64 => true,
        Ty::Int64
        | Ty::Float64
        | Ty::Float32
        | Ty::Bool
        | Ty::Nil
        | Ty::User(_)
        | Ty::Sum(_)
        | Ty::Box(_) => false,
        Ty::String | Ty::ViewByte | Ty::ViewUnicode => false,
        Ty::Unicode => true,
        Ty::Array(_) | Ty::List(_) | Ty::View(_) | Ty::Map(_) | Ty::Set(_) => false,
    }
}

pub(crate) fn is_scalar_collection_element(ty: Ty) -> bool {
    matches!(
        ty,
        Ty::Int64
            | Ty::Byte
            | Ty::UInt16
            | Ty::UInt32
            | Ty::UInt64
            | Ty::Float32
            | Ty::Float64
            | Ty::Bool
            | Ty::Unicode
    )
}
