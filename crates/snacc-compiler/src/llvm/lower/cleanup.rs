//! Drop and cleanup lowering (Specification 029 Phase 3).

use super::*;

impl<'ctx> Codegen<'ctx, '_> {
    pub(crate) fn drop_sequence_elements(
        &self,
        ptr: PointerValue<'ctx>,
        len: IntValue<'ctx>,
        elem: Ty,
    ) -> Result<(), String> {
        let function = self.current_function();
        let loop_block = self
            .context
            .append_basic_block(function, "list_clear_drop_loop");
        let done = self
            .context
            .append_basic_block(function, "list_clear_drop_done");
        let entry = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("list clear has no insertion block"))?;
        self.builder
            .build_unconditional_branch(loop_block)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(loop_block);
        let index = self
            .builder
            .build_phi(self.context.i64_type(), "list_clear_drop_index")
            .map_err(|error| error.to_string())?;
        let zero = self.context.i64_type().const_zero();
        index.add_incoming(&[(&zero, entry)]);
        let current = index.as_basic_value().into_int_value();
        let more = self
            .builder
            .build_int_compare(IntPredicate::ULT, current, len, "list_clear_drop_more")
            .map_err(|error| error.to_string())?;
        let body = self
            .context
            .append_basic_block(function, "list_clear_drop_item");
        self.builder
            .build_conditional_branch(more, body, done)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(body);
        // Safety: `current < len` and the descriptor points at initialized
        // storage for one value of the checked element type.
        let item_ptr = unsafe {
            self.builder
                .build_gep(self.ty(elem), ptr, &[current], "list_clear_drop_ptr")
        }
        .map_err(|error| error.to_string())?;
        let item = self
            .builder
            .build_load(self.ty(elem), item_ptr, "list_clear_drop_value")
            .map_err(|error| error.to_string())?;
        self.drop_value(elem, item)?;
        let next = self
            .builder
            .build_int_add(
                current,
                self.context.i64_type().const_int(1, false),
                "list_clear_drop_next",
            )
            .map_err(|error| error.to_string())?;
        let body_end = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("list clear drop body has no block"))?;
        self.builder
            .build_unconditional_branch(loop_block)
            .map_err(|error| error.to_string())?;
        index.add_incoming(&[(&next, body_end)]);
        self.builder.position_at_end(done);
        Ok(())
    }

    /// Drops every opaque value still held by a raw map. The runtime stores
    /// only bytes, so typed destruction must happen before the runtime erases
    /// the entries or releases the map allocation.
    pub(crate) fn drop_map_values(
        &self,
        key_ty: Ty,
        value_ty: Ty,
        descriptor: StructValue<'ctx>,
    ) -> Result<(), String> {
        if !is_move_only(self.program, value_ty) {
            return Ok(());
        }
        let len = self
            .builder
            .build_extract_value(descriptor, 1, "map_drop_len")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let function = self.current_function();
        let loop_block = self.context.append_basic_block(function, "map_drop_loop");
        let done = self.context.append_basic_block(function, "map_drop_done");
        let body = self.context.append_basic_block(function, "map_drop_item");
        let entry = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("map drop has no insertion block"))?;
        self.builder
            .build_unconditional_branch(loop_block)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(loop_block);
        let index = self
            .builder
            .build_phi(self.context.i64_type(), "map_drop_index")
            .map_err(|error| error.to_string())?;
        let zero = self.context.i64_type().const_zero();
        index.add_incoming(&[(&zero, entry)]);
        let current = index.as_basic_value().into_int_value();
        let more = self
            .builder
            .build_int_compare(IntPredicate::ULT, current, len, "map_drop_more")
            .map_err(|error| error.to_string())?;
        self.builder
            .build_conditional_branch(more, body, done)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(body);
        let out = self.entry_alloca(self.ty(value_ty), "map_drop_value")?;
        let map_slot = self.entry_alloca(self.collection_type().into(), "map_drop_map")?;
        self.builder
            .build_store(map_slot, descriptor)
            .map_err(|error| error.to_string())?;
        self.invoke(
            self.runtime_import(Family::MapRaw, "value_at", key_ty, Ty::Nil)?,
            &[
                map_slot.into(),
                current.into(),
                out.into(),
                self.usize_ty()
                    .const_int(self.size_align(value_ty).0, false)
                    .into(),
            ],
        )?;
        let item = self
            .builder
            .build_load(self.ty(value_ty), out, "map_drop_item_value")
            .map_err(|error| error.to_string())?;
        self.drop_value(value_ty, item)?;
        let next = self
            .builder
            .build_int_add(
                current,
                self.context.i64_type().const_int(1, false),
                "map_drop_next",
            )
            .map_err(|error| error.to_string())?;
        let body_end = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("map drop body has no block"))?;
        self.builder
            .build_unconditional_branch(loop_block)
            .map_err(|error| error.to_string())?;
        index.add_incoming(&[(&next, body_end)]);
        self.builder.position_at_end(done);
        Ok(())
    }

    /// Recursively destroys one owned value of `ty` (Specification 016
    /// section 8.1). Only ever called where the checked cleanup plan already
    /// decided `ty` is move-only -- a copyable value is never dropped -- so
    /// every arm recurses only into fields/members/pointees that are
    /// themselves move-only, skipping a copyable one entirely rather than
    /// re-deriving that fact from scratch at each level.
    pub(crate) fn drop_value(&self, ty: Ty, value: BasicValueEnum<'ctx>) -> Result<(), String> {
        match ty {
            Ty::String => {
                let slot = self.entry_alloca(self.string_type().into(), "string_drop_value")?;
                self.builder
                    .build_store(slot, value)
                    .map_err(|error| error.to_string())?;
                self.invoke(
                    self.runtime_import(Family::String, "drop", Ty::Nil, Ty::Nil)?,
                    &[slot.into()],
                )?;
                Ok(())
            }
            Ty::Box(id) => {
                let pointee = self.box_pointee(id);
                let ptr = value.into_pointer_value();
                if is_move_only(self.program, pointee) {
                    let pointee_ty = self.ty(pointee);
                    let loaded = self
                        .builder
                        .build_load(pointee_ty, ptr, "boxed")
                        .map_err(|error| error.to_string())?;
                    self.drop_value(pointee, loaded)?;
                }
                self.call_dealloc(ptr, pointee)
            }
            Ty::User(id) => match self.def(id) {
                TypeDef::Represented { target, .. } => self.drop_value(*target, value),
                TypeDef::Struct { fields, .. } | TypeDef::UnionMember { fields, .. } => {
                    // Specification 016 section 8.1: a struct drops its
                    // move-only fields in source declaration order.
                    for (index, (_, field_ty)) in fields.iter().enumerate() {
                        if is_move_only(self.program, *field_ty) {
                            let field = self
                                .builder
                                .build_extract_value(as_struct(value)?, index as u32, "field")
                                .map_err(|error| error.to_string())?;
                            self.drop_value(*field_ty, field)?;
                        }
                    }
                    Ok(())
                }
                TypeDef::Union { members, .. } => self.drop_union(members, value),
            },
            Ty::Sum(id) => {
                let members = self.program.sums[id.index()].clone();
                self.drop_sum(&members, value)
            }
            Ty::Map(id) => {
                let descriptor = as_struct(value)?;
                let (key_ty, value_ty) = match &self.program.collections[id.index()] {
                    crate::types::CollectionDef::Map { key, value } => (*key, *value),
                    _ => return Err(internal("map drop has non-map metadata")),
                };
                if self.map_uses_raw_value(value_ty) {
                    self.drop_map_values(key_ty, value_ty, descriptor)?;
                    let map_slot =
                        self.entry_alloca(self.collection_type().into(), "map_drop_map")?;
                    self.builder
                        .build_store(map_slot, descriptor)
                        .map_err(|error| error.to_string())?;
                    self.invoke(
                        self.runtime_import(Family::MapRaw, "drop", key_ty, Ty::Nil)?,
                        &[map_slot.into()],
                    )?;
                } else {
                    let slot = self.entry_alloca(self.collection_type().into(), "map_drop_map")?;
                    self.builder
                        .build_store(slot, descriptor)
                        .map_err(|error| error.to_string())?;
                    self.invoke(
                        self.runtime_import(Family::Map, "drop", key_ty, value_ty)?,
                        &[slot.into()],
                    )?;
                }
                Ok(())
            }
            Ty::Set(id) => {
                let descriptor = as_struct(value)?;
                let elem = match &self.program.collections[id.index()] {
                    crate::types::CollectionDef::Set { elem } => *elem,
                    _ => return Err(internal("set drop has non-set metadata")),
                };
                let slot = self.entry_alloca(self.collection_type().into(), "set_drop_set")?;
                self.builder
                    .build_store(slot, descriptor)
                    .map_err(|error| error.to_string())?;
                self.invoke(
                    self.runtime_import(Family::Set, "drop", elem, Ty::Nil)?,
                    &[slot.into()],
                )?;
                Ok(())
            }
            Ty::Array(id) | Ty::List(id) => {
                let descriptor = as_struct(value)?;
                let ptr = self
                    .builder
                    .build_extract_value(descriptor, 0, "collection_ptr")
                    .map_err(|error| error.to_string())?
                    .into_pointer_value();
                let len = self
                    .builder
                    .build_extract_value(descriptor, 1, "collection_len")
                    .map_err(|error| error.to_string())?
                    .into_int_value();
                let cap = self
                    .builder
                    .build_extract_value(descriptor, 2, "collection_cap")
                    .map_err(|error| error.to_string())?
                    .into_int_value();
                let elem = match &self.program.collections[id.index()] {
                    crate::types::CollectionDef::Array { elem, .. }
                    | crate::types::CollectionDef::List { elem } => *elem,
                    _ => return Err(internal("sequence drop has non-sequence metadata")),
                };
                let function = self.current_function();
                let loop_block = self
                    .context
                    .append_basic_block(function, "collection_drop_loop");
                let done = self
                    .context
                    .append_basic_block(function, "collection_drop_done");
                let entry_block = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| internal("collection drop has no insertion block"))?;
                self.builder
                    .build_unconditional_branch(loop_block)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(loop_block);
                let index = self
                    .builder
                    .build_phi(self.context.i64_type(), "collection_drop_index")
                    .map_err(|error| error.to_string())?;
                let zero = self.context.i64_type().const_zero();
                index.add_incoming(&[(&zero, entry_block)]);
                let current = index.as_basic_value().into_int_value();
                let more = self
                    .builder
                    .build_int_compare(IntPredicate::ULT, current, len, "collection_drop_more")
                    .map_err(|error| error.to_string())?;
                let body = self
                    .context
                    .append_basic_block(function, "collection_drop_item");
                self.builder
                    .build_conditional_branch(more, body, done)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(body);
                if is_move_only(self.program, elem) {
                    let item_ptr = unsafe {
                        self.builder.build_gep(
                            self.ty(elem),
                            ptr,
                            &[current],
                            "collection_drop_item_ptr",
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    let item = self
                        .builder
                        .build_load(self.ty(elem), item_ptr, "collection_drop_item")
                        .map_err(|error| error.to_string())?;
                    self.drop_value(elem, item)?;
                }
                let next = self
                    .builder
                    .build_int_add(
                        current,
                        self.context.i64_type().const_int(1, false),
                        "collection_drop_next",
                    )
                    .map_err(|error| error.to_string())?;
                self.builder
                    .build_unconditional_branch(loop_block)
                    .map_err(|error| error.to_string())?;
                index.add_incoming(&[(&next, self.builder.get_insert_block().unwrap())]);
                self.builder.position_at_end(done);
                let elem_size = self.size_align(elem).0;
                let size = self
                    .builder
                    .build_int_mul(
                        cap,
                        self.usize_ty().const_int(elem_size, false),
                        "collection_drop_size",
                    )
                    .map_err(|error| error.to_string())?;
                let align = self.usize_ty().const_int(self.size_align(elem).1, false);
                let nonnull = self
                    .builder
                    .build_is_not_null(ptr, "collection_drop_allocated")
                    .map_err(|error| error.to_string())?;
                let free = self
                    .context
                    .append_basic_block(function, "collection_drop_free");
                let after_free = self
                    .context
                    .append_basic_block(function, "collection_drop_after_free");
                self.builder
                    .build_conditional_branch(nonnull, free, after_free)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(free);
                self.call_raw_dealloc(ptr, size, align)?;
                self.builder
                    .build_unconditional_branch(after_free)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(after_free);
                Ok(())
            }
            // A caller only ever reaches this function for a move-only type
            // (Specification 016 section 5.3 composes structurally, so a
            // scalar is never move-only itself); kept as a safe no-op rather
            // than an internal-error panic since a struct/union/sum's field
            // loop above deliberately does not pre-filter its recursive call
            // by scalar-ness before checking `is_move_only`.
            _ => Ok(()),
        }
    }

    /// A union's drop (Specification 016 section 8.1: "a union drops only its
    /// active payload"): the active member is not statically known here, so
    /// this dispatches on the union's own runtime tag, mirroring
    /// `Codegen::equal_union`'s tag-then-member-block shape exactly, except
    /// each block runs a drop (a side effect) rather than producing a phi
    /// value.
    pub(crate) fn drop_union(
        &self,
        members: &[TypeId],
        value: BasicValueEnum<'ctx>,
    ) -> Result<(), String> {
        let tag = self
            .builder
            .build_extract_value(as_struct(value)?, 0, "tag")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let function = self.current_function();
        let done = self.context.append_basic_block(function, "drop_done");
        let unknown = self.context.append_basic_block(function, "drop_unknown");
        let mut cases = Vec::with_capacity(members.len());
        for member in members {
            let tag_value = self.member_tag(*member)?;
            let block = self.context.append_basic_block(function, "drop_member");
            cases.push((
                self.context
                    .i32_type()
                    .const_int(u64::from(tag_value), false),
                block,
            ));
        }
        self.builder
            .build_switch(tag, unknown, &cases)
            .map_err(|error| error.to_string())?;
        // A stored tag outside the union's members means construction wrote one
        // that does not exist.
        self.builder.position_at_end(unknown);
        self.exhausted()?;

        for (member, (_, block)) in members.iter().zip(&cases) {
            self.builder.position_at_end(*block);
            let member_ty = Ty::User(*member);
            if is_move_only(self.program, member_ty) {
                let field_index = self.member_tag(*member)? + 1;
                let payload = self
                    .builder
                    .build_extract_value(as_struct(value)?, field_index, "member")
                    .map_err(|error| error.to_string())?;
                self.drop_value(member_ty, payload)?;
            }
            self.builder
                .build_unconditional_branch(done)
                .map_err(|error| error.to_string())?;
        }
        self.builder.position_at_end(done);
        Ok(())
    }

    /// An inline sum's drop: identical strategy to [`Self::drop_union`],
    /// except a member's deterministic tag is its position in `members`
    /// rather than a `TypeId`'s own tag (Specification 018 section 8's
    /// tag-plus-fields shape, reused unchanged).
    pub(crate) fn drop_sum(
        &self,
        members: &[Ty],
        value: BasicValueEnum<'ctx>,
    ) -> Result<(), String> {
        let tag = self
            .builder
            .build_extract_value(as_struct(value)?, 0, "tag")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let function = self.current_function();
        let done = self.context.append_basic_block(function, "drop_done");
        let unknown = self.context.append_basic_block(function, "drop_unknown");
        let cases: Vec<_> = (0..members.len())
            .map(|tag| {
                (
                    self.context.i32_type().const_int(tag as u64, false),
                    self.context.append_basic_block(function, "drop_member"),
                )
            })
            .collect();
        self.builder
            .build_switch(tag, unknown, &cases)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(unknown);
        self.exhausted()?;

        for (index, (member, (_, block))) in members.iter().zip(cases).enumerate() {
            self.builder.position_at_end(block);
            if is_move_only(self.program, *member) {
                let payload = self
                    .builder
                    .build_extract_value(as_struct(value)?, index as u32 + 1, "member")
                    .map_err(|error| error.to_string())?;
                self.drop_value(*member, payload)?;
            }
            self.builder
                .build_unconditional_branch(done)
                .map_err(|error| error.to_string())?;
        }
        self.builder.position_at_end(done);
        Ok(())
    }

    /// Runs the checked cleanup plan's drops (Specification 016 section 8.1),
    /// in the order the checker already put them in (reverse declaration
    /// order): loads each place's current value, then destroys it.
    pub(crate) fn drop_places(&self, env: &Env<'ctx>, drops: &[Place]) -> Result<(), String> {
        for place in drops {
            let value = self.place_value(env, place)?;
            self.drop_value(place.ty, value)?;
        }
        Ok(())
    }
}
