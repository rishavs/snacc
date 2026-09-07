//! List, map, and set operation lowering (Specification 029 Phase 3).
//!
//! Each method here owns one collection match arm's body, verbatim as it
//! lowered inside `stmt`/`expr`; the arms themselves stay in place as
//! one-line dispatchers so both matches remain exhaustive.

use super::*;

impl<'ctx> Codegen<'ctx, '_> {
    pub(crate) fn lower_sequence_index_assign(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        index: &TExpr,
        value: &TExpr,
        elem: Ty,
    ) -> Result<(), String> {
        // The value is completely evaluated before the destination
        // collection is touched, matching ordinary assignment and
        // preventing a replacement from invalidating its source.
        let index = self.expr(env, loops, index)?.into_int_value();
        let value = self.expr(env, loops, value)?;
        let Some((collection_ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal(
                "sequence indexed assignment reached a place with no storage",
            ));
        };
        let descriptor = self
            .builder
            .build_load(
                self.collection_type(),
                collection_ptr,
                "sequence_descriptor",
            )
            .map_err(|error| error.to_string())?
            .into_struct_value();
        let data = self
            .builder
            .build_extract_value(descriptor, 0, "sequence_data")
            .map_err(|error| error.to_string())?
            .into_pointer_value();
        let len = self
            .builder
            .build_extract_value(descriptor, 1, "sequence_length")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let nonnegative = self
            .builder
            .build_int_compare(
                IntPredicate::SGE,
                index,
                self.context.i64_type().const_zero(),
                "assignment_index_nonnegative",
            )
            .map_err(|error| error.to_string())?;
        let in_range = self
            .builder
            .build_int_compare(IntPredicate::ULT, index, len, "assignment_index_in_range")
            .map_err(|error| error.to_string())?;
        let valid = self
            .builder
            .build_and(nonnegative, in_range, "assignment_index_valid")
            .map_err(|error| error.to_string())?;
        let function = self.current_function();
        let valid_block = self
            .context
            .append_basic_block(function, "index_assign_valid");
        let invalid_block = self
            .context
            .append_basic_block(function, "index_assign_invalid");
        self.builder
            .build_conditional_branch(valid, valid_block, invalid_block)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(invalid_block);
        self.invoke(
            self.runtime_import(Family::Fail, "bounds_fail", Ty::Nil, Ty::Nil)?,
            &[],
        )?;
        self.builder
            .build_unreachable()
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(valid_block);
        // Safety: the checker restricts this statement to an owning
        // array/list and the preceding branch proves the index range.
        let element_ptr = unsafe {
            self.builder
                .build_gep(self.ty(elem), data, &[index], "assigned_element")
        }
        .map_err(|error| error.to_string())?;
        if is_move_only(self.program, elem) {
            let old = self
                .builder
                .build_load(self.ty(elem), element_ptr, "replaced_element")
                .map_err(|error| error.to_string())?;
            self.drop_value(elem, old)?;
        }
        self.builder
            .build_store(element_ptr, value)
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn lower_list_push(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        value: &TExpr,
        elem: Ty,
    ) -> Result<(), String> {
        // Evaluate the argument before taking the receiver address,
        // matching assignment's left-to-right effect ordering.
        let value = self.expr(env, loops, value)?;
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("List.push reached a place with no storage"));
        };
        if is_scalar_collection_element(elem) {
            let function = self.runtime_import(Family::List, "push", elem, Ty::Nil)?;
            self.invoke(function, &[ptr.into(), value.into()])?;
        } else {
            let slot = self.entry_alloca(self.ty(elem), "list_push_value")?;
            self.builder
                .build_store(slot, value)
                .map_err(|error| error.to_string())?;
            let (size, align) = self.size_align(elem);
            self.invoke(
                self.runtime_import(Family::ListRaw, "push", Ty::Nil, Ty::Nil)?,
                &[
                    ptr.into(),
                    slot.into(),
                    self.usize_ty().const_int(size, false).into(),
                    self.usize_ty().const_int(align, false).into(),
                ],
            )?;
        }
        Ok(())
    }

    pub(crate) fn lower_list_clear(
        &self,
        env: &Env<'ctx>,
        receiver: &Place,
        elem: Ty,
    ) -> Result<(), String> {
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("List.clear reached a place with no storage"));
        };
        if is_move_only(self.program, elem) {
            let descriptor = self
                .builder
                .build_load(self.collection_type(), ptr, "list_descriptor")
                .map_err(|error| error.to_string())?
                .into_struct_value();
            let data = self
                .builder
                .build_extract_value(descriptor, 0, "list_data")
                .map_err(|error| error.to_string())?
                .into_pointer_value();
            let len = self
                .builder
                .build_extract_value(descriptor, 1, "list_length")
                .map_err(|error| error.to_string())?
                .into_int_value();
            self.drop_sequence_elements(data, len, elem)?;
        }
        if is_scalar_collection_element(elem) {
            self.invoke(
                self.runtime_import(Family::ListFixed, "clear", Ty::Nil, Ty::Nil)?,
                &[ptr.into()],
            )?;
        } else {
            self.invoke(
                self.runtime_import(Family::ListRaw, "clear", Ty::Nil, Ty::Nil)?,
                &[ptr.into()],
            )?;
        }
        Ok(())
    }

    pub(crate) fn lower_list_insert(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        index: &TExpr,
        value: &TExpr,
        elem: Ty,
    ) -> Result<(), String> {
        let index = self.expr(env, loops, index)?;
        let value = self.expr(env, loops, value)?;
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("List.insert reached a place with no storage"));
        };
        if is_scalar_collection_element(elem) {
            self.invoke(
                self.runtime_import(Family::List, "insert", elem, Ty::Nil)?,
                &[ptr.into(), index.into(), value.into()],
            )?;
        } else {
            let slot = self.entry_alloca(self.ty(elem), "list_insert_value")?;
            self.builder
                .build_store(slot, value)
                .map_err(|error| error.to_string())?;
            let (size, align) = self.size_align(elem);
            self.invoke(
                self.runtime_import(Family::ListRaw, "insert", Ty::Nil, Ty::Nil)?,
                &[
                    ptr.into(),
                    index.into(),
                    slot.into(),
                    self.usize_ty().const_int(size, false).into(),
                    self.usize_ty().const_int(align, false).into(),
                ],
            )?;
        }
        Ok(())
    }

    pub(crate) fn lower_list_reserve(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        minimum: &TExpr,
        elem: Ty,
    ) -> Result<(), String> {
        let minimum = self.expr(env, loops, minimum)?;
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("List.reserve reached a place with no storage"));
        };
        let (size, align) = self.size_align(elem);
        self.invoke(
            self.runtime_import(Family::ListFixed, "reserve", Ty::Nil, Ty::Nil)?,
            &[
                ptr.into(),
                minimum.into(),
                self.usize_ty().const_int(size, false).into(),
                self.usize_ty().const_int(align, false).into(),
            ],
        )?;
        Ok(())
    }

    pub(crate) fn lower_map_clear(
        &self,
        env: &Env<'ctx>,
        receiver: &Place,
        key_ty: Ty,
        value_ty: Ty,
    ) -> Result<(), String> {
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("Map.clear reached a place with no storage"));
        };
        let descriptor = self
            .builder
            .build_load(self.collection_type(), ptr, "map_clear_descriptor")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        if self.map_uses_raw_value(value_ty) {
            self.drop_map_values(key_ty, value_ty, descriptor)?;
        }
        if self.map_uses_raw_value(value_ty) {
            self.invoke(
                self.runtime_import(Family::MapRaw, "clear", key_ty, Ty::Nil)?,
                &[ptr.into()],
            )?;
        } else {
            self.invoke(
                self.runtime_import(Family::Map, "clear", key_ty, value_ty)?,
                &[ptr.into()],
            )?;
        }
        Ok(())
    }

    pub(crate) fn lower_map_reserve(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        minimum: &TExpr,
        key_ty: Ty,
        value_ty: Ty,
    ) -> Result<(), String> {
        let minimum = self.expr(env, loops, minimum)?;
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("Map.reserve reached a place with no storage"));
        };
        if self.map_uses_raw_value(value_ty) {
            self.invoke(
                self.runtime_import(Family::MapRaw, "reserve", key_ty, Ty::Nil)?,
                &[ptr.into(), minimum.into()],
            )?;
        } else {
            self.invoke(
                self.runtime_import(Family::Map, "reserve", key_ty, value_ty)?,
                &[ptr.into(), minimum.into()],
            )?;
        }
        Ok(())
    }

    pub(crate) fn lower_set_clear(
        &self,
        env: &Env<'ctx>,
        receiver: &Place,
        elem: Ty,
    ) -> Result<(), String> {
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("Set.clear reached a place with no storage"));
        };
        self.invoke(
            self.runtime_import(Family::Set, "clear", elem, Ty::Nil)?,
            &[ptr.into()],
        )?;
        Ok(())
    }

    pub(crate) fn lower_set_reserve(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        minimum: &TExpr,
        elem: Ty,
    ) -> Result<(), String> {
        let minimum = self.expr(env, loops, minimum)?;
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("Set.reserve reached a place with no storage"));
        };
        self.invoke(
            self.runtime_import(Family::Set, "reserve", elem, Ty::Nil)?,
            &[ptr.into(), minimum.into()],
        )?;
        Ok(())
    }

    pub(crate) fn lower_map_contains(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &TExpr,
        key: &TExpr,
        key_ty: Ty,
        value_ty: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let receiver = self.expr(env, loops, receiver)?;
        let key = self.expr(env, loops, key)?;
        let receiver =
            self.descriptor_ptr(receiver, self.collection_type().into(), "map_contains_map")?;
        let key = if key_ty == Ty::ViewByte {
            self.descriptor_ptr(key, self.view_type().into(), "map_contains_key")?
                .into()
        } else {
            key.into()
        };
        if self.map_uses_raw_value(value_ty) {
            return Ok(self
                .invoke(
                    self.runtime_import(Family::MapRaw, "contains", key_ty, Ty::Nil)?,
                    &[receiver.into(), key],
                )?
                .try_as_basic_value()
                .expect_basic("raw Map.contains returns Bool"));
        }
        Ok(self
            .invoke(
                self.runtime_import(Family::Map, "contains", key_ty, Ty::Nil)?,
                &[receiver.into(), key],
            )?
            .try_as_basic_value()
            .expect_basic("Map.contains returns Bool"))
    }

    pub(crate) fn lower_map_insert(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        key: &TExpr,
        value: &TExpr,
        key_ty: Ty,
        value_ty: Ty,
        require_existing: bool,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let key = self.expr(env, loops, key)?;
        let value = self.expr(env, loops, value)?;
        let key = if key_ty == Ty::String {
            self.descriptor_ptr(key, self.string_type().into(), "map_insert_key")?
                .into()
        } else {
            key.into()
        };
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("Map.insert reached a place with no storage"));
        };
        if self.map_uses_raw_value(value_ty) {
            let value_slot = self.entry_alloca(self.ty(value_ty), "map_value")?;
            let old_slot = self.entry_alloca(self.ty(value_ty), "map_old_value")?;
            self.builder
                .build_store(value_slot, value)
                .map_err(|error| error.to_string())?;
            let size = self.size_align(value_ty).0;
            let inserted = self
                .invoke(
                    self.runtime_import(Family::MapRaw, "insert", key_ty, Ty::Nil)?,
                    &[
                        ptr.into(),
                        key,
                        value_slot.into(),
                        self.usize_ty().const_int(size, false).into(),
                        old_slot.into(),
                    ],
                )?
                .try_as_basic_value()
                .expect_basic("raw Map.insert returns Bool")
                .into_int_value();
            if require_existing {
                let fresh = self
                    .builder
                    .build_int_compare(
                        IntPredicate::NE,
                        inserted,
                        self.context.i8_type().const_zero(),
                        "map_assignment_missing",
                    )
                    .map_err(|error| error.to_string())?;
                let fail = self
                    .context
                    .append_basic_block(self.current_function(), "map_assignment_fail");
                let continue_block = self
                    .context
                    .append_basic_block(self.current_function(), "map_assignment_done");
                self.builder
                    .build_conditional_branch(fresh, fail, continue_block)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(fail);
                self.invoke(
                    self.runtime_import(Family::Fail, "bounds_fail", Ty::Nil, Ty::Nil)?,
                    &[],
                )?;
                self.builder
                    .build_unreachable()
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(continue_block);
            }
            if is_move_only(self.program, value_ty) {
                let replaced = self
                    .builder
                    .build_int_compare(
                        IntPredicate::EQ,
                        inserted,
                        self.context.i8_type().const_zero(),
                        "map_replaced",
                    )
                    .map_err(|error| error.to_string())?;
                let drop_block = self
                    .context
                    .append_basic_block(self.current_function(), "map_drop_replaced");
                let continue_block = self
                    .context
                    .append_basic_block(self.current_function(), "map_insert_done");
                self.builder
                    .build_conditional_branch(replaced, drop_block, continue_block)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(drop_block);
                let old = self
                    .builder
                    .build_load(self.ty(value_ty), old_slot, "map_replaced_value")
                    .map_err(|error| error.to_string())?;
                self.drop_value(value_ty, old)?;
                self.builder
                    .build_unconditional_branch(continue_block)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(continue_block);
            }
            return Ok(inserted.into());
        }
        let inserted = self
            .invoke(
                self.runtime_import(Family::Map, "insert", key_ty, value_ty)?,
                &[ptr.into(), key, value.into()],
            )?
            .try_as_basic_value()
            .expect_basic("Map.insert returns Bool")
            .into_int_value();
        if require_existing {
            let fresh = self
                .builder
                .build_int_compare(
                    IntPredicate::NE,
                    inserted,
                    self.context.i8_type().const_zero(),
                    "map_assignment_missing",
                )
                .map_err(|error| error.to_string())?;
            let fail = self
                .context
                .append_basic_block(self.current_function(), "map_assignment_fail");
            let continue_block = self
                .context
                .append_basic_block(self.current_function(), "map_assignment_done");
            self.builder
                .build_conditional_branch(fresh, fail, continue_block)
                .map_err(|error| error.to_string())?;
            self.builder.position_at_end(fail);
            self.invoke(
                self.runtime_import(Family::Fail, "bounds_fail", Ty::Nil, Ty::Nil)?,
                &[],
            )?;
            self.builder
                .build_unreachable()
                .map_err(|error| error.to_string())?;
            self.builder.position_at_end(continue_block);
        }
        Ok(inserted.into())
    }

    pub(crate) fn lower_map_delete(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        key: &TExpr,
        key_ty: Ty,
        value_ty: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let key = self.expr(env, loops, key)?;
        let key = if key_ty == Ty::ViewByte {
            self.descriptor_ptr(key, self.view_type().into(), "map_delete_key")?
                .into()
        } else {
            key.into()
        };
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("Map.delete reached a place with no storage"));
        };
        if self.map_uses_raw_value(value_ty) {
            let old_slot = self.entry_alloca(self.ty(value_ty), "map_deleted_value")?;
            let existed = self
                .invoke(
                    self.runtime_import(Family::MapRaw, "delete", key_ty, Ty::Nil)?,
                    &[
                        ptr.into(),
                        key,
                        old_slot.into(),
                        self.usize_ty()
                            .const_int(self.size_align(value_ty).0, false)
                            .into(),
                    ],
                )?
                .try_as_basic_value()
                .expect_basic("raw Map.delete returns Bool")
                .into_int_value();
            if is_move_only(self.program, value_ty) {
                let found = self
                    .builder
                    .build_int_compare(
                        IntPredicate::NE,
                        existed,
                        self.context.i8_type().const_zero(),
                        "map_delete_found",
                    )
                    .map_err(|error| error.to_string())?;
                let drop_block = self
                    .context
                    .append_basic_block(self.current_function(), "map_drop_deleted");
                let continue_block = self
                    .context
                    .append_basic_block(self.current_function(), "map_delete_done");
                self.builder
                    .build_conditional_branch(found, drop_block, continue_block)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(drop_block);
                let old = self
                    .builder
                    .build_load(self.ty(value_ty), old_slot, "map_deleted_value")
                    .map_err(|error| error.to_string())?;
                self.drop_value(value_ty, old)?;
                self.builder
                    .build_unconditional_branch(continue_block)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(continue_block);
            }
            return Ok(existed.into());
        }
        Ok(self
            .invoke(
                self.runtime_import(Family::Map, "delete", key_ty, Ty::Nil)?,
                &[ptr.into(), key],
            )?
            .try_as_basic_value()
            .expect_basic("Map.delete returns Bool"))
    }

    pub(crate) fn lower_map_index(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &TExpr,
        key: &TExpr,
        key_ty: Ty,
        value_ty: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let receiver = self.expr(env, loops, receiver)?;
        let key = self.expr(env, loops, key)?;
        let receiver =
            self.descriptor_ptr(receiver, self.collection_type().into(), "map_index_map")?;
        let key = if key_ty == Ty::ViewByte {
            self.descriptor_ptr(key, self.view_type().into(), "map_index_key")?
                .into()
        } else {
            key.into()
        };
        if self.map_uses_raw_value(value_ty) {
            let out = self.entry_alloca(self.ty(value_ty), "map_index_value")?;
            self.invoke(
                self.runtime_import(Family::MapRaw, "index", key_ty, Ty::Nil)?,
                &[
                    receiver.into(),
                    key,
                    out.into(),
                    self.usize_ty()
                        .const_int(self.size_align(value_ty).0, false)
                        .into(),
                ],
            )?;
            return self
                .builder
                .build_load(self.ty(value_ty), out, "map_index_value")
                .map_err(|error| error.to_string());
        }
        Ok(self
            .invoke(
                self.runtime_import(Family::Map, "index", key_ty, value_ty)?,
                &[receiver.into(), key],
            )?
            .try_as_basic_value()
            .expect_basic("Map indexing returns its value"))
    }

    pub(crate) fn lower_map_take(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        key: &TExpr,
        key_ty: Ty,
        value_ty: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let key = self.expr(env, loops, key)?;
        let key = if key_ty == Ty::ViewByte {
            self.descriptor_ptr(key, self.view_type().into(), "map_take_key")?
                .into()
        } else {
            key.into()
        };
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("Map.take reached a place with no storage"));
        };
        if self.map_uses_raw_value(value_ty) {
            let out = self.entry_alloca(self.ty(value_ty), "map_take_value")?;
            self.invoke(
                self.runtime_import(Family::MapRaw, "take", key_ty, Ty::Nil)?,
                &[
                    ptr.into(),
                    key,
                    out.into(),
                    self.usize_ty()
                        .const_int(self.size_align(value_ty).0, false)
                        .into(),
                ],
            )?;
            return self
                .builder
                .build_load(self.ty(value_ty), out, "map_take_value")
                .map_err(|error| error.to_string());
        }
        Ok(self
            .invoke(
                self.runtime_import(Family::Map, "take", key_ty, value_ty)?,
                &[ptr.into(), key],
            )?
            .try_as_basic_value()
            .expect_basic("Map.take returns its value"))
    }

    pub(crate) fn lower_set_contains(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &TExpr,
        value: &TExpr,
        elem: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let receiver = self.expr(env, loops, receiver)?;
        let value = self.expr(env, loops, value)?;
        let receiver =
            self.descriptor_ptr(receiver, self.collection_type().into(), "set_contains_set")?;
        let value = if elem == Ty::String || elem == Ty::ViewByte {
            self.descriptor_ptr(value, self.view_type().into(), "set_contains_value")?
                .into()
        } else {
            value.into()
        };
        Ok(self
            .invoke(
                self.runtime_import(Family::Set, "contains", elem, Ty::Nil)?,
                &[receiver.into(), value],
            )?
            .try_as_basic_value()
            .expect_basic("Set.contains returns Bool"))
    }

    pub(crate) fn lower_set_insert(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        value: &TExpr,
        elem: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let value = self.expr(env, loops, value)?;
        let value = if elem == Ty::String {
            self.descriptor_ptr(value, self.string_type().into(), "set_insert_value")?
                .into()
        } else {
            value.into()
        };
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("Set.insert reached a place with no storage"));
        };
        Ok(self
            .invoke(
                self.runtime_import(Family::Set, "insert", elem, Ty::Nil)?,
                &[ptr.into(), value],
            )?
            .try_as_basic_value()
            .expect_basic("Set.insert returns Bool"))
    }

    pub(crate) fn lower_set_delete(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        value: &TExpr,
        elem: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let value = self.expr(env, loops, value)?;
        let value = if elem == Ty::String || elem == Ty::ViewByte {
            self.descriptor_ptr(value, self.view_type().into(), "set_delete_value")?
                .into()
        } else {
            value.into()
        };
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("Set.delete reached a place with no storage"));
        };
        Ok(self
            .invoke(
                self.runtime_import(Family::Set, "delete", elem, Ty::Nil)?,
                &[ptr.into(), value],
            )?
            .try_as_basic_value()
            .expect_basic("Set.delete returns Bool"))
    }

    pub(crate) fn lower_collection_literal(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        ty: Ty,
        items: &[TExpr],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let collection_id = match ty {
            Ty::Array(id) | Ty::List(id) => id,
            _ => return Err(internal("only arrays and lists have collection literals")),
        };
        let elem = match &self.program.collections[collection_id.index()] {
            crate::types::CollectionDef::Array { elem, .. }
            | crate::types::CollectionDef::List { elem } => *elem,
            _ => return Err(internal("collection literal metadata is not a sequence")),
        };
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            values.push(self.expr(env, loops, item)?);
        }
        let ptr = if values.is_empty() {
            self.context.ptr_type(AddressSpace::default()).const_null()
        } else {
            let (element_size, element_align) = self.size_align(elem);
            let count = self.usize_ty().const_int(values.len() as u64, false);
            let size = self
                .builder
                .build_int_mul(
                    count,
                    self.usize_ty().const_int(element_size, false),
                    "collection_size",
                )
                .map_err(|error| error.to_string())?;
            let ptr = self
                .invoke(
                    self.alloc_import(),
                    &[
                        size.into(),
                        self.usize_ty().const_int(element_align, false).into(),
                    ],
                )?
                .try_as_basic_value()
                .expect_basic("collection allocation returns a pointer")
                .into_pointer_value();
            for (index, value) in values.iter().enumerate() {
                // Safety: `ptr` is the allocation returned above and
                // every index is within the exact number of elements.
                let slot = unsafe {
                    self.builder.build_gep(
                        self.ty(elem),
                        ptr,
                        &[self.context.i64_type().const_int(index as u64, false)],
                        "collection_element",
                    )
                }
                .map_err(|error| error.to_string())?;
                self.builder
                    .build_store(slot, *value)
                    .map_err(|error| error.to_string())?;
            }
            ptr
        };
        let len = self
            .context
            .i64_type()
            .const_int(values.len() as u64, false);
        let mut descriptor = self.ty(ty).into_struct_type().const_zero();
        descriptor = self
            .builder
            .build_insert_value(descriptor, ptr, 0, "collection_ptr")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        descriptor = self
            .builder
            .build_insert_value(descriptor, len, 1, "collection_len")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        descriptor = self
            .builder
            .build_insert_value(descriptor, len, 2, "collection_cap")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        Ok(descriptor.into())
    }

    pub(crate) fn lower_collection_new(&self, ty: Ty) -> Result<BasicValueEnum<'ctx>, String> {
        let mut descriptor = self.ty(ty).into_struct_type().const_zero();
        descriptor = self
            .builder
            .build_insert_value(
                descriptor,
                self.context.ptr_type(AddressSpace::default()).const_null(),
                0,
                "collection_ptr",
            )
            .map_err(|error| error.to_string())?
            .into_struct_value();
        Ok(descriptor.into())
    }

    pub(crate) fn lower_collection_length(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        value: &TExpr,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let value = self.expr(env, loops, value)?;
        self.builder
            .build_extract_value(as_struct(value)?, 1, "collection_len")
            .map_err(|error| error.to_string())
    }

    pub(crate) fn lower_collection_is_empty(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        value: &TExpr,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let value = self.expr(env, loops, value)?;
        let length = self
            .builder
            .build_extract_value(as_struct(value)?, 1, "collection_len")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let empty = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                length,
                self.context.i64_type().const_zero(),
                "collection_empty",
            )
            .map_err(|error| error.to_string())?;
        Ok(self
            .builder
            .build_int_z_extend(empty, self.context.i8_type(), "collection_empty_byte")
            .map_err(|error| error.to_string())?
            .into())
    }

    pub(crate) fn lower_collection_capacity(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        value: &TExpr,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let value = self.expr(env, loops, value)?;
        self.builder
            .build_extract_value(as_struct(value)?, 2, "collection_cap")
            .map_err(|error| error.to_string())
    }

    pub(crate) fn lower_collection_view(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        value: &TExpr,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        self.expr(env, loops, value)
    }

    pub(crate) fn lower_collection_slice(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        value: &TExpr,
        start: &TExpr,
        end: &TExpr,
        view_ty: Ty,
        sum: SumId,
        elem: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let value = self.expr(env, loops, value)?.into_struct_value();
        let start = self.expr(env, loops, start)?.into_int_value();
        let end = self.expr(env, loops, end)?.into_int_value();
        let ptr = self
            .builder
            .build_extract_value(value, 0, "view_ptr")
            .map_err(|error| error.to_string())?
            .into_pointer_value();
        let length = self
            .builder
            .build_extract_value(value, 1, "view_len")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let zero = self.context.i64_type().const_zero();
        let start_nonnegative = self
            .builder
            .build_int_compare(IntPredicate::SGE, start, zero, "slice_start_nonnegative")
            .map_err(|error| error.to_string())?;
        let end_nonnegative = self
            .builder
            .build_int_compare(IntPredicate::SGE, end, zero, "slice_end_nonnegative")
            .map_err(|error| error.to_string())?;
        let ordered = self
            .builder
            .build_int_compare(IntPredicate::ULE, start, end, "slice_ordered")
            .map_err(|error| error.to_string())?;
        let end_in_range = self
            .builder
            .build_int_compare(IntPredicate::ULE, end, length, "slice_end_in_range")
            .map_err(|error| error.to_string())?;
        let valid = self
            .builder
            .build_and(start_nonnegative, end_nonnegative, "slice_nonnegative")
            .map_err(|error| error.to_string())?;
        let valid = self
            .builder
            .build_and(valid, ordered, "slice_ordered_valid")
            .map_err(|error| error.to_string())?;
        let valid = self
            .builder
            .build_and(valid, end_in_range, "slice_range_valid")
            .map_err(|error| error.to_string())?;
        let function = self.current_function();
        let valid_block = self.context.append_basic_block(function, "slice_valid");
        let invalid_block = self.context.append_basic_block(function, "slice_invalid");
        let join_block = self.context.append_basic_block(function, "slice_join");
        self.builder
            .build_conditional_branch(valid, valid_block, invalid_block)
            .map_err(|error| error.to_string())?;

        self.builder.position_at_end(invalid_block);
        let nil_tag = self.sum_member_tag(sum, Ty::Nil)?;
        let zeroed = self.struct_ty(Ty::Sum(sum))?.const_zero();
        let nil = self
            .builder
            .build_insert_value(
                zeroed,
                self.context.i32_type().const_int(u64::from(nil_tag), false),
                0,
                "slice_nil_tag",
            )
            .map_err(|error| error.to_string())?
            .into_struct_value();
        self.builder
            .build_unconditional_branch(join_block)
            .map_err(|error| error.to_string())?;
        let invalid_end = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("slice invalid block has no insertion block"))?;

        self.builder.position_at_end(valid_block);
        // Safety: the checker establishes the half-open range and the
        // source descriptor owns aligned storage for `elem` values.
        let sliced_ptr = unsafe {
            self.builder
                .build_gep(self.ty(elem), ptr, &[start], "slice_ptr")
        }
        .map_err(|error| error.to_string())?;
        let sliced_length = self
            .builder
            .build_int_sub(end, start, "slice_len")
            .map_err(|error| error.to_string())?;
        let view_zero = self.ty(view_ty).into_struct_type().const_zero();
        let view = self
            .builder
            .build_insert_value(view_zero, sliced_ptr, 0, "slice_view_ptr")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        let view = self
            .builder
            .build_insert_value(view, sliced_length, 1, "slice_view_len")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        let view_tag = self.sum_member_tag(sum, view_ty)?;
        let tagged = self
            .builder
            .build_insert_value(
                zeroed,
                self.context
                    .i32_type()
                    .const_int(u64::from(view_tag), false),
                0,
                "slice_view_tag",
            )
            .map_err(|error| error.to_string())?
            .into_struct_value();
        let tagged = self
            .builder
            .build_insert_value(tagged, view, view_tag + 1, "slice_view_payload")
            .map_err(|error| error.to_string())?
            .into_struct_value();
        self.builder
            .build_unconditional_branch(join_block)
            .map_err(|error| error.to_string())?;
        let valid_end = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("slice valid block has no insertion block"))?;

        self.builder.position_at_end(join_block);
        let phi = self
            .builder
            .build_phi(self.struct_ty(Ty::Sum(sum))?, "slice_result")
            .map_err(|error| error.to_string())?;
        phi.add_incoming(&[(&nil, invalid_end), (&tagged, valid_end)]);
        Ok(phi.as_basic_value())
    }

    pub(crate) fn lower_collection_index(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        collection: &TExpr,
        index: &TExpr,
        elem: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let collection = self.expr(env, loops, collection)?.into_struct_value();
        let index = self.expr(env, loops, index)?.into_int_value();
        let ptr = self
            .builder
            .build_extract_value(collection, 0, "collection_ptr")
            .map_err(|error| error.to_string())?
            .into_pointer_value();
        let len = self
            .builder
            .build_extract_value(collection, 1, "collection_len")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let nonnegative = self
            .builder
            .build_int_compare(
                IntPredicate::SGE,
                index,
                self.context.i64_type().const_zero(),
                "index_nonnegative",
            )
            .map_err(|error| error.to_string())?;
        let in_range = self
            .builder
            .build_int_compare(IntPredicate::ULT, index, len, "index_in_range")
            .map_err(|error| error.to_string())?;
        let valid = self
            .builder
            .build_and(nonnegative, in_range, "index_valid")
            .map_err(|error| error.to_string())?;
        let function = self.current_function();
        let valid_block = self.context.append_basic_block(function, "index_valid");
        let invalid_block = self.context.append_basic_block(function, "index_invalid");
        self.builder
            .build_conditional_branch(valid, valid_block, invalid_block)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(invalid_block);
        self.invoke(
            self.runtime_import(Family::Fail, "bounds_fail", Ty::Nil, Ty::Nil)?,
            &[],
        )?;
        self.builder
            .build_unreachable()
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(valid_block);
        // Safety: the checker enforces the range predicate above and
        // the descriptor points at storage for this element type.
        let element_ptr = unsafe {
            self.builder
                .build_gep(self.ty(elem), ptr, &[index], "indexed_element")
        }
        .map_err(|error| error.to_string())?;
        self.builder
            .build_load(self.ty(elem), element_ptr, "indexed_value")
            .map_err(|error| error.to_string())
    }

    pub(crate) fn lower_list_pop(
        &self,
        env: &Env<'ctx>,
        receiver: &Place,
        elem: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("List.pop reached a place with no storage"));
        };
        if is_scalar_collection_element(elem) {
            Ok(self
                .invoke(
                    self.runtime_import(Family::List, "pop", elem, Ty::Nil)?,
                    &[ptr.into()],
                )?
                .try_as_basic_value()
                .expect_basic("List.pop returns an element"))
        } else {
            let slot = self.entry_alloca(self.ty(elem), "list_pop_value")?;
            let (size, _) = self.size_align(elem);
            self.invoke(
                self.runtime_import(Family::ListRaw, "pop", Ty::Nil, Ty::Nil)?,
                &[
                    ptr.into(),
                    slot.into(),
                    self.usize_ty().const_int(size, false).into(),
                ],
            )?;
            self.builder
                .build_load(self.ty(elem), slot, "list_pop_value")
                .map_err(|error| error.to_string())
        }
    }

    pub(crate) fn lower_list_remove(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        receiver: &Place,
        index: &TExpr,
        elem: Ty,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let index = self.expr(env, loops, index)?;
        let Some((ptr, _)) = self.place_ptr(env, receiver)? else {
            return Err(internal("List.remove reached a place with no storage"));
        };
        if is_scalar_collection_element(elem) {
            Ok(self
                .invoke(
                    self.runtime_import(Family::List, "remove", elem, Ty::Nil)?,
                    &[ptr.into(), index.into()],
                )?
                .try_as_basic_value()
                .expect_basic("List.remove returns an element"))
        } else {
            let slot = self.entry_alloca(self.ty(elem), "list_remove_value")?;
            let (size, _) = self.size_align(elem);
            self.invoke(
                self.runtime_import(Family::ListRaw, "remove", Ty::Nil, Ty::Nil)?,
                &[
                    ptr.into(),
                    index.into(),
                    slot.into(),
                    self.usize_ty().const_int(size, false).into(),
                ],
            )?;
            self.builder
                .build_load(self.ty(elem), slot, "list_remove_value")
                .map_err(|error| error.to_string())
        }
    }
}
