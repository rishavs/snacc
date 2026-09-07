//! Function and module lowering: the `Codegen` struct, call lowering, and
//! module construction (Specification 029 Phase 3).

use super::*;

pub(crate) mod cleanup;
pub(crate) mod collections;
pub(crate) mod equality;
pub(crate) mod expr;
pub(crate) mod stmt;

const SELF: &str = "self";

/// How a local's current value is reached. Immutable roots stay as SSA values;
/// a `let mut` root and a method's `self` need addressable storage.
#[derive(Clone, Copy)]
pub(crate) enum Slot<'ctx> {
    Value(BasicValueEnum<'ctx>),
    Mutable(PointerValue<'ctx>),
}

pub(crate) type Env<'ctx> = Vec<(String, Slot<'ctx>)>;

/// How one incoming parameter binds. A `Ref<T>` parameter is a mutable root of
/// its referent (Specification 011 section 7), and its incoming LLVM value is
/// already the address of the caller's storage -- so it binds as the very slot
/// shape a `let mut` local uses, with no `alloca` of its own.
fn param_slot<'ctx>(param: &TParam, value: BasicValueEnum<'ctx>) -> Slot<'ctx> {
    match param.mode {
        ParamMode::Value => Slot::Value(value),
        ParamMode::Reference => Slot::Mutable(value.into_pointer_value()),
    }
}

/// A declaration without a result lowers to an LLVM `void` function; no value
/// type stands in for its absent result. `leading` supplies the hidden receiver
/// pointer of a method (Specification 010 section 15.3) and is empty otherwise.
///
/// Specification 011 section 11: a `Ref<T>` parameter lowers to a pointer to
/// the caller's storage. Pointers are opaque in this LLVM version, so the
/// referent's own type never appears in the signature.
fn function_type<'ctx>(
    context: &'ctx Context,
    target_data: &TargetData,
    layout: &[BasicTypeEnum<'ctx>],
    sums: &[BasicTypeEnum<'ctx>],
    leading: &[BasicMetadataTypeEnum<'ctx>],
    params: &[TParam],
    result: Option<Ty>,
    bridge: bool,
) -> FunctionType<'ctx> {
    let mut llvm_params = leading.to_vec();
    for param in params {
        match param.mode {
            ParamMode::Value if bridge && is_bridge_view_ty(param.ty) => {
                let pointer = context.ptr_type(AddressSpace::default());
                llvm_params.push(pointer.into());
                llvm_params.push(context.ptr_sized_int_type(target_data, None).into());
            }
            ParamMode::Value => llvm_params.push(llvm_ty(context, layout, sums, param.ty).into()),
            ParamMode::Reference => {
                llvm_params.push(context.ptr_type(AddressSpace::default()).into())
            }
        }
    }
    match result {
        Some(ty) => llvm_ty(context, layout, sums, ty).fn_type(&llvm_params, false),
        None => context.void_type().fn_type(&llvm_params, false),
    }
}

fn is_bridge_view_ty(ty: Ty) -> bool {
    matches!(ty, Ty::ViewByte | Ty::ViewUnicode | Ty::View(_))
}

pub(crate) struct Codegen<'ctx, 'a> {
    pub(crate) context: &'ctx Context,
    pub(crate) builder: &'a Builder<'ctx>,
    pub(crate) module: &'a Module<'ctx>,
    pub(crate) functions: &'a HashMap<String, FunctionValue<'ctx>>,
    /// Lowered methods, indexed by `MethodId`.
    pub(crate) methods: &'a [FunctionValue<'ctx>],
    /// The checked program, read for type definitions and the receiver-write
    /// effect, both indexed by their resolved ID.
    pub(crate) program: &'a Program,
    /// LLVM types for those definitions, indexed by `TypeId`.
    pub(crate) layout: &'a [BasicTypeEnum<'ctx>],
    /// LLVM types for every interned inline sum, indexed by `SumId`
    /// (Specification 018 section 8).
    pub(crate) sums: &'a [BasicTypeEnum<'ctx>],
    /// The target's real size and alignment facts (Specification 016 section
    /// 8.2): `box(expression)` and every drop of a boxed value need a
    /// pointee's actual byte size and alignment to call the runtime
    /// allocator/deallocator correctly, which an LLVM type alone does not
    /// carry without consulting the target.
    pub(crate) target_data: &'a TargetData,
}

/// Basic blocks a `break` may branch to, innermost last.
pub(crate) type Loops<'ctx> = Vec<BasicBlock<'ctx>>;

impl<'ctx> Codegen<'ctx, '_> {
    pub(crate) fn ty(&self, ty: Ty) -> BasicTypeEnum<'ctx> {
        llvm_ty(self.context, self.layout, self.sums, ty)
    }

    /// The named LLVM struct behind a type that has fields, or a union.
    pub(crate) fn struct_ty(&self, ty: Ty) -> Result<StructType<'ctx>, String> {
        match self.ty(ty) {
            BasicTypeEnum::StructType(structure) => Ok(structure),
            _ => Err(internal("a field path reached a type without fields")),
        }
    }

    pub(crate) fn def(&self, id: TypeId) -> &'_ TypeDef {
        &self.program.types[id.index()]
    }

    /// The declared type of one field of a struct or union-member type.
    pub(crate) fn field_ty(&self, ty: Ty, index: usize) -> Result<Ty, String> {
        let Ty::User(id) = ty else {
            return Err(internal("a field path reached a built-in type"));
        };
        self.def(id)
            .fields()
            .and_then(|fields| fields.get(index))
            .map(|(_, ty)| *ty)
            .ok_or_else(|| internal("a field path selected a field that does not exist"))
    }

    /// A union member's deterministic source-order tag.
    pub(crate) fn member_tag(&self, member: TypeId) -> Result<u32, String> {
        match self.def(member) {
            TypeDef::UnionMember { tag, .. } => Ok(*tag),
            _ => Err(internal(
                "injection named a type that is not a union member",
            )),
        }
    }

    /// An inline sum member's deterministic tag (Specification 018 Phase 4
    /// item 1): its position in the sum's canonical (sorted) member list, the
    /// same list `SumTable::intern` built and the checker's `InjectSum` and
    /// `SumTest` nodes both refer back to by `SumId` alone -- neither records
    /// a tag itself, since assigning one is lowering's job.
    pub(crate) fn sum_member_tag(&self, sum: SumId, member: Ty) -> Result<u32, String> {
        self.program.sums[sum.index()]
            .iter()
            .position(|candidate| *candidate == member)
            .map(|index| index as u32)
            .ok_or_else(|| {
                internal(
                    "a sum type test or injection named a type that is not a member of the sum",
                )
            })
    }

    /// A box's pointee type (Specification 016 section 4.1), mirroring
    /// `Types::box_pointee` against the lowering-only snapshot `Program`
    /// carries instead.
    pub(crate) fn box_pointee(&self, id: BoxId) -> Ty {
        self.program.boxes[id.index()]
    }

    /// The target's `usize`-equivalent integer type (Specification 016
    /// section 8.2): the runtime allocator's `size`/`align` parameters are
    /// Rust `usize`, whose width is always the target's pointer width.
    pub(crate) fn usize_ty(&self) -> IntType<'ctx> {
        self.context.ptr_sized_int_type(self.target_data, None)
    }

    /// A type's real target size and ABI alignment (Specification 016
    /// section 8.2), for a runtime allocate/deallocate call's `size`/`align`
    /// arguments.
    pub(crate) fn size_align(&self, ty: Ty) -> (u64, u64) {
        let llvm_ty = self.ty(ty);
        (
            self.target_data.get_abi_size(&llvm_ty),
            u64::from(self.target_data.get_abi_alignment(&llvm_ty)),
        )
    }

    /// Declares the runtime allocator import on first use, mirroring the table-driven declaration path.
    pub(crate) fn alloc_import(&self) -> FunctionValue<'ctx> {
        let symbol = "snacc_alloc";
        self.module.get_function(symbol).unwrap_or_else(|| {
            let usize_ty = self.usize_ty();
            let ptr_ty = self.context.ptr_type(AddressSpace::default());
            declare(
                self.context,
                self.module,
                symbol,
                ptr_ty.fn_type(&[usize_ty.into(), usize_ty.into()], false),
                None,
            )
        })
    }

    /// Declares the runtime deallocator import on first use, mirroring the table-driven declaration path.
    pub(crate) fn dealloc_import(&self) -> FunctionValue<'ctx> {
        let symbol = "snacc_dealloc";
        self.module.get_function(symbol).unwrap_or_else(|| {
            let usize_ty = self.usize_ty();
            let ptr_ty = self.context.ptr_type(AddressSpace::default());
            declare(
                self.context,
                self.module,
                symbol,
                self.context
                    .void_type()
                    .fn_type(&[ptr_ty.into(), usize_ty.into(), usize_ty.into()], false),
                None,
            )
        })
    }

    /// Declares the single fatal runtime entry used for every invalid
    /// floating-point result. Keeping this as one import gives the NaN
    /// invariant a stable ABI surface instead of one symbol per operation.
    pub(crate) fn invalid_floating_operation_import(&self) -> FunctionValue<'ctx> {
        let symbol = "snacc_invalid_floating_operation";
        self.module.get_function(symbol).unwrap_or_else(|| {
            declare(
                self.context,
                self.module,
                symbol,
                self.context.void_type().fn_type(&[], false),
                None,
            )
        })
    }

    /// Branches around one unordered floating-point check. LLVM's UNO
    /// predicate is true exactly when either operand is NaN; comparing a
    /// value with itself therefore detects NaN without changing the value.
    pub(crate) fn validate_float(
        &self,
        value: FloatValue<'ctx>,
        label: &str,
    ) -> Result<(), String> {
        let is_nan = self
            .builder
            .build_float_compare(FloatPredicate::UNO, value, value, label)
            .map_err(|error| error.to_string())?;
        let function = self.current_function();
        let invalid = self.context.append_basic_block(function, "invalid_float");
        let valid = self.context.append_basic_block(function, "valid_float");
        self.builder
            .build_conditional_branch(is_nan, invalid, valid)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(invalid);
        self.invoke(self.invalid_floating_operation_import(), &[])?;
        self.builder
            .build_unreachable()
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(valid);
        Ok(())
    }

    /// Calls the runtime deallocator for `ptr`, sized and aligned for
    /// `pointee` (Specification 016 section 8.1: a box releases its
    /// allocation on destruction).
    pub(crate) fn call_dealloc(&self, ptr: PointerValue<'ctx>, pointee: Ty) -> Result<(), String> {
        let (size, align) = self.size_align(pointee);
        self.call_raw_dealloc(
            ptr,
            self.usize_ty().const_int(size, false),
            self.usize_ty().const_int(align, false),
        )
    }

    pub(crate) fn call_raw_dealloc(
        &self,
        ptr: PointerValue<'ctx>,
        size: IntValue<'ctx>,
        align: IntValue<'ctx>,
    ) -> Result<(), String> {
        let dealloc_fn = self.dealloc_import();
        self.invoke(dealloc_fn, &[ptr.into(), size.into(), align.into()])?;
        Ok(())
    }

    /// Follows zero or more `Box<T>` layers from an address `ptr` whose
    /// current content is a value of `ty`, loading each box's stored pointer
    /// value in turn, until `ty` is no longer `Ty::Box` (Specification 016
    /// section 4.3). Mirrors `deref_box` in `checker.rs` at the value level:
    /// that function decides *how many* layers automatic access crosses; this
    /// one performs each crossing as a genuine load, since a box field or
    /// local's own storage holds a pointer that must be read to reach its
    /// pointee's real (heap) address.
    pub(crate) fn deref_box_ptr(
        &self,
        mut ptr: PointerValue<'ctx>,
        mut ty: Ty,
    ) -> Result<(PointerValue<'ctx>, Ty), String> {
        while let Ty::Box(id) = ty {
            ptr = self
                .builder
                .build_load(self.context.ptr_type(AddressSpace::default()), ptr, "deref")
                .map_err(|error| error.to_string())?
                .into_pointer_value();
            ty = self.box_pointee(id);
        }
        Ok((ptr, ty))
    }

    pub(crate) fn current_function(&self) -> FunctionValue<'ctx> {
        self.builder
            .get_insert_block()
            .and_then(|block| block.get_parent())
            .expect("lowering always occurs inside a function")
    }

    pub(crate) fn call(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        name: &str,
        args: &[TArg],
    ) -> Result<inkwell::values::CallSiteValue<'ctx>, String> {
        let mut llvm_args = Vec::new();
        let extern_decl = self.program.externs.get(name);
        for (index, arg) in args.iter().enumerate() {
            let bridge_view = extern_decl
                .and_then(|declaration| declaration.params.get(index))
                .is_some_and(|param| param.mode == ParamMode::Value && is_bridge_view_ty(param.ty));
            if bridge_view {
                let TArg::Value(value) = arg else {
                    return Err(internal(
                        "a view bridge parameter reached lowering by reference",
                    ));
                };
                let descriptor = self.expr(env, loops, value)?.into_struct_value();
                llvm_args.push(
                    self.builder
                        .build_extract_value(descriptor, 0, "bridge_view_ptr")
                        .map_err(|error| error.to_string())?
                        .into(),
                );
                llvm_args.push(
                    self.builder
                        .build_extract_value(descriptor, 1, "bridge_view_len")
                        .map_err(|error| error.to_string())?
                        .into(),
                );
            } else {
                llvm_args.push(self.argument(env, loops, arg)?);
            }
        }
        let call = self.invoke(self.functions[name], &llvm_args)?;
        if let Some(extern_decl) = extern_decl {
            for (param, arg) in extern_decl.params.iter().zip(args) {
                if matches!(param.ty, Ty::Float32 | Ty::Float64)
                    && matches!(param.mode, ParamMode::Reference)
                {
                    let TArg::Reference(place) = arg else {
                        return Err(internal(
                            "a reference bridge parameter was lowered as a value",
                        ));
                    };
                    let Some((ptr, _)) = self.place_ptr(env, place)? else {
                        return Err(internal("a bridge reference has no storage"));
                    };
                    let value = self
                        .builder
                        .build_load(self.ty(param.ty), ptr, "bridge_float_ref")
                        .map_err(|error| error.to_string())?
                        .into_float_value();
                    self.validate_float(value, "bridge_float_ref_nan")?;
                }
            }
        }
        Ok(call)
    }

    /// Specification 011 section 11: a reference argument passes the address of
    /// its checked place, never a copy of its value. The checker already
    /// required a mutable root, so `place_ptr` always finds storage here.
    pub(crate) fn argument(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        arg: &TArg,
    ) -> Result<BasicMetadataValueEnum<'ctx>, String> {
        match arg {
            TArg::Value(value) => Ok(self.expr(env, loops, value)?.into()),
            TArg::Reference(place) => match self.place_ptr(env, place)? {
                Some((ptr, _)) => Ok(ptr.into()),
                None => Err(internal(
                    "a reference argument reached a place with no storage",
                )),
            },
        }
    }

    /// Resolves a method call receiver to the address methods actually
    /// expect: the pointee's storage after peeling every `Box<T>` layer the
    /// receiver's static type has (Specification 016 section 4.3). Unlike
    /// `place_ptr`'s own automatic dereference, a method-call receiver's
    /// `place.ty` is deliberately left un-dereferenced by the checker (see
    /// `check_method_call`'s comment on `TReceiver::Place`), so this always
    /// derefs further on top of whatever `place_ptr`/`place_value` returned,
    /// rather than relying on them to have already done it.
    ///
    /// `borrowed` is true exactly when the returned address is real
    /// caller-owned storage rather than a compiler-owned temporary the call
    /// result discards (Specification 010 section 15.3) -- a box's pointee is
    /// always real heap storage, so a `Box<T>` receiver is `borrowed` under
    /// the same rule as any other addressable place.
    pub(crate) fn receiver_ptr(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &TReceiver,
    ) -> Result<(PointerValue<'ctx>, bool), String> {
        match receiver {
            TReceiver::Place(place) => match self.place_ptr(env, place)? {
                Some((ptr, _)) => {
                    let (ptr, _) = self.deref_box_ptr(ptr, place.ty)?;
                    Ok((ptr, true))
                }
                None => {
                    let value = self.place_value(env, place)?;
                    self.receiver_from_value(place.ty, value)
                }
            },
            TReceiver::Value(value, ty) => {
                let value = self.expr(env, loops, value)?;
                self.receiver_from_value(*ty, value)
            }
        }
    }

    /// A receiver read as a bare value with no place of its own. A `Box<T>`
    /// value already *is* the address of real storage, so it is peeled
    /// directly with no compiler-owned temporary of its own; anything else
    /// gets compiler-owned storage the call result discards (Specification
    /// 010 section 15.3).
    pub(crate) fn receiver_from_value(
        &self,
        ty: Ty,
        value: BasicValueEnum<'ctx>,
    ) -> Result<(PointerValue<'ctx>, bool), String> {
        match ty {
            Ty::Box(id) => {
                let pointee = self.box_pointee(id);
                let (ptr, _) = self.deref_box_ptr(value.into_pointer_value(), pointee)?;
                Ok((ptr, false))
            }
            _ => Ok((self.materialize(value, SELF)?, false)),
        }
    }

    pub(crate) fn method_call(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        call: &TMethodCall,
    ) -> Result<inkwell::values::CallSiteValue<'ctx>, String> {
        let (receiver, borrowed) = self.receiver_ptr(env, loops, &call.receiver)?;
        // Compiler-owned storage is discarded when the call returns, so a
        // method that may assign through `self` must have reached the caller's
        // own storage. The checker enforces a mutable receiver root for exactly
        // that case; this catches the write being silently dropped if it ever
        // did not.
        if !borrowed && self.program.methods[call.method.index()].writes_receiver {
            return Err(internal(
                "a receiver-writing method call reached a receiver with no caller storage",
            ));
        }
        let mut args: Vec<BasicMetadataValueEnum> = vec![receiver.into()];
        for arg in &call.args {
            args.push(self.argument(env, loops, arg)?);
        }
        let callee = *self
            .methods
            .get(call.method.index())
            .ok_or_else(|| internal("a method call named a method that was not lowered"))?;
        self.invoke(callee, &args)
    }

    /// Builds a call and repeats the callee's ABI extension attributes on the
    /// call site, the way a C compiler does, so a bridge's sub-word arguments
    /// and result agree on both sides of the boundary.
    pub(crate) fn invoke(
        &self,
        callee: FunctionValue<'ctx>,
        args: &[BasicMetadataValueEnum<'ctx>],
    ) -> Result<inkwell::values::CallSiteValue<'ctx>, String> {
        let call = self
            .builder
            .build_call(callee, args, "call")
            .map_err(|error| error.to_string())?;
        zero_extend_subwords(self.context, callee.get_type(), |location, attribute| {
            call.add_attribute(location, attribute)
        });
        Ok(call)
    }

    pub(crate) fn descriptor_ptr(
        &self,
        value: BasicValueEnum<'ctx>,
        ty: BasicTypeEnum<'ctx>,
        name: &str,
    ) -> Result<PointerValue<'ctx>, String> {
        let slot = self.entry_alloca(ty, name)?;
        self.builder
            .build_store(slot, value)
            .map_err(|error| error.to_string())?;
        Ok(slot)
    }

    pub(crate) fn string_out_call(
        &self,
        function: FunctionValue<'ctx>,
        mut args: Vec<BasicMetadataValueEnum<'ctx>>,
        name: &str,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let out = self.entry_alloca(self.string_type().into(), name)?;
        args.insert(0, out.into());
        self.invoke(function, &args)?;
        self.builder
            .build_load(self.string_type(), out, name)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn view_out_call(
        &self,
        function: FunctionValue<'ctx>,
        mut args: Vec<BasicMetadataValueEnum<'ctx>>,
        name: &str,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let out = self.entry_alloca(self.view_type().into(), name)?;
        args.insert(0, out.into());
        self.invoke(function, &args)?;
        self.builder
            .build_load(self.view_type(), out, name)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn string_type(&self) -> StructType<'ctx> {
        let ptr = self.context.ptr_type(AddressSpace::default());
        self.context.struct_type(
            &[ptr.into(), self.usize_ty().into(), self.usize_ty().into()],
            false,
        )
    }

    pub(crate) fn unsigned_to_i64(
        &self,
        value: IntValue<'ctx>,
        name: &str,
    ) -> Result<IntValue<'ctx>, String> {
        let width = value.get_type().get_bit_width();
        match width.cmp(&64) {
            std::cmp::Ordering::Less => self
                .builder
                .build_int_z_extend(value, self.context.i64_type(), name)
                .map_err(|error| error.to_string()),
            std::cmp::Ordering::Equal => Ok(value),
            std::cmp::Ordering::Greater => self
                .builder
                .build_int_truncate(value, self.context.i64_type(), name)
                .map_err(|error| error.to_string()),
        }
    }

    pub(crate) fn view_type(&self) -> StructType<'ctx> {
        let ptr = self.context.ptr_type(AddressSpace::default());
        self.context
            .struct_type(&[ptr.into(), self.usize_ty().into()], false)
    }

    pub(crate) fn collection_type(&self) -> StructType<'ctx> {
        let ptr = self.context.ptr_type(AddressSpace::default());
        self.context.struct_type(
            &[ptr.into(), self.usize_ty().into(), self.usize_ty().into()],
            false,
        )
    }
}

pub(crate) fn build_module<'ctx>(
    context: &'ctx Context,
    program: &Program,
    module_name: &str,
    machine: &TargetMachine,
) -> Result<Module<'ctx>, String> {
    let triple = TargetMachine::get_default_triple();
    let module = context.create_module(module_name);
    let builder = context.create_builder();
    module.set_triple(&triple);
    // Specification 016 section 8.2: `box(expression)` needs the target's
    // real size and alignment for its pointee to call the runtime allocator
    // correctly, so this target data outlives the one-off value used just
    // above to set the module's data layout string.
    let target_data = machine.get_target_data();
    module.set_data_layout(&target_data.get_data_layout());

    // Named types come first: every function signature below may mention one.
    let (layout, sum_layout) = build_layout(context, &program.types, &program.sums)?;

    // Every function is declared before any body is lowered, so recursion and
    // forward calls do not depend on source or hash-map iteration order.
    let mut functions = HashMap::new();
    for (name, function) in &program.externs {
        let llvm_function = declare(
            context,
            &module,
            &function.symbol,
            function_type(
                context,
                &target_data,
                &layout,
                &sum_layout,
                &[],
                &function.params,
                function.result,
                true,
            ),
            None,
        );
        functions.insert(name.clone(), llvm_function);
    }
    for (name, function) in &program.funcs {
        let llvm_function = declare(
            context,
            &module,
            &format!("snacc_fn_{name}"),
            function_type(
                context,
                &target_data,
                &layout,
                &sum_layout,
                &[],
                &function.params,
                function.result,
                false,
            ),
            Some(Linkage::Internal),
        );
        functions.insert(name.clone(), llvm_function);
    }

    // Specification 010 section 15.3: a method is an internal function whose
    // hidden first parameter is a pointer to the receiver's storage. The symbol
    // is derived from the resolved receiver and method IDs and is not public
    // ABI.
    let receiver_param: BasicMetadataTypeEnum = context.ptr_type(AddressSpace::default()).into();
    let mut methods = Vec::with_capacity(program.methods.len());
    for (id, method) in program.methods.iter().enumerate() {
        methods.push(declare(
            context,
            &module,
            &format!("snacc_method_{}_{id}", method.receiver.0),
            function_type(
                context,
                &target_data,
                &layout,
                &sum_layout,
                &[receiver_param],
                &method.params,
                method.result,
                false,
            ),
            Some(Linkage::Internal),
        ));
    }

    let cg = Codegen {
        context,
        builder: &builder,
        module: &module,
        functions: &functions,
        methods: &methods,
        program,
        layout: &layout,
        sums: &sum_layout,
        target_data: &target_data,
    };

    for (name, function) in &program.funcs {
        let llvm_function = functions[name];
        let entry = context.append_basic_block(llvm_function, "entry");
        builder.position_at_end(entry);

        let mut env: Env = Vec::new();
        for (param, value) in function.params.iter().zip(llvm_function.get_params()) {
            env.push((param.name.clone(), param_slot(param, value)));
        }
        cg.body(&mut env, &function.body, function.result)?;
    }

    for (id, method) in program.methods.iter().enumerate() {
        let llvm_function = methods[id];
        let entry = context.append_basic_block(llvm_function, "entry");
        builder.position_at_end(entry);

        let receiver = llvm_function
            .get_first_param()
            .ok_or_else(|| internal("a lowered method has no receiver parameter"))?
            .into_pointer_value();
        let mut env: Env = vec![(SELF.to_string(), Slot::Mutable(receiver))];
        for (param, value) in method
            .params
            .iter()
            .zip(llvm_function.get_params().into_iter().skip(1))
        {
            env.push((param.name.clone(), param_slot(param, value)));
        }
        cg.body(&mut env, &method.body, method.result)?;
    }

    // The Rust runtime owns the platform entry point and calls this stable ABI
    // boundary. Snacc has no exit-code semantics yet, so success returns zero.
    let entry_type = context.i32_type().fn_type(&[], false);
    let entry_function = module.add_function("snacc_main", entry_type, None);
    let entry = context.append_basic_block(entry_function, "entry");
    builder.position_at_end(entry);
    let mut env: Env = Vec::new();
    let mut loops = Vec::new();
    let (_, terminated) = cg.block(&mut env, &mut loops, &program.body)?;
    if !terminated {
        let zero = context.i32_type().const_zero();
        builder
            .build_return(Some(&zero))
            .map_err(|error| error.to_string())?;
    }

    // Specification 010 section 19 phase 5 step 7: a module the backend built
    // that LLVM rejects is a compiler bug, not a property of the program.
    module
        .verify()
        .map_err(|error| internal(format!("LLVM rejected the generated module: {error}")))?;
    Ok(module)
}

/// Mirrors `Types::is_move_only` (Specification 016 section 5.3) using only
/// the lowering-time `Program` snapshot, since the backend does not carry the
/// checker's own `Types` table. Recursion only follows by-value fields/
/// members, which the checker's layout-cycle check already proved acyclic,
/// and a `Box<T>` edge short-circuits to `true` without recursing into its
/// pointee, so this needs no memoization -- the same reasoning
/// `Types::is_move_only` itself relies on.
pub(crate) fn is_move_only(program: &Program, ty: Ty) -> bool {
    match ty {
        Ty::Box(_) | Ty::String | Ty::List(_) | Ty::Map(_) | Ty::Set(_) => true,
        Ty::Array(id) => match &program.collections[id.index()] {
            crate::types::CollectionDef::Array { elem, .. } => is_move_only(program, *elem),
            _ => false,
        },
        Ty::User(id) => match &program.types[id.index()] {
            TypeDef::Represented { target, .. } => is_move_only(program, *target),
            TypeDef::Struct { fields, .. } | TypeDef::UnionMember { fields, .. } => {
                fields.iter().any(|(_, ty)| is_move_only(program, *ty))
            }
            TypeDef::Union { members, .. } => members
                .iter()
                .any(|member| is_move_only(program, Ty::User(*member))),
        },
        Ty::Sum(id) => program.sums[id.index()]
            .iter()
            .any(|member| is_move_only(program, *member)),
        _ => false,
    }
}

/// The environment key for a place root. `self` is a reserved word, so the
/// receiver never collides with a declared local.
pub(crate) fn root_name(root: &PlaceRoot) -> &str {
    match root {
        PlaceRoot::Local(name) => name,
        PlaceRoot::SelfRef => SELF,
    }
}

pub(crate) fn as_struct(value: BasicValueEnum<'_>) -> Result<StructValue<'_>, String> {
    match value {
        BasicValueEnum::StructValue(structure) => Ok(structure),
        _ => Err(internal("a field read reached a value without fields")),
    }
}

pub(crate) fn lookup<'ctx>(env: &Env<'ctx>, name: &str) -> Option<Slot<'ctx>> {
    env.iter()
        .rev()
        .find(|(bound, _)| bound == name)
        .map(|(_, slot)| *slot)
}
