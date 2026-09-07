//! Statement and block lowering (Specification 029 Phase 3).

use super::*;

pub(crate) enum ArmBinding<'t> {
    Union(&'t TTypeTest),
    Sum(&'t TSumTypeTest),
}

impl<'ctx> Codegen<'ctx, '_> {
    /// Lowers one function or method body and its return. Parameter drops are
    /// already the final entries of the body's unified cleanup plan.
    pub(crate) fn body(
        &self,
        env: &mut Env<'ctx>,
        block: &TBlock,
        result: Option<Ty>,
    ) -> Result<(), String> {
        let mut loops = Vec::new();
        let (value, terminated) = self.block(env, &mut loops, block)?;
        if terminated {
            return Ok(());
        }
        match (result, value) {
            (Some(_), Some(value)) => self.builder.build_return(Some(&value)),
            (None, _) => self.builder.build_return(None),
            (Some(_), None) => {
                return Err(internal("a declaration with a result produced no value"));
            }
        }
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Every `alloca` goes in the entry block so a mutable root declared inside
    /// a loop body does not grow the stack per iteration.
    pub(crate) fn entry_alloca(
        &self,
        ty: BasicTypeEnum<'ctx>,
        name: &str,
    ) -> Result<PointerValue<'ctx>, String> {
        let function = self.current_function();
        let resume = self
            .builder
            .get_insert_block()
            .expect("lowering always occurs inside a block");
        let entry = function
            .get_first_basic_block()
            .expect("every lowered function has an entry block");
        match entry.get_first_instruction() {
            Some(instruction) => self.builder.position_before(&instruction),
            None => self.builder.position_at_end(entry),
        }
        let slot = self
            .builder
            .build_alloca(ty, name)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(resume);
        Ok(slot)
    }

    /// Gives a value compiler-owned addressable storage, so a read-only method
    /// call on a temporary still receives a receiver pointer
    /// (Specification 010 section 15.3).
    pub(crate) fn materialize(
        &self,
        value: BasicValueEnum<'ctx>,
        name: &str,
    ) -> Result<PointerValue<'ctx>, String> {
        let slot = self.entry_alloca(value.get_type(), name)?;
        self.builder
            .build_store(slot, value)
            .map_err(|error| error.to_string())?;
        Ok(slot)
    }

    /// The address of a place, or `None` when its root is an SSA value with no
    /// addressable storage of its own. Field selectors lower to GEPs
    /// (Specification 010 section 15.3), and a `Box<T>` step along the way
    /// lowers to a load through the box's own stored pointer, crossing as
    /// many layers as `place.path` requires before each field selector --
    /// exactly mirroring `walk_fields`/`deref_box`'s automatic dereference in
    /// `checker.rs` (Specification 016 section 4.3).
    ///
    /// A caller occasionally wants a place dereferenced *beyond* what
    /// `place.path` alone implies -- a union/sum type-test subject or a
    /// `Box<T>`-to-`Ref<T>` lending argument, both of which the checker
    /// resolves by leaving `path` untouched but overwriting `place.ty` itself
    /// to the already-dereferenced type (see `check_arm_condition` and
    /// `check_reference_arg`). The trailing check below catches up to that by
    /// dereferencing further whenever the path walk's own natural result
    /// still disagrees with `place.ty`.
    pub(crate) fn place_ptr(
        &self,
        env: &Env<'ctx>,
        place: &Place,
    ) -> Result<Option<(PointerValue<'ctx>, BasicTypeEnum<'ctx>)>, String> {
        let Some(slot) = lookup(env, root_name(&place.root)) else {
            return Err(internal("a checked place root is not in scope"));
        };
        let mut ty = place.root_ty;
        let mut ptr = match slot {
            Slot::Mutable(ptr) => ptr,
            Slot::Value(value) => {
                if place.path.is_empty() && place.ty == ty {
                    // Nothing beyond the root's own SSA value is wanted; it
                    // has no address of its own to hand back.
                    return Ok(None);
                }
                let Ty::Box(id) = ty else {
                    // A non-box SSA root (an immutable struct local, say)
                    // still has no address; `place_value`'s `extract_value`
                    // fallback reads through it instead.
                    return Ok(None);
                };
                // The box's own SSA value already *is* the address of real
                // (heap) storage for its pointee (Specification 016 section
                // 4.3), unlike a `Slot::Mutable` root's address, which holds
                // a box pointer that still needs loading -- so this peels
                // exactly the outer layer "for free" before the loop below
                // (which only ever performs genuine loads) continues through
                // any further nested layers.
                ty = self.box_pointee(id);
                value.into_pointer_value()
            }
        };
        for &index in &place.path {
            (ptr, ty) = self.deref_box_ptr(ptr, ty)?;
            ptr = self
                .builder
                .build_struct_gep(self.struct_ty(ty)?, ptr, index as u32, "field")
                .map_err(|error| error.to_string())?;
            ty = self.field_ty(ty, index)?;
        }
        if ty != place.ty {
            (ptr, ty) = self.deref_box_ptr(ptr, ty)?;
            debug_assert_eq!(
                ty, place.ty,
                "a place's automatic dereference did not land on its own checked type"
            );
        }
        Ok(Some((ptr, self.ty(place.ty))))
    }

    /// Reads a place's current value.
    pub(crate) fn place_value(
        &self,
        env: &Env<'ctx>,
        place: &Place,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        if let Some((ptr, ty)) = self.place_ptr(env, place)? {
            return self
                .builder
                .build_load(ty, ptr, root_name(&place.root))
                .map_err(|error| error.to_string());
        }
        let Some(Slot::Value(mut value)) = lookup(env, root_name(&place.root)) else {
            return Err(internal("a checked place root is not in scope"));
        };
        for &index in &place.path {
            value = self
                .builder
                .build_extract_value(as_struct(value)?, index as u32, "field")
                .map_err(|error| error.to_string())?;
        }
        Ok(value)
    }

    /// Reads one LLVM field of a union place: index 0 is the tag, index
    /// `tag + 1` is that member's storage. A member field is only ever read
    /// here on a control-flow edge where its tag already matched.
    pub(crate) fn union_field(
        &self,
        env: &Env<'ctx>,
        place: &Place,
        index: u32,
        ty: BasicTypeEnum<'ctx>,
        name: &str,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        match self.place_ptr(env, place)? {
            Some((ptr, _)) => {
                let field = self
                    .builder
                    .build_struct_gep(self.struct_ty(place.ty)?, ptr, index, name)
                    .map_err(|error| error.to_string())?;
                self.builder
                    .build_load(ty, field, name)
                    .map_err(|error| error.to_string())
            }
            None => {
                let value = self.place_value(env, place)?;
                self.builder
                    .build_extract_value(as_struct(value)?, index, name)
                    .map_err(|error| error.to_string())
            }
        }
    }

    /// Lowers a block's statements in order, then its optional result value.
    /// Returns whether control flow left the block terminated, so no caller
    /// ever appends a second terminator or a merge branch to a dead block.
    pub(crate) fn block(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        block: &TBlock,
    ) -> Result<(Option<BasicValueEnum<'ctx>>, bool), String> {
        let scope = env.len();
        let mut terminated = false;
        for statement in &block.statements {
            if terminated {
                // RFC 008: source after a terminator is not lowered as
                // reachable code.
                break;
            }
            terminated = self.stmt(env, loops, statement)?;
        }
        let value = match (&block.result, terminated) {
            (Some(result), false) => Some(self.expr(env, loops, result)?),
            _ => None,
        };
        // Specification 025: only a normal fall-through executes the block's
        // normal cleanup plan. Early exits carry their own plan on the checked
        // statement, so no second cleanup is appended after a terminator.
        if !terminated {
            let error = match (value, block.result_ty) {
                (Some(value), Some(ty)) => self.result_error_condition(value, Some(ty))?,
                _ => None,
            };
            self.cleanup(env, loops, &block.cleanup, error)?;
        }
        env.truncate(scope);
        Ok((value, terminated))
    }

    /// Emits one checked cleanup plan in its stored execution order. `error`
    /// is absent for a statically successful exit and present for a return
    /// whose active result tag selects success or error. Deferred calls that
    /// consume a root disarm that root's later destruction in the applicable
    /// branch.
    pub(crate) fn cleanup(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        cleanup: &[TCleanup],
        error: Option<IntValue<'ctx>>,
    ) -> Result<(), String> {
        let mut always_consumed = HashSet::new();
        let mut error_consumed = HashSet::new();
        for entry in cleanup {
            match entry {
                TCleanup::Drop(place) if always_consumed.contains(&place.root) => {}
                TCleanup::Drop(place) if error_consumed.contains(&place.root) => {
                    let Some(error) = error else {
                        return Err(internal(
                            "an error-only deferred move reached a successful cleanup plan",
                        ));
                    };
                    let function = self.current_function();
                    let skip = self.context.append_basic_block(function, "skip_drop");
                    let run = self.context.append_basic_block(function, "run_drop");
                    let merge = self.context.append_basic_block(function, "drop_merge");
                    self.builder
                        .build_conditional_branch(error, skip, run)
                        .map_err(|err| err.to_string())?;
                    self.builder.position_at_end(run);
                    self.drop_places(env, std::slice::from_ref(place))?;
                    self.builder
                        .build_unconditional_branch(merge)
                        .map_err(|err| err.to_string())?;
                    self.builder.position_at_end(skip);
                    self.builder
                        .build_unconditional_branch(merge)
                        .map_err(|err| err.to_string())?;
                    self.builder.position_at_end(merge);
                }
                TCleanup::Drop(place) => {
                    self.drop_places(env, std::slice::from_ref(place))?;
                }
                TCleanup::Deferred(deferred) if !deferred.on_error => {
                    self.stmt(env, loops, &deferred.call)?;
                    always_consumed.extend(deferred.consumes.iter().cloned());
                }
                TCleanup::Deferred(deferred) => {
                    let Some(error) = error else {
                        continue;
                    };
                    let function = self.current_function();
                    let run = self.context.append_basic_block(function, "defer_error");
                    let skip = self.context.append_basic_block(function, "defer_skip");
                    let merge = self.context.append_basic_block(function, "defer_merge");
                    self.builder
                        .build_conditional_branch(error, run, skip)
                        .map_err(|err| err.to_string())?;
                    self.builder.position_at_end(run);
                    self.stmt(env, loops, &deferred.call)?;
                    self.builder
                        .build_unconditional_branch(merge)
                        .map_err(|err| err.to_string())?;
                    self.builder.position_at_end(skip);
                    self.builder
                        .build_unconditional_branch(merge)
                        .map_err(|err| err.to_string())?;
                    self.builder.position_at_end(merge);
                    error_consumed.extend(deferred.consumes.iter().cloned());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn result_error_condition(
        &self,
        value: BasicValueEnum<'ctx>,
        result: Option<Ty>,
    ) -> Result<Option<IntValue<'ctx>>, String> {
        let Some(Ty::Sum(sum)) = result else {
            return Ok(None);
        };
        let Some(error_index) = self
            .program
            .types
            .iter()
            .position(|def| def.name() == "Error")
        else {
            return Err(internal("the predeclared Error type is missing"));
        };
        let error_ty = Ty::User(TypeId(error_index as u32));
        let Some(error_tag) = self.program.sums[sum.index()]
            .iter()
            .position(|member| *member == error_ty)
        else {
            return Ok(None);
        };
        let tag = self
            .builder
            .build_extract_value(as_struct(value)?, 0, "return_tag")
            .map_err(|err| err.to_string())?
            .into_int_value();
        let is_error = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                tag,
                self.context.i32_type().const_int(error_tag as u64, false),
                "return_is_error",
            )
            .map_err(|err| err.to_string())?;
        Ok(Some(is_error))
    }

    /// Returns whether this statement terminated the current basic block.
    pub(crate) fn stmt(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        statement: &TStmt,
    ) -> Result<bool, String> {
        match statement {
            TStmt::Let {
                mutable,
                name,
                ty,
                value,
            } => {
                let value = self.expr(env, loops, value)?;
                if *mutable {
                    let ty = self.ty(*ty);
                    let slot = self.entry_alloca(ty, name)?;
                    self.builder
                        .build_store(slot, value)
                        .map_err(|error| error.to_string())?;
                    env.push((name.clone(), Slot::Mutable(slot)));
                } else {
                    env.push((name.clone(), Slot::Value(value)));
                }
                Ok(false)
            }
            TStmt::Assign {
                place,
                value,
                drop_before,
            } => {
                // Specification 016 section 6.3: the right operand evaluates
                // completely before the destination is touched at all.
                let value = self.expr(env, loops, value)?;
                let Some((ptr, ty)) = self.place_ptr(env, place)? else {
                    return Err(internal("an assignment reached a place with no storage"));
                };
                if *drop_before {
                    let old = self
                        .builder
                        .build_load(ty, ptr, "old")
                        .map_err(|error| error.to_string())?;
                    self.drop_value(place.ty, old)?;
                }
                self.builder
                    .build_store(ptr, value)
                    .map_err(|error| error.to_string())?;
                Ok(false)
            }
            TStmt::SequenceIndexAssign {
                receiver,
                index,
                value,
                elem,
            } => {
                self.lower_sequence_index_assign(env, loops, receiver, index, value, *elem)?;
                Ok(false)
            }
            TStmt::MethodCall(call) => {
                self.method_call(env, loops, call)?;
                Ok(false)
            }
            TStmt::ListPush {
                receiver,
                value,
                elem,
            } => {
                self.lower_list_push(env, loops, receiver, value, *elem)?;
                Ok(false)
            }
            TStmt::ListClear { receiver, elem } => {
                self.lower_list_clear(env, receiver, *elem)?;
                Ok(false)
            }
            TStmt::ListInsert {
                receiver,
                index,
                value,
                elem,
            } => {
                self.lower_list_insert(env, loops, receiver, index, value, *elem)?;
                Ok(false)
            }
            TStmt::ListReserve {
                receiver,
                minimum,
                elem,
            } => {
                self.lower_list_reserve(env, loops, receiver, minimum, *elem)?;
                Ok(false)
            }
            TStmt::MapClear {
                receiver,
                key_ty,
                value_ty,
            } => {
                self.lower_map_clear(env, receiver, *key_ty, *value_ty)?;
                Ok(false)
            }
            TStmt::MapReserve {
                receiver,
                minimum,
                key_ty,
                value_ty,
            } => {
                self.lower_map_reserve(env, loops, receiver, minimum, *key_ty, *value_ty)?;
                Ok(false)
            }
            TStmt::SetClear { receiver, elem } => {
                self.lower_set_clear(env, receiver, *elem)?;
                Ok(false)
            }
            TStmt::SetReserve {
                receiver,
                minimum,
                elem,
            } => {
                self.lower_set_reserve(env, loops, receiver, minimum, *elem)?;
                Ok(false)
            }
            TStmt::While { condition, body } => {
                let function = self.current_function();
                let condition_block = self.context.append_basic_block(function, "while_condition");
                let body_block = self.context.append_basic_block(function, "while_body");
                let exit_block = self.context.append_basic_block(function, "while_exit");
                self.builder
                    .build_unconditional_branch(condition_block)
                    .map_err(|error| error.to_string())?;

                self.builder.position_at_end(condition_block);
                let test = self.condition(env, loops, condition)?;
                self.builder
                    .build_conditional_branch(test, body_block, exit_block)
                    .map_err(|error| error.to_string())?;

                self.builder.position_at_end(body_block);
                loops.push(exit_block);
                let (_, terminated) = self.block(env, loops, body)?;
                loops.pop();
                if !terminated {
                    self.builder
                        .build_unconditional_branch(condition_block)
                        .map_err(|error| error.to_string())?;
                }

                // The exit block is always reachable: the condition branches to
                // it when false.
                self.builder.position_at_end(exit_block);
                Ok(false)
            }
            TStmt::For {
                value_name,
                value_ty,
                key_name,
                key_ty,
                iterable,
                collection_ty,
                body,
            } => {
                let collection = self.expr(env, loops, iterable)?.into_struct_value();
                let is_map = matches!(collection_ty, Ty::Map(_));
                let is_set = matches!(collection_ty, Ty::Set(_));
                let collection_slot = if is_map || is_set {
                    let slot = self.entry_alloca(self.collection_type().into(), "for_map")?;
                    self.builder
                        .build_store(slot, collection)
                        .map_err(|error| error.to_string())?;
                    Some(slot)
                } else {
                    None
                };
                let ptr = if !is_map && !is_set {
                    Some(
                        self.builder
                            .build_extract_value(collection, 0, "for_ptr")
                            .map_err(|error| error.to_string())?
                            .into_pointer_value(),
                    )
                } else {
                    None
                };
                let len = if *collection_ty == Ty::ViewUnicode {
                    let view = self.descriptor_ptr(
                        collection.into(),
                        self.view_type().into(),
                        "for_view",
                    )?;
                    self.invoke(
                        self.runtime_import(Family::View, "length_unicode", Ty::Nil, Ty::Nil)?,
                        &[view.into()],
                    )?
                    .try_as_basic_value()
                    .expect_basic("Unicode view length returns an integer")
                    .into_int_value()
                } else {
                    self.builder
                        .build_extract_value(collection, 1, "for_len")
                        .map_err(|error| error.to_string())?
                        .into_int_value()
                };
                let function = self.current_function();
                let condition_block = self.context.append_basic_block(function, "for_condition");
                let body_block = self.context.append_basic_block(function, "for_body");
                let exit_block = self.context.append_basic_block(function, "for_exit");
                let entry = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| internal("for loop has no insertion block"))?;
                self.builder
                    .build_unconditional_branch(condition_block)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(condition_block);
                let index = self
                    .builder
                    .build_phi(self.context.i64_type(), "for_index")
                    .map_err(|error| error.to_string())?;
                let zero = self.context.i64_type().const_zero();
                index.add_incoming(&[(&zero, entry)]);
                let current = index.as_basic_value().into_int_value();
                let more = self
                    .builder
                    .build_int_compare(IntPredicate::ULT, current, len, "for_more")
                    .map_err(|error| error.to_string())?;
                self.builder
                    .build_conditional_branch(more, body_block, exit_block)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(body_block);
                let item_slot = if is_map {
                    let key_ty = key_ty.ok_or_else(|| internal("map loop has no key type"))?;
                    if self.map_uses_raw_value(*value_ty) {
                        let out = self.entry_alloca(self.ty(*value_ty), "map_loop_value")?;
                        self.invoke(
                            self.runtime_import(Family::MapRaw, "value_at", key_ty, Ty::Nil)?,
                            &[
                                collection_slot
                                    .ok_or_else(|| internal("map loop has no descriptor slot"))?
                                    .into(),
                                current.into(),
                                out.into(),
                                self.usize_ty()
                                    .const_int(self.size_align(*value_ty).0, false)
                                    .into(),
                            ],
                        )?;
                        Slot::Mutable(out)
                    } else {
                        let item = self
                            .invoke(
                                self.runtime_import(Family::Map, "value_at", key_ty, *value_ty)?,
                                &[
                                    collection_slot
                                        .ok_or_else(|| internal("map loop has no descriptor slot"))?
                                        .into(),
                                    current.into(),
                                ],
                            )?
                            .try_as_basic_value()
                            .expect_basic("map iteration value lookup returns the map value");
                        Slot::Value(item)
                    }
                } else if is_set {
                    if *value_ty == Ty::String {
                        let out = self.entry_alloca(self.string_type().into(), "set_loop_value")?;
                        self.invoke(
                            self.runtime_import(Family::Set, "at", *value_ty, Ty::Nil)?,
                            &[
                                out.into(),
                                collection_slot
                                    .ok_or_else(|| internal("set loop has no descriptor slot"))?
                                    .into(),
                                current.into(),
                            ],
                        )?;
                        Slot::Value(
                            self.builder
                                .build_load(self.string_type(), out, "set_loop_value")
                                .map_err(|error| error.to_string())?,
                        )
                    } else {
                        let item = self
                            .invoke(
                                self.runtime_import(Family::Set, "at", *value_ty, Ty::Nil)?,
                                &[
                                    collection_slot
                                        .ok_or_else(|| internal("set loop has no descriptor slot"))?
                                        .into(),
                                    current.into(),
                                ],
                            )?
                            .try_as_basic_value()
                            .expect_basic("set iteration lookup returns the set element");
                        Slot::Value(item)
                    }
                } else if *collection_ty == Ty::ViewUnicode {
                    let scalar = self
                        .invoke(
                            self.runtime_import(Family::View, "at_unicode", Ty::Nil, Ty::Nil)?,
                            &[
                                self.descriptor_ptr(
                                    collection.into(),
                                    self.view_type().into(),
                                    "for_view_at",
                                )?
                                .into(),
                                current.into(),
                            ],
                        )?
                        .try_as_basic_value()
                        .expect_basic("Unicode view lookup returns an integer")
                        .into_int_value();
                    let item = self
                        .builder
                        .build_int_truncate(scalar, self.context.i32_type(), "for_unicode_item")
                        .map_err(|error| error.to_string())?;
                    Slot::Value(item.into())
                } else {
                    let item_ptr = unsafe {
                        self.builder.build_gep(
                            self.ty(*value_ty),
                            ptr.ok_or_else(|| internal("sequence loop has no storage"))?,
                            &[current],
                            "for_item_ptr",
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    if is_move_only(self.program, *value_ty) {
                        Slot::Mutable(item_ptr)
                    } else {
                        Slot::Value(
                            self.builder
                                .build_load(self.ty(*value_ty), item_ptr, "for_item")
                                .map_err(|error| error.to_string())?,
                        )
                    }
                };
                let scope = env.len();
                if is_map {
                    let key_ty = key_ty.ok_or_else(|| internal("map loop has no key type"))?;
                    let key = if self.map_uses_raw_value(*value_ty) && key_ty == Ty::String {
                        let out = self.entry_alloca(self.string_type().into(), "map_loop_key")?;
                        self.invoke(
                            self.runtime_import(Family::MapRaw, "key_at", key_ty, Ty::Nil)?,
                            &[
                                out.into(),
                                collection_slot
                                    .ok_or_else(|| internal("raw map loop has no descriptor slot"))?
                                    .into(),
                                current.into(),
                            ],
                        )?;
                        self.builder
                            .build_load(self.string_type(), out, "map_loop_key")
                            .map_err(|error| error.to_string())?
                    } else if self.map_uses_raw_value(*value_ty) {
                        self.invoke(
                            self.runtime_import(Family::MapRaw, "key_at", key_ty, Ty::Nil)?,
                            &[
                                collection_slot
                                    .ok_or_else(|| internal("raw map loop has no descriptor slot"))?
                                    .into(),
                                current.into(),
                            ],
                        )?
                        .try_as_basic_value()
                        .expect_basic("raw map iteration key lookup returns the map key")
                    } else if key_ty == Ty::String {
                        let out = self.entry_alloca(self.string_type().into(), "map_loop_key")?;
                        self.invoke(
                            self.runtime_import(Family::Map, "key_at", key_ty, *value_ty)?,
                            &[
                                out.into(),
                                collection_slot
                                    .ok_or_else(|| internal("map loop has no descriptor slot"))?
                                    .into(),
                                current.into(),
                            ],
                        )?;
                        self.builder
                            .build_load(self.string_type(), out, "map_loop_key")
                            .map_err(|error| error.to_string())?
                    } else {
                        self.invoke(
                            self.runtime_import(Family::Map, "key_at", key_ty, *value_ty)?,
                            &[
                                collection_slot
                                    .ok_or_else(|| internal("map loop has no descriptor slot"))?
                                    .into(),
                                current.into(),
                            ],
                        )?
                        .try_as_basic_value()
                        .expect_basic("map iteration key lookup returns the map key")
                    };
                    env.push((
                        key_name
                            .clone()
                            .ok_or_else(|| internal("map loop has no key binding"))?,
                        Slot::Value(key),
                    ));
                }
                env.push((value_name.clone(), item_slot));
                let (_, terminated) = self.block(env, loops, body)?;
                env.truncate(scope);
                if !terminated {
                    let next = self
                        .builder
                        .build_int_add(
                            current,
                            self.context.i64_type().const_int(1, false),
                            "for_next",
                        )
                        .map_err(|error| error.to_string())?;
                    self.builder
                        .build_unconditional_branch(condition_block)
                        .map_err(|error| error.to_string())?;
                    index.add_incoming(&[(&next, self.builder.get_insert_block().unwrap())]);
                }
                self.builder.position_at_end(exit_block);
                Ok(false)
            }
            TStmt::Break { cleanup } => {
                self.cleanup(env, loops, cleanup, None)?;
                let exit = *loops
                    .last()
                    .ok_or("checker allowed a 'break' outside every loop")?;
                self.builder
                    .build_unconditional_branch(exit)
                    .map_err(|error| error.to_string())?;
                Ok(true)
            }
            TStmt::If(form) => {
                let function = self.current_function();
                let merge = self.context.append_basic_block(function, "if_merge");
                let mut reaches_merge = false;
                for (condition, body) in &form.arms {
                    let (test, bound) = self.arm_condition(env, loops, condition)?;
                    let then_block = self.context.append_basic_block(function, "if_then");
                    let next_block = self.context.append_basic_block(function, "if_next");
                    self.builder
                        .build_conditional_branch(test, then_block, next_block)
                        .map_err(|error| error.to_string())?;
                    self.builder.position_at_end(then_block);
                    let scope = env.len();
                    self.bind(env, bound)?;
                    let (_, terminated) = self.block(env, loops, body)?;
                    env.truncate(scope);
                    if !terminated {
                        self.builder
                            .build_unconditional_branch(merge)
                            .map_err(|error| error.to_string())?;
                        reaches_merge = true;
                    }
                    self.builder.position_at_end(next_block);
                }
                // The builder now sits in the block reached when no arm
                // matched: the `else` body, a direct path to the merge, or --
                // for a proven-exhaustive type-test chain -- nothing at all.
                match (&form.else_branch, form.exhaustive) {
                    (Some(body), _) => {
                        let (_, terminated) = self.block(env, loops, body)?;
                        if !terminated {
                            self.builder
                                .build_unconditional_branch(merge)
                                .map_err(|error| error.to_string())?;
                            reaches_merge = true;
                        }
                    }
                    (None, true) => {
                        self.exhausted()?;
                    }
                    (None, false) => {
                        self.builder
                            .build_unconditional_branch(merge)
                            .map_err(|error| error.to_string())?;
                        reaches_merge = true;
                    }
                }
                self.builder.position_at_end(merge);
                if reaches_merge {
                    Ok(false)
                } else {
                    // Every branch terminated, so nothing reaches the merge. It
                    // still needs a terminator to verify.
                    self.builder
                        .build_unreachable()
                        .map_err(|error| error.to_string())?;
                    Ok(true)
                }
            }
            TStmt::Call(name, args) => {
                self.call(env, loops, name, args)?;
                Ok(false)
            }
            // Specification 026 section 10: the result -- if any -- is
            // evaluated and materialized before the checked cleanup plan
            // runs, so a moved-out local is read for its value here before
            // (not being) destroyed below; `drops` already excludes any root
            // the checker transferred to the caller. Shares no lowering path
            // with `body`'s own implicit-fallthrough exit beyond `drop_places`
            // itself, but produces the exact same instructions that path
            // would for the same result and cleanup facts.
            TStmt::Return {
                value,
                result,
                cleanup,
            } => {
                let value = match value {
                    Some(expression) => Some(self.expr(env, loops, expression)?),
                    None => None,
                };
                let error = match (value, *result) {
                    (Some(value), Some(result)) => {
                        self.result_error_condition(value, Some(result))?
                    }
                    _ => None,
                };
                self.cleanup(env, loops, cleanup, error)?;
                match &value {
                    Some(value) => self.builder.build_return(Some(value)),
                    None => self.builder.build_return(None),
                }
                .map_err(|error| error.to_string())?;
                Ok(true)
            }
            TStmt::ReturnOnError {
                value,
                sum,
                result,
                cleanup,
            } => {
                self.lower_return_on_error(env, loops, value, *sum, Ty::Nil, *result, cleanup)?;
                Ok(false)
            }
            TStmt::Expr(expression) => {
                self.expr(env, loops, expression)?;
                Ok(false)
            }
        }
    }

    /// The fall-through edge of a proven-exhaustive type-test chain. Every
    /// direct member of the union was tested, so no tag can arrive here; it is
    /// reachable only if the checker's coverage proof was wrong, which `unreachable`
    /// states rather than silently producing a value.
    pub(crate) fn exhausted(&self) -> Result<(), String> {
        self.builder
            .build_unreachable()
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// An `if`/`elseif` arm's condition. A type test compares the tested
    /// place's stored tag against the tag the checker resolved for a named
    /// union (Specification 010 section 15.4) or lowering computes on the fly
    /// from canonical member order for an inline sum (Specification 018
    /// Phase 4 item 1). The returned binding, when present, must be loaded on
    /// the successful edge only.
    pub(crate) fn arm_condition<'t>(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        condition: &'t TCondition,
    ) -> Result<(IntValue<'ctx>, Option<ArmBinding<'t>>), String> {
        match condition {
            TCondition::Expr(expression) => Ok((self.condition(env, loops, expression)?, None)),
            TCondition::Test(test) => {
                let tag =
                    self.union_field(env, &test.place, 0, self.context.i32_type().into(), "tag")?;
                let expected = self
                    .context
                    .i32_type()
                    .const_int(u64::from(test.tag), false);
                let compared = self
                    .builder
                    .build_int_compare(IntPredicate::EQ, tag.into_int_value(), expected, "is")
                    .map_err(|error| error.to_string())?;
                Ok((
                    compared,
                    test.binding.is_some().then_some(ArmBinding::Union(test)),
                ))
            }
            TCondition::SumTest(test) => {
                let member_tag = self.sum_member_tag(test.sum, test.member)?;
                let tag =
                    self.union_field(env, &test.place, 0, self.context.i32_type().into(), "tag")?;
                let expected = self
                    .context
                    .i32_type()
                    .const_int(u64::from(member_tag), false);
                let compared = self
                    .builder
                    .build_int_compare(IntPredicate::EQ, tag.into_int_value(), expected, "is")
                    .map_err(|error| error.to_string())?;
                Ok((
                    compared,
                    test.binding.is_some().then_some(ArmBinding::Sum(test)),
                ))
            }
        }
    }

    /// Loads a successful type test's binding. Called only after the builder is
    /// positioned in the arm's `then` block, so an inactive member is never
    /// read on the failing edge.
    pub(crate) fn bind(
        &self,
        env: &mut Env<'ctx>,
        bound: Option<ArmBinding<'_>>,
    ) -> Result<(), String> {
        match bound {
            None => Ok(()),
            Some(ArmBinding::Union(test)) => {
                let Some((name, ty)) = &test.binding else {
                    return Ok(());
                };
                let value = self.union_field(env, &test.place, test.tag + 1, self.ty(*ty), name)?;
                env.push((name.clone(), Slot::Value(value)));
                Ok(())
            }
            Some(ArmBinding::Sum(test)) => {
                let Some((name, ty)) = &test.binding else {
                    return Ok(());
                };
                let member_tag = self.sum_member_tag(test.sum, test.member)?;
                let value =
                    self.union_field(env, &test.place, member_tag + 1, self.ty(*ty), name)?;
                env.push((name.clone(), Slot::Value(value)));
                Ok(())
            }
        }
    }

    /// Lowers Specification 021 truthiness without changing the value's
    /// representation. Only `Bool(false)` and an active `Nil` are falsey;
    /// numeric zeroes, aggregates, boxes, strings, and views are truthy.
    pub(crate) fn truthiness_value(
        &self,
        ty: Ty,
        value: BasicValueEnum<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        match ty {
            Ty::Bool => {
                let compared = self
                    .builder
                    .build_int_compare(
                        IntPredicate::NE,
                        value.into_int_value(),
                        self.context.i8_type().const_zero(),
                        "truthy_bool",
                    )
                    .map_err(|error| error.to_string())?;
                self.builder
                    .build_int_z_extend(compared, self.context.i8_type(), "truthy_bool_byte")
                    .map_err(|error| error.to_string())
            }
            Ty::Nil => Ok(self.context.i8_type().const_zero()),
            Ty::User(id) => match self.def(id) {
                TypeDef::Represented { target, .. } => self.truthiness_value(*target, value),
                TypeDef::Union { members, .. } => {
                    let aggregate = as_struct(value)?;
                    let tag = self
                        .builder
                        .build_extract_value(aggregate, 0, "truthy_tag")
                        .map_err(|error| error.to_string())?
                        .into_int_value();
                    let mut result = self.context.i8_type().const_zero();
                    for (index, member) in members.iter().enumerate().rev() {
                        let member_truth = match self.def(*member) {
                            TypeDef::UnionMember { nil: true, .. } => {
                                self.context.i8_type().const_zero()
                            }
                            _ => self.context.i8_type().const_int(1, false),
                        };
                        let selected = self
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                self.context.i32_type().const_int(index as u64, false),
                                "truthy_member_tag",
                            )
                            .map_err(|error| error.to_string())?;
                        result = self
                            .builder
                            .build_select(selected, member_truth, result, "truthy_union")
                            .map_err(|error| error.to_string())?
                            .into_int_value();
                    }
                    Ok(result)
                }
                TypeDef::UnionMember { nil, .. } => {
                    Ok(self.context.i8_type().const_int((!nil) as u64, false))
                }
                TypeDef::Struct { .. } => Ok(self.context.i8_type().const_int(1, false)),
            },
            Ty::Sum(id) => {
                let aggregate = as_struct(value)?;
                let tag = self
                    .builder
                    .build_extract_value(aggregate, 0, "truthy_tag")
                    .map_err(|error| error.to_string())?
                    .into_int_value();
                let mut result = self.context.i8_type().const_zero();
                for (index, member) in self.program.sums[id.index()].iter().enumerate().rev() {
                    let member_value = self
                        .builder
                        .build_extract_value(aggregate, (index + 1) as u32, "truthy_member")
                        .map_err(|error| error.to_string())?;
                    let member_truth = self.truthiness_value(*member, member_value)?;
                    let selected = self
                        .builder
                        .build_int_compare(
                            IntPredicate::EQ,
                            tag,
                            self.context.i32_type().const_int(index as u64, false),
                            "truthy_member_tag",
                        )
                        .map_err(|error| error.to_string())?;
                    result = self
                        .builder
                        .build_select(selected, member_truth, result, "truthy_sum")
                        .map_err(|error| error.to_string())?
                        .into_int_value();
                }
                Ok(result)
            }
            _ => Ok(self.context.i8_type().const_int(1, false)),
        }
    }

    pub(crate) fn condition(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        condition: &TExpr,
    ) -> Result<IntValue<'ctx>, String> {
        let value = self.expr(env, loops, condition)?.into_int_value();
        self.builder
            .build_int_compare(
                IntPredicate::NE,
                value,
                self.context.i8_type().const_zero(),
                "condition",
            )
            .map_err(|error| error.to_string())
    }
}
