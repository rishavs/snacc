//! Runtime import declarations (Specification 029 section 7.1).
//!
//! One generic path declares every runtime import from its [`ImportRow`]:
//! the row's symbol (patterned or literal) names the declaration, and its
//! parameter and result shapes build the signature. The declarations produced
//! are byte-identical to the hand-written methods this replaced.

use super::*;

fn is_subword_int<'ctx>(ty: impl TryInto<IntType<'ctx>>) -> bool {
    ty.try_into()
        .is_ok_and(|int: IntType<'ctx>| int.get_bit_width() < 32)
}

/// Rust's `extern "C"` functions carry `zeroext` on sub-word integer parameters
/// and results on every target Snacc emits for (confirmed against rustc's own
/// IR). Specification 009 section 5.2 requires the backend to match that rather
/// than assume an LLVM width alone defines the call ABI, so declarations and
/// their call sites both get the attribute -- this covers `Byte`, `UInt16`,
/// and the pre-existing `Bool`/`Nil` `u8` mapping alike.
pub(crate) fn zero_extend_subwords<'ctx>(
    context: &'ctx Context,
    signature: FunctionType<'ctx>,
    mut add: impl FnMut(AttributeLoc, Attribute),
) {
    let zeroext = context.create_enum_attribute(Attribute::get_named_enum_kind_id("zeroext"), 0);
    for (index, param) in signature.get_param_types().into_iter().enumerate() {
        if is_subword_int(param) {
            add(AttributeLoc::Param(index as u32), zeroext);
        }
    }
    if signature.get_return_type().is_some_and(is_subword_int) {
        add(AttributeLoc::Return, zeroext);
    }
}

pub(crate) fn declare<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    symbol: &str,
    signature: FunctionType<'ctx>,
    linkage: Option<Linkage>,
) -> FunctionValue<'ctx> {
    let function = module.add_function(symbol, signature, linkage);
    zero_extend_subwords(context, signature, |location, attribute| {
        function.add_attribute(location, attribute)
    });
    function
}

impl<'ctx> Codegen<'ctx, '_> {
    fn resolve_abi(&self, abi: AbiTy, primary: Ty, assoc: Ty) -> BasicMetadataTypeEnum<'ctx> {
        match abi {
            AbiTy::Ptr => self.context.ptr_type(AddressSpace::default()).into(),
            AbiTy::Usize => self.usize_ty().into(),
            AbiTy::I64 => self.context.i64_type().into(),
            AbiTy::Key => self.ty(primary).into(),
            AbiTy::Assoc => self.ty(assoc).into(),
            AbiTy::Scalar => scalar_ty(self.context, primary).into(),
            AbiTy::View => self.view_type().into(),
        }
    }

    /// Declares the runtime import for one table row on first use: the row's
    /// symbol names the declaration and its shapes build the signature.
    /// `primary` is the key/element/printed type, `assoc` the map value type;
    /// either is `Ty::Nil` when its row does not use it.
    pub(crate) fn runtime_import(
        &self,
        family: Family,
        op: &str,
        primary: Ty,
        assoc: Ty,
    ) -> Result<FunctionValue<'ctx>, String> {
        let row = import_row(family, op, primary)?;
        let (lookup, declared) = match row.symbol {
            SymbolSpec::Pattern {
                prefix,
                middle,
                suffix_first,
            } => {
                let suffix = name_suffix(primary);
                let name = if suffix_first {
                    if middle.is_empty() {
                        format!("snacc_{prefix}_{suffix}_{op}")
                    } else {
                        format!("snacc_{prefix}_{suffix}_{middle}_{op}")
                    }
                } else if middle.is_empty() {
                    format!("snacc_{prefix}_{op}_{suffix}")
                } else {
                    format!("snacc_{prefix}_{op}_{middle}_{suffix}")
                };
                (name.clone(), name)
            }
            SymbolSpec::Literal(symbol) => (symbol.to_string(), symbol.to_string()),
            SymbolSpec::Split { lookup, declare } => (lookup.to_string(), declare.to_string()),
        };
        let params: Vec<BasicMetadataTypeEnum<'ctx>> = row
            .params
            .iter()
            .map(|abi| self.resolve_abi(*abi, primary, assoc))
            .collect();
        let signature = match row.result {
            AbiResult::Void => self.context.void_type().fn_type(&params, false),
            AbiResult::I8 => self.context.i8_type().fn_type(&params, false),
            AbiResult::I64 => self.context.i64_type().fn_type(&params, false),
            AbiResult::Key => self.ty(primary).fn_type(&params, false),
            AbiResult::Assoc => self.ty(assoc).fn_type(&params, false),
        };
        Ok(self
            .module
            .get_function(&lookup)
            .unwrap_or_else(|| declare(self.context, self.module, &declared, signature, None)))
    }

    pub(crate) fn map_uses_raw_value(&self, value_ty: Ty) -> bool {
        value_ty != Ty::Int64
    }
}

const MAP_OPS: &[&str] = &[
    "contains", "insert", "delete", "index", "take", "key_at", "value_at", "clear", "reserve",
    "drop",
];

const SET_OPS: &[&str] = &[
    "contains", "insert", "delete", "at", "clear", "reserve", "drop",
];
const LIST_OPS: &[&str] = &["push", "pop", "insert", "remove"];

fn class_of(ty: Ty) -> KeyClass {
    match ty {
        Ty::String => KeyClass::String,
        Ty::ViewByte => KeyClass::View,
        _ => KeyClass::Scalar,
    }
}

fn is_map_key(ty: Ty) -> bool {
    matches!(
        ty,
        Ty::String
            | Ty::ViewByte
            | Ty::Byte
            | Ty::UInt16
            | Ty::UInt32
            | Ty::UInt64
            | Ty::Int64
            | Ty::Bool
            | Ty::Unicode
    )
}

fn is_set_elem(ty: Ty) -> bool {
    is_map_key(ty)
}

fn is_scalar_list_elem(ty: Ty) -> bool {
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

/// Validates `(family, op, primary)` with the diagnostics the hand-written
/// lookups produced, then returns the table row. A miss after validation is a
/// missing table entry, not a program error.
pub(crate) fn import_row(
    family: Family,
    op: &str,
    primary: Ty,
) -> Result<&'static ImportRow, String> {
    match family {
        Family::Map => {
            if !MAP_OPS.contains(&op) {
                return Err(internal("unsupported map operation reached lowering"));
            }
            if !is_map_key(primary) {
                return Err(internal("unsupported map key type reached lowering"));
            }
        }
        Family::MapRaw => {
            if !MAP_OPS.contains(&op) {
                return Err(internal("unsupported raw map operation reached lowering"));
            }
            if !is_map_key(primary) {
                return Err(internal("unsupported raw map key type reached lowering"));
            }
        }
        Family::Set => {
            if !SET_OPS.contains(&op) {
                return Err(internal("unsupported set operation reached lowering"));
            }
            if !is_set_elem(primary) {
                return Err(internal("unsupported set element type reached lowering"));
            }
        }
        Family::List => {
            if !LIST_OPS.contains(&op) {
                return Err(internal("unsupported scalar list operation"));
            }
            if !is_scalar_list_elem(primary) {
                if op == "push" {
                    return Err(internal("unsupported List.push element type"));
                }
                return Err(internal("unsupported scalar list operation"));
            }
        }
        Family::ListRaw => {
            if !matches!(op, "push" | "pop" | "insert" | "remove" | "clear") {
                unreachable!("unsupported raw list operation")
            }
        }
        Family::ListFixed | Family::String | Family::View | Family::Print | Family::Fail => {}
    }
    let class = match family {
        Family::Map | Family::MapRaw | Family::Set | Family::List => class_of(primary),
        Family::ListRaw
        | Family::ListFixed
        | Family::String
        | Family::View
        | Family::Print
        | Family::Fail => KeyClass::Any,
    };
    IMPORTS
        .iter()
        .find(|row| row.family == family && row.op == op && row.class == class)
        .ok_or_else(|| internal("the runtime import table is missing an entry"))
}
