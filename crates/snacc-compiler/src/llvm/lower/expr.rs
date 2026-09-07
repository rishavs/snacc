//! Expression lowering (Specification 029 Phase 3).
//!
//! Collection-operation arms dispatch to [`super::collections`] (each arm
//! group owned by a method there); every other arm lowers inline.

use super::*;

impl<'ctx> Codegen<'ctx, '_> {
    pub(crate) fn concat_scalar_bits(
        &self,
        value: BasicValueEnum<'ctx>,
        ty: Ty,
    ) -> Result<IntValue<'ctx>, String> {
        match ty {
            Ty::Float32 => {
                let bits = self
                    .builder
                    .build_bit_cast(value, self.context.i32_type(), "concat_f32_bits")
                    .map_err(|error| error.to_string())?
                    .into_int_value();
                self.unsigned_to_i64(bits, "concat_f32_bits64")
            }
            Ty::Float64 => Ok(self
                .builder
                .build_bit_cast(value, self.context.i64_type(), "concat_f64_bits")
                .map_err(|error| error.to_string())?
                .into_int_value()),
            Ty::Int64 | Ty::UInt64 => Ok(value.into_int_value()),
            Ty::Byte | Ty::UInt16 | Ty::UInt32 | Ty::Bool | Ty::Unicode => {
                self.unsigned_to_i64(value.into_int_value(), "concat_scalar_bits")
            }
            _ => Err(internal("unsupported scalar concatenation value")),
        }
    }

    pub(crate) fn expr(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        expr: &TExpr,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        match expr {
            // Each literal arrived at its exact value in the lexer, so nothing
            // here re-parses or re-rounds it. `f32 as f64` is exact, so the
            // already-rounded binary32 value reaches `const_float` unchanged.
            TExpr::Num(literal) => Ok(match literal {
                NumLiteral::F64(value) => self.context.f64_type().const_float(*value).into(),
                NumLiteral::F32(value) => self
                    .context
                    .f32_type()
                    .const_float(f64::from(*value))
                    .into(),
                NumLiteral::Int(value) => self
                    .context
                    .i64_type()
                    .const_int(*value as u64, true)
                    .into(),
                NumLiteral::U8(value) => self
                    .context
                    .i8_type()
                    .const_int(u64::from(*value), false)
                    .into(),
                NumLiteral::U16(value) => self
                    .context
                    .i16_type()
                    .const_int(u64::from(*value), false)
                    .into(),
                NumLiteral::U32(value) => self
                    .context
                    .i32_type()
                    .const_int(u64::from(*value), false)
                    .into(),
                NumLiteral::U64(value) => self.context.i64_type().const_int(*value, false).into(),
            }),
            TExpr::Bool(value) => Ok(self
                .context
                .i8_type()
                .const_int(*value as u64, false)
                .into()),
            TExpr::Nil => Ok(self.context.i8_type().const_zero().into()),
            TExpr::Unicode(value) => Ok(self
                .context
                .i32_type()
                .const_int(u64::from(*value), false)
                .into()),
            TExpr::StringLiteral(value) => {
                let global = self
                    .builder
                    .build_global_string_ptr(value, "string_literal")
                    .map_err(|error| error.to_string())?;
                let out = self.entry_alloca(self.string_type().into(), "string_literal_value")?;
                self.invoke(
                    self.runtime_import(Family::String, "new", Ty::Nil, Ty::Nil)?,
                    &[
                        out.into(),
                        global.as_pointer_value().into(),
                        self.usize_ty().const_int(value.len() as u64, false).into(),
                    ],
                )?;
                Ok(self
                    .builder
                    .build_load(self.string_type(), out, "string_literal_value")
                    .map_err(|error| error.to_string())?)
            }
            TExpr::StringClone(value) => {
                let value = self.expr(env, loops, value)?;
                let value =
                    self.descriptor_ptr(value, self.string_type().into(), "string_clone")?;
                self.string_out_call(
                    self.runtime_import(Family::String, "clone", Ty::Nil, Ty::Nil)?,
                    vec![value.into()],
                    "string_clone_value",
                )
            }
            TExpr::StringConcat(parts) => {
                if parts.is_empty() {
                    let empty = self
                        .builder
                        .build_global_string_ptr("", "empty_interpolation")
                        .map_err(|error| error.to_string())?;
                    return self.string_out_call(
                        self.runtime_import(Family::String, "new", Ty::Nil, Ty::Nil)?,
                        vec![
                            empty.as_pointer_value().into(),
                            self.usize_ty().const_zero().into(),
                        ],
                        "empty_interpolation_value",
                    );
                }
                let i64_type = self.context.i64_type();
                let erased_type = self
                    .context
                    .struct_type(&[i64_type.into(), i64_type.into(), i64_type.into()], false);
                let array_type = erased_type.array_type(parts.len() as u32);
                let storage = self.entry_alloca(array_type.into(), "string_concat_parts")?;
                let zero = self.context.i32_type().const_zero();
                let mut owned_temporaries = Vec::new();

                for (index, part) in parts.iter().enumerate() {
                    let (tag, first, second) = if let TExpr::StringLiteral(text) = &part.value {
                        let global = self
                            .builder
                            .build_global_string_ptr(text, "string_concat_literal")
                            .map_err(|error| error.to_string())?;
                        let pointer = self
                            .builder
                            .build_ptr_to_int(
                                global.as_pointer_value(),
                                i64_type,
                                "string_concat_literal_ptr",
                            )
                            .map_err(|error| error.to_string())?;
                        (0, pointer, i64_type.const_int(text.len() as u64, false))
                    } else {
                        let value = self.expr(env, loops, &part.value)?;
                        match part.ty {
                            Ty::String => {
                                if !matches!(&part.value, TExpr::Place(_, _)) {
                                    owned_temporaries.push(value);
                                }
                                let descriptor = value.into_struct_value();
                                let pointer = self
                                    .builder
                                    .build_extract_value(descriptor, 0, "string_concat_text_ptr")
                                    .map_err(|error| error.to_string())?
                                    .into_pointer_value();
                                let length = self
                                    .builder
                                    .build_extract_value(descriptor, 1, "string_concat_text_len")
                                    .map_err(|error| error.to_string())?
                                    .into_int_value();
                                let pointer = self
                                    .builder
                                    .build_ptr_to_int(pointer, i64_type, "string_concat_text_bits")
                                    .map_err(|error| error.to_string())?;
                                let length =
                                    self.unsigned_to_i64(length, "string_concat_text_len64")?;
                                (0, pointer, length)
                            }
                            Ty::ViewUnicode => {
                                let descriptor = value.into_struct_value();
                                let pointer = self
                                    .builder
                                    .build_extract_value(descriptor, 0, "string_concat_view_ptr")
                                    .map_err(|error| error.to_string())?
                                    .into_pointer_value();
                                let length = self
                                    .builder
                                    .build_extract_value(descriptor, 1, "string_concat_view_len")
                                    .map_err(|error| error.to_string())?
                                    .into_int_value();
                                let pointer = self
                                    .builder
                                    .build_ptr_to_int(pointer, i64_type, "string_concat_view_bits")
                                    .map_err(|error| error.to_string())?;
                                let length =
                                    self.unsigned_to_i64(length, "string_concat_view_len64")?;
                                (0, pointer, length)
                            }
                            ty => (
                                concat_part_tag(ty)?,
                                self.concat_scalar_bits(value, ty)?,
                                i64_type.const_zero(),
                            ),
                        }
                    };
                    let mut erased = erased_type.const_zero();
                    erased = self
                        .builder
                        .build_insert_value(
                            erased,
                            i64_type.const_int(tag, false),
                            0,
                            "string_concat_tag",
                        )
                        .map_err(|error| error.to_string())?
                        .into_struct_value();
                    erased = self
                        .builder
                        .build_insert_value(erased, first, 1, "string_concat_first")
                        .map_err(|error| error.to_string())?
                        .into_struct_value();
                    erased = self
                        .builder
                        .build_insert_value(erased, second, 2, "string_concat_second")
                        .map_err(|error| error.to_string())?
                        .into_struct_value();
                    let slot = unsafe {
                        self.builder.build_in_bounds_gep(
                            array_type,
                            storage,
                            &[zero, self.context.i32_type().const_int(index as u64, false)],
                            "string_concat_part",
                        )
                    }
                    .map_err(|error| error.to_string())?;
                    self.builder
                        .build_store(slot, erased)
                        .map_err(|error| error.to_string())?;
                }

                let result = self.string_out_call(
                    self.runtime_import(Family::String, "concat_parts", Ty::Nil, Ty::Nil)?,
                    vec![
                        storage.into(),
                        self.usize_ty().const_int(parts.len() as u64, false).into(),
                    ],
                    "string_concat_value",
                )?;
                for temporary in owned_temporaries {
                    self.drop_value(Ty::String, temporary)?;
                }
                Ok(result)
            }
            TExpr::StringFromUnicode(value) => {
                let value = self.expr(env, loops, value)?;
                let value =
                    self.descriptor_ptr(value, self.view_type().into(), "string_from_view")?;
                self.string_out_call(
                    self.runtime_import(Family::String, "from_view", Ty::Nil, Ty::Nil)?,
                    vec![value.into()],
                    "string_from_view_value",
                )
            }
            TExpr::StringFromUtf8(value, sum) => {
                let value = self.expr(env, loops, value)?;
                let value =
                    self.descriptor_ptr(value, self.view_type().into(), "string_from_utf8")?;
                let string = self
                    .string_out_call(
                        self.runtime_import(Family::String, "from_utf8", Ty::Nil, Ty::Nil)?,
                        vec![value.into()],
                        "string_from_utf8_value",
                    )?
                    .into_struct_value();
                let valid = self
                    .builder
                    .build_is_not_null(
                        self.builder
                            .build_extract_value(string, 0, "utf8_ptr")
                            .map_err(|error| error.to_string())?
                            .into_pointer_value(),
                        "utf8_valid",
                    )
                    .map_err(|error| error.to_string())?;
                let nil_tag = self.sum_member_tag(*sum, Ty::Nil)?;
                let string_tag = self.sum_member_tag(*sum, Ty::String)?;
                let zeroed = self.struct_ty(Ty::Sum(*sum))?.const_zero();
                let nil = self
                    .builder
                    .build_insert_value(
                        zeroed,
                        self.context.i32_type().const_int(u64::from(nil_tag), false),
                        0,
                        "utf8_nil",
                    )
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                let tagged = self
                    .builder
                    .build_insert_value(
                        zeroed,
                        self.context
                            .i32_type()
                            .const_int(u64::from(string_tag), false),
                        0,
                        "utf8_string",
                    )
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                let valid_value = self
                    .builder
                    .build_insert_value(tagged, string, string_tag + 1, "utf8_payload")
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                Ok(self
                    .builder
                    .build_select(valid, valid_value, nil, "utf8_result")
                    .map_err(|error| error.to_string())?)
            }
            TExpr::ViewFromString(value, ty) => {
                let value = self.expr(env, loops, value)?;
                let value =
                    self.descriptor_ptr(value, self.string_type().into(), "string_view_input")?;
                Ok(self.view_out_call(
                    self.runtime_import(
                        Family::String,
                        if *ty == Ty::ViewUnicode {
                            "unicode"
                        } else {
                            "bytes"
                        },
                        Ty::Nil,
                        Ty::Nil,
                    )?,
                    vec![value.into()],
                    "string_view_value",
                )?)
            }
            TExpr::ViewLength(value, ty) => {
                let value = self.expr(env, loops, value)?;
                let value =
                    self.descriptor_ptr(value, self.view_type().into(), "view_length_input")?;
                let call = self.invoke(
                    self.runtime_import(
                        Family::View,
                        if *ty == Ty::ViewUnicode {
                            "length_unicode"
                        } else {
                            "length_byte"
                        },
                        Ty::Nil,
                        Ty::Nil,
                    )?,
                    &[value.into()],
                )?;
                Ok(call
                    .try_as_basic_value()
                    .expect_basic("view length returns Int64"))
            }
            TExpr::ViewAt(value, index, view_ty, sum) => {
                let value = self.expr(env, loops, value)?;
                let index = self.expr(env, loops, index)?;
                let value = self.descriptor_ptr(value, self.view_type().into(), "view_at_input")?;
                let raw = self
                    .invoke(
                        self.runtime_import(
                            Family::View,
                            if *view_ty == Ty::ViewUnicode {
                                "at_unicode"
                            } else {
                                "at_byte"
                            },
                            Ty::Nil,
                            Ty::Nil,
                        )?,
                        &[value.into(), index.into()],
                    )?
                    .try_as_basic_value()
                    .expect_basic("view lookup returns a signed sentinel")
                    .into_int_value();
                let nil = self.context.i64_type().const_int(u64::MAX, true);
                let is_nil = self
                    .builder
                    .build_int_compare(IntPredicate::EQ, raw, nil, "view_at_nil")
                    .map_err(|error| error.to_string())?;
                let members = self.program.sums[sum.index()].clone();
                let success = if *view_ty == Ty::ViewUnicode {
                    Ty::Unicode
                } else {
                    Ty::Byte
                };
                let success_tag = self.sum_member_tag(*sum, success)?;
                let nil_tag = self.sum_member_tag(*sum, Ty::Nil)?;
                let zeroed = self.struct_ty(Ty::Sum(*sum))?.const_zero();
                let nil_value = self
                    .builder
                    .build_insert_value(
                        zeroed,
                        self.context.i32_type().const_int(u64::from(nil_tag), false),
                        0,
                        "nil_tag",
                    )
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                let payload: BasicValueEnum<'ctx> = if *view_ty == Ty::ViewUnicode {
                    self.builder
                        .build_int_truncate(raw, self.context.i32_type(), "unicode_at")
                        .map_err(|error| error.to_string())?
                        .into()
                } else {
                    self.builder
                        .build_int_truncate(raw, self.context.i8_type(), "byte_at")
                        .map_err(|error| error.to_string())?
                        .into()
                };
                let success_value = self
                    .builder
                    .build_insert_value(
                        zeroed,
                        self.context
                            .i32_type()
                            .const_int(u64::from(success_tag), false),
                        0,
                        "value_tag",
                    )
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                let success_value = self
                    .builder
                    .build_insert_value(success_value, payload, success_tag + 1, "value_payload")
                    .map_err(|error| error.to_string())?;
                let _ = members;
                Ok(self
                    .builder
                    .build_select(
                        is_nil,
                        nil_value,
                        success_value.into_struct_value(),
                        "view_at",
                    )
                    .map_err(|error| error.to_string())?)
            }
            TExpr::MapContains {
                receiver,
                key,
                key_ty,
                value_ty,
            } => self.lower_map_contains(env, loops, receiver, key, *key_ty, *value_ty),
            TExpr::MapInsert {
                receiver,
                key,
                value,
                key_ty,
                value_ty,
                require_existing,
            } => self.lower_map_insert(
                env,
                loops,
                receiver,
                key,
                value,
                *key_ty,
                *value_ty,
                *require_existing,
            ),
            TExpr::MapDelete {
                receiver,
                key,
                key_ty,
                value_ty,
            } => self.lower_map_delete(env, loops, receiver, key, *key_ty, *value_ty),
            TExpr::MapIndex {
                receiver,
                key,
                key_ty,
                value_ty,
            } => self.lower_map_index(env, loops, receiver, key, *key_ty, *value_ty),
            TExpr::MapTake {
                receiver,
                key,
                key_ty,
                value_ty,
            } => self.lower_map_take(env, loops, receiver, key, *key_ty, *value_ty),
            TExpr::SetContains {
                receiver,
                value,
                elem,
            } => self.lower_set_contains(env, loops, receiver, value, *elem),
            TExpr::SetInsert {
                receiver,
                value,
                elem,
            } => self.lower_set_insert(env, loops, receiver, value, *elem),
            TExpr::SetDelete {
                receiver,
                value,
                elem,
            } => self.lower_set_delete(env, loops, receiver, value, *elem),
            TExpr::CollectionLiteral { ty, items } => {
                self.lower_collection_literal(env, loops, *ty, items)
            }
            TExpr::CollectionNew(ty) => self.lower_collection_new(*ty),
            TExpr::CollectionLength(value) => self.lower_collection_length(env, loops, value),
            TExpr::CollectionIsEmpty(value) => self.lower_collection_is_empty(env, loops, value),
            TExpr::ViewSlice {
                value,
                start,
                end,
                view_ty,
                sum,
            } => {
                let value = self.expr(env, loops, value)?;
                let start = self.expr(env, loops, start)?;
                let end = self.expr(env, loops, end)?;
                let value =
                    self.descriptor_ptr(value, self.view_type().into(), "view_slice_input")?;
                let slice = self
                    .view_out_call(
                        self.runtime_import(
                            Family::View,
                            if *view_ty == Ty::ViewUnicode {
                                "slice_unicode"
                            } else {
                                "slice_byte"
                            },
                            Ty::Nil,
                            Ty::Nil,
                        )?,
                        vec![value.into(), start.into(), end.into()],
                        "view_slice_value",
                    )?
                    .into_struct_value();
                let valid = self
                    .builder
                    .build_is_not_null(
                        self.builder
                            .build_extract_value(slice, 0, "slice_ptr")
                            .map_err(|error| error.to_string())?
                            .into_pointer_value(),
                        "slice_valid",
                    )
                    .map_err(|error| error.to_string())?;
                let nil_tag = self.sum_member_tag(*sum, Ty::Nil)?;
                let view_tag = self.sum_member_tag(*sum, *view_ty)?;
                let zeroed = self.struct_ty(Ty::Sum(*sum))?.const_zero();
                let nil = self
                    .builder
                    .build_insert_value(
                        zeroed,
                        self.context.i32_type().const_int(u64::from(nil_tag), false),
                        0,
                        "slice_nil",
                    )
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                let tagged = self
                    .builder
                    .build_insert_value(
                        zeroed,
                        self.context
                            .i32_type()
                            .const_int(u64::from(view_tag), false),
                        0,
                        "slice_view",
                    )
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                let view = self
                    .builder
                    .build_insert_value(tagged, slice, view_tag + 1, "slice_payload")
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                Ok(self
                    .builder
                    .build_select(valid, view, nil, "slice_result")
                    .map_err(|error| error.to_string())?)
            }
            TExpr::CollectionCapacity(value) => self.lower_collection_capacity(env, loops, value),
            TExpr::CollectionView(value, _) => self.lower_collection_view(env, loops, value),
            TExpr::CollectionSlice {
                value,
                start,
                end,
                view_ty,
                sum,
                elem,
            } => self.lower_collection_slice(env, loops, value, start, end, *view_ty, *sum, *elem),
            TExpr::CollectionIndex {
                collection,
                index,
                collection_ty: _,
                elem,
            } => self.lower_collection_index(env, loops, collection, index, *elem),
            TExpr::ListPop { receiver, elem } => self.lower_list_pop(env, receiver, *elem),
            TExpr::ListRemove {
                receiver,
                index,
                elem,
            } => self.lower_list_remove(env, loops, receiver, index, *elem),
            TExpr::Cast(value, Ty::Float64) => {
                let value = self.expr(env, loops, value)?.into_int_value();
                Ok(self
                    .builder
                    .build_signed_int_to_float(value, self.context.f64_type(), "i64_to_f64")
                    .map_err(|e| e.to_string())?
                    .into())
            }
            TExpr::Cast(_, _) => Err(internal("checker emitted an unsupported cast")),
            // RFC 016 Task B's `UseMode` records whether this read is a
            // consuming context for the checker's own move-availability
            // analysis; lowering reads the place identically either way.
            TExpr::Place(place, _) => self.place_value(env, place),
            TExpr::FieldRead {
                base,
                base_ty,
                index,
                ty,
            } => {
                let base_value = self.expr(env, loops, base)?;
                match base_ty {
                    // Specification 016 section 4.3: a fresh box value (not a
                    // place) reached this field access, so lowering derefs it
                    // the same number of layers `checker.rs`'s `deref_box`
                    // did to resolve the field, then reads through the
                    // resulting real address instead of extracting from an
                    // (absent) aggregate SSA value.
                    Ty::Box(id) => {
                        let pointee = self.box_pointee(*id);
                        let (ptr, struct_ty) =
                            self.deref_box_ptr(base_value.into_pointer_value(), pointee)?;
                        let field = self
                            .builder
                            .build_struct_gep(
                                self.struct_ty(struct_ty)?,
                                ptr,
                                *index as u32,
                                "field",
                            )
                            .map_err(|error| error.to_string())?;
                        self.builder
                            .build_load(self.ty(*ty), field, "field")
                            .map_err(|error| error.to_string())
                    }
                    _ => self
                        .builder
                        .build_extract_value(as_struct(base_value)?, *index as u32, "field")
                        .map_err(|error| error.to_string()),
                }
            }
            // Specification 010 section 8.2: arguments evaluate left to right in
            // written order, then land in their declared field slots.
            TExpr::Construct { type_id, fields } => {
                let mut values = Vec::with_capacity(fields.len());
                for (index, value) in fields {
                    values.push((*index as u32, self.expr(env, loops, value)?));
                }
                let mut aggregate = self.struct_ty(Ty::User(*type_id))?.const_zero();
                for (index, value) in values {
                    aggregate = self
                        .builder
                        .build_insert_value(aggregate, value, index, "field")
                        .map_err(|error| error.to_string())?
                        .into_struct_value();
                }
                Ok(aggregate.into())
            }
            // Adding or removing a represented layer is identity at runtime.
            TExpr::Represent { value, .. } => self.expr(env, loops, value),
            // Specification 010 section 15.2: construction begins from the
            // complete union's zero initializer, then writes the tag and the
            // active member. Inactive storage is deterministic, never poison.
            TExpr::Inject {
                member,
                into_union,
                value,
            } => {
                let value = self.expr(env, loops, value)?;
                let tag = self.member_tag(*member)?;
                let zeroed = self.struct_ty(Ty::User(*into_union))?.const_zero();
                let tagged = self
                    .builder
                    .build_insert_value(
                        zeroed,
                        self.context.i32_type().const_int(u64::from(tag), false),
                        0,
                        "tag",
                    )
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                let injected = self
                    .builder
                    .build_insert_value(tagged, value, tag + 1, "member")
                    .map_err(|error| error.to_string())?;
                Ok(injected.into_struct_value().into())
            }
            // Specification 018 section 8 and Phase 4 items 2-3 reuse
            // `TExpr::Inject`'s strategy exactly: start from the complete
            // sum's zero initializer, then write the tag and the active
            // member, so an inactive field is always deterministic. Only the
            // tag source differs -- a sum member's tag is its position in the
            // canonical member list, computed on demand, since `InjectSum`
            // never records one itself.
            TExpr::InjectSum { sum, member, value } => {
                let value = self.expr(env, loops, value)?;
                let tag = self.sum_member_tag(*sum, *member)?;
                let zeroed = self.struct_ty(Ty::Sum(*sum))?.const_zero();
                let tagged = self
                    .builder
                    .build_insert_value(
                        zeroed,
                        self.context.i32_type().const_int(u64::from(tag), false),
                        0,
                        "tag",
                    )
                    .map_err(|error| error.to_string())?
                    .into_struct_value();
                let injected = self
                    .builder
                    .build_insert_value(tagged, value, tag + 1, "member")
                    .map_err(|error| error.to_string())?;
                Ok(injected.into_struct_value().into())
            }
            TExpr::LiftSum { value, from, to } => {
                let value = self.expr(env, loops, value)?;
                let source = as_struct(value)?;
                let source_tag = self
                    .builder
                    .build_extract_value(source, 0, "source_tag")
                    .map_err(|error| error.to_string())?
                    .into_int_value();
                let source_members = self.program.sums[from.index()].clone();
                let function = self.current_function();
                let done = self.context.append_basic_block(function, "lift_done");
                let unknown = self.context.append_basic_block(function, "lift_unknown");
                let cases: Vec<_> = source_members
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        (
                            self.context.i32_type().const_int(index as u64, false),
                            self.context.append_basic_block(function, "lift_member"),
                        )
                    })
                    .collect();
                self.builder
                    .build_switch(source_tag, unknown, &cases)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(unknown);
                self.exhausted()?;
                let target_ty = self.struct_ty(Ty::Sum(*to))?;
                let zeroed = target_ty.const_zero();
                let mut incoming = Vec::with_capacity(source_members.len());
                for (member, (_, block)) in source_members.iter().zip(cases) {
                    self.builder.position_at_end(block);
                    let target_tag = self.sum_member_tag(*to, *member)?;
                    let tagged = self
                        .builder
                        .build_insert_value(
                            zeroed,
                            self.context
                                .i32_type()
                                .const_int(u64::from(target_tag), false),
                            0,
                            "lift_tag",
                        )
                        .map_err(|error| error.to_string())?
                        .into_struct_value();
                    let payload = if *member == Ty::Nil {
                        self.context.i8_type().const_zero().into()
                    } else {
                        let source_tag = self.sum_member_tag(*from, *member)?;
                        self.builder
                            .build_extract_value(source, source_tag + 1, "lift_payload")
                            .map_err(|error| error.to_string())?
                    };
                    let lifted = self
                        .builder
                        .build_insert_value(tagged, payload, target_tag + 1, "lift_payload")
                        .map_err(|error| error.to_string())?
                        .into_struct_value();
                    let current = self
                        .builder
                        .get_insert_block()
                        .ok_or_else(|| internal("sum lift arm has no block"))?;
                    self.builder
                        .build_unconditional_branch(done)
                        .map_err(|error| error.to_string())?;
                    incoming.push((lifted, current));
                }
                self.builder.position_at_end(done);
                let phi = self
                    .builder
                    .build_phi(target_ty, "lifted_sum")
                    .map_err(|error| error.to_string())?;
                let incoming: Vec<(&dyn BasicValue<'ctx>, BasicBlock<'ctx>)> = incoming
                    .iter()
                    .map(|(value, block)| (value as &dyn BasicValue<'ctx>, *block))
                    .collect();
                phi.add_incoming(&incoming);
                Ok(phi.as_basic_value())
            }
            TExpr::Not(value) => {
                let value = self.expr(env, loops, value)?.into_int_value();
                let result = self
                    .builder
                    .build_int_compare(
                        IntPredicate::EQ,
                        value,
                        self.context.i8_type().const_zero(),
                        "not",
                    )
                    .map_err(|error| error.to_string())?;
                Ok(self
                    .builder
                    .build_int_z_extend(result, self.context.i8_type(), "bool")
                    .map_err(|error| error.to_string())?
                    .into())
            }
            TExpr::Logical(left, op, right) => {
                let left = self.expr(env, loops, left)?.into_int_value();
                let function = self.current_function();
                let right_block = self.context.append_basic_block(function, "logical_right");
                let short_block = self.context.append_basic_block(function, "logical_short");
                let merge = self.context.append_basic_block(function, "logical_merge");
                let left_true = self
                    .builder
                    .build_int_compare(
                        IntPredicate::NE,
                        left,
                        self.context.i8_type().const_zero(),
                        "logical_test",
                    )
                    .map_err(|error| error.to_string())?;
                let (when_true, when_false, short_value) = match op {
                    LogicalOp::And => (
                        right_block,
                        short_block,
                        self.context.i8_type().const_zero(),
                    ),
                    LogicalOp::Or => (
                        short_block,
                        right_block,
                        self.context.i8_type().const_int(1, false),
                    ),
                };
                self.builder
                    .build_conditional_branch(left_true, when_true, when_false)
                    .map_err(|error| error.to_string())?;

                self.builder.position_at_end(short_block);
                self.builder
                    .build_unconditional_branch(merge)
                    .map_err(|error| error.to_string())?;
                let short_end = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| internal("logical short-circuit block has no end"))?;

                self.builder.position_at_end(right_block);
                let right = self.expr(env, loops, right)?.into_int_value();
                self.builder
                    .build_unconditional_branch(merge)
                    .map_err(|error| error.to_string())?;
                let right_end = self
                    .builder
                    .get_insert_block()
                    .ok_or_else(|| internal("logical right-hand block has no end"))?;

                self.builder.position_at_end(merge);
                let phi = self
                    .builder
                    .build_phi(self.context.i8_type(), "logical_value")
                    .map_err(|error| error.to_string())?;
                phi.add_incoming(&[(&short_value, short_end), (&right, right_end)]);
                Ok(phi.as_basic_value())
            }
            TExpr::Truthiness(value, ty) => {
                let value = self.expr(env, loops, value)?;
                Ok(self.truthiness_value(*ty, value)?.into())
            }
            TExpr::ReturnOnError {
                value,
                sum,
                success,
                result,
                cleanup,
            } => self.lower_return_on_error(env, loops, value, *sum, *success, *result, cleanup),
            TExpr::Arith(left, op, right, ty) => {
                let ty = *ty;
                let left = self.expr(env, loops, left)?;
                let right = self.expr(env, loops, right)?;
                let builder = self.builder;
                if is_float(ty) {
                    // `Float32` operands are already `float`, so every rounding
                    // happens at binary32 and never through `double`.
                    let (left, right) = (left.into_float_value(), right.into_float_value());
                    let value = match op {
                        ArithOp::Add => builder.build_float_add(left, right, "add"),
                        ArithOp::Sub => builder.build_float_sub(left, right, "sub"),
                        ArithOp::Mul => builder.build_float_mul(left, right, "mul"),
                        ArithOp::Div => builder.build_float_div(left, right, "div"),
                    }
                    .map_err(|e| e.to_string())?;
                    self.validate_float(value, "arithmetic_nan")?;
                    return Ok(value.into());
                }
                if matches!(ty, Ty::Bool | Ty::Nil | Ty::User(_) | Ty::Sum(_)) {
                    return Err(internal("checker produced non-numeric arithmetic"));
                }
                // Plain `add`/`sub`/`mul` on an N-bit integer already wrap
                // modulo 2^N; no no-wrap flag may be added or the modular
                // result Specification 009 section 4.5 requires becomes poison.
                // Unsigned division is `udiv`, whose division by zero is
                // undefined behavior by that same section -- deliberately
                // unguarded.
                let (left, right) = (left.into_int_value(), right.into_int_value());
                let value = match op {
                    ArithOp::Add => builder.build_int_add(left, right, "add"),
                    ArithOp::Sub => builder.build_int_sub(left, right, "sub"),
                    ArithOp::Mul => builder.build_int_mul(left, right, "mul"),
                    ArithOp::Div if is_unsigned(ty) => {
                        builder.build_int_unsigned_div(left, right, "div")
                    }
                    ArithOp::Div => builder.build_int_signed_div(left, right, "div"),
                }
                .map_err(|e| e.to_string())?;
                Ok(value.into())
            }
            TExpr::Cmp(left, op, right, operand_ty) => {
                let operand_ty = *operand_ty;
                let left = self.expr(env, loops, left)?;
                let right = self.expr(env, loops, right)?;
                let builder = self.builder;
                let ordered = !matches!(op, CmpOp::Eq | CmpOp::NotEq);
                if ordered && matches!(operand_ty, Ty::Bool | Ty::Nil | Ty::User(_) | Ty::Sum(_)) {
                    return Err(internal(
                        "checker allowed an ordered non-numeric comparison",
                    ));
                }
                let comparison = if matches!(operand_ty, Ty::String) {
                    let left =
                        self.descriptor_ptr(left, self.string_type().into(), "equal_left")?;
                    let right =
                        self.descriptor_ptr(right, self.string_type().into(), "equal_right")?;
                    let call = self.invoke(
                        self.runtime_import(Family::String, "equal", Ty::Nil, Ty::Nil)?,
                        &[left.into(), right.into()],
                    )?;
                    let equal = call
                        .try_as_basic_value()
                        .expect_basic("string equality returns a byte")
                        .into_int_value();
                    match op {
                        CmpOp::Eq => equal,
                        CmpOp::NotEq => builder
                            .build_xor(equal, self.context.i8_type().const_int(1, false), "ne")
                            .map_err(|error| error.to_string())?,
                        _ => return Err(internal("checker allowed ordered string comparison")),
                    }
                } else if matches!(operand_ty, Ty::ViewByte | Ty::ViewUnicode) {
                    let left = self.descriptor_ptr(left, self.view_type().into(), "equal_left")?;
                    let right =
                        self.descriptor_ptr(right, self.view_type().into(), "equal_right")?;
                    let equal = self
                        .invoke(
                            self.runtime_import(Family::View, "equal", Ty::Nil, Ty::Nil)?,
                            &[left.into(), right.into()],
                        )?
                        .try_as_basic_value()
                        .expect_basic("view equality returns a byte")
                        .into_int_value();
                    match op {
                        CmpOp::Eq => equal,
                        CmpOp::NotEq => builder
                            .build_xor(equal, self.context.i8_type().const_int(1, false), "ne")
                            .map_err(|error| error.to_string())?,
                        _ => return Err(internal("checker allowed ordered view comparison")),
                    }
                } else if matches!(operand_ty, Ty::Array(_) | Ty::List(_) | Ty::View(_)) {
                    let equal = self.equal(operand_ty, left, right)?;
                    match op {
                        CmpOp::Eq => equal,
                        CmpOp::NotEq => builder
                            .build_not(equal, "ne")
                            .map_err(|error| error.to_string())?,
                        _ => return Err(internal("checker allowed ordered collection comparison")),
                    }
                } else if matches!(operand_ty, Ty::User(_) | Ty::Sum(_)) {
                    // Recursive, type-directed equality; `!=` is its negation.
                    let equal = self.equal(operand_ty, left, right)?;
                    match op {
                        CmpOp::Eq => equal,
                        _ => builder
                            .build_not(equal, "ne")
                            .map_err(|error| error.to_string())?,
                    }
                } else if is_float(operand_ty) {
                    // `Float32` reuses the `Float64` rule: every predicate but
                    // `!=` is ordered, so a NaN operand makes it false.
                    let predicate = match op {
                        CmpOp::Eq => FloatPredicate::OEQ,
                        CmpOp::NotEq => FloatPredicate::UNE,
                        CmpOp::Less => FloatPredicate::OLT,
                        CmpOp::LessEq => FloatPredicate::OLE,
                        CmpOp::Greater => FloatPredicate::OGT,
                        CmpOp::GreaterEq => FloatPredicate::OGE,
                    };
                    builder
                        .build_float_compare(
                            predicate,
                            left.into_float_value(),
                            right.into_float_value(),
                            "compare",
                        )
                        .map_err(|error| error.to_string())?
                } else {
                    let unsigned = is_unsigned(operand_ty);
                    let predicate = match op {
                        CmpOp::Eq => IntPredicate::EQ,
                        CmpOp::NotEq => IntPredicate::NE,
                        CmpOp::Less if unsigned => IntPredicate::ULT,
                        CmpOp::Less => IntPredicate::SLT,
                        CmpOp::LessEq if unsigned => IntPredicate::ULE,
                        CmpOp::LessEq => IntPredicate::SLE,
                        CmpOp::Greater if unsigned => IntPredicate::UGT,
                        CmpOp::Greater => IntPredicate::SGT,
                        CmpOp::GreaterEq if unsigned => IntPredicate::UGE,
                        CmpOp::GreaterEq => IntPredicate::SGE,
                    };
                    builder
                        .build_int_compare(
                            predicate,
                            left.into_int_value(),
                            right.into_int_value(),
                            "compare",
                        )
                        .map_err(|error| error.to_string())?
                };
                let value = builder
                    .build_int_z_extend(comparison, self.context.i8_type(), "bool")
                    .map_err(|error| error.to_string())?;
                Ok(value.into())
            }
            TExpr::Call(name, args) => {
                let call = self.call(env, loops, name, args)?;
                let value = call
                    .try_as_basic_value()
                    .expect_basic("a checked call expression always returns a value");
                let result_ty = self
                    .program
                    .externs
                    .get(name)
                    .and_then(|function| function.result)
                    .or_else(|| {
                        self.program
                            .funcs
                            .get(name)
                            .and_then(|function| function.result)
                    });
                if matches!(result_ty, Some(Ty::Float32 | Ty::Float64)) {
                    self.validate_float(value.into_float_value(), "bridge_float_result_nan")?;
                }
                Ok(value)
            }
            TExpr::MethodCall(call) => {
                let method_id = call.method;
                let call_value = self.method_call(env, loops, call)?;
                let value = call_value
                    .try_as_basic_value()
                    .expect_basic("a checked method call expression always returns a value");
                if matches!(
                    self.program.methods[method_id.index()].result,
                    Some(Ty::Float32 | Ty::Float64)
                ) {
                    self.validate_float(value.into_float_value(), "method_float_result_nan")?;
                }
                Ok(value)
            }
            TExpr::If(form) => self.value_if(env, loops, form),
            TExpr::Print(value, ty) => {
                let value = self.expr(env, loops, value)?;
                let function = self.runtime_import(Family::Print, print_op(*ty)?, *ty, Ty::Nil)?;
                if *ty == Ty::String {
                    let value =
                        self.descriptor_ptr(value, self.string_type().into(), "print_string")?;
                    self.invoke(function, &[value.into()])?;
                } else {
                    self.invoke(function, &[value.into()])?;
                }
                Ok(value)
            }
            // Specification 016 section 4.2 and 8.2: the operand evaluates
            // exactly once, the runtime allocator is sized and aligned for
            // the *pointee* type (not the box pointer itself), and the
            // evaluated value is stored into the fresh allocation before its
            // address is produced as the box's own value. The cleanup
            // obligation this allocation creates is not registered here --
            // it is whatever the checked cleanup plan already attached to
            // wherever this `box(...)` result ends up bound (a block's
            // `drops` or an assignment's `drop_before`), exactly like any
            // other move-only value.
            TExpr::Box(operand, ty) => {
                let Ty::Box(id) = *ty else {
                    return Err(internal("a 'box(...)' node did not have a box result type"));
                };
                let pointee = self.box_pointee(id);
                let value = self.expr(env, loops, operand)?;
                let (size, align) = self.size_align(pointee);
                let usize_ty = self.usize_ty();
                let alloc_fn = self.alloc_import();
                let call = self.invoke(
                    alloc_fn,
                    &[
                        usize_ty.const_int(size, false).into(),
                        usize_ty.const_int(align, false).into(),
                    ],
                )?;
                let ptr = call
                    .try_as_basic_value()
                    .expect_basic("'snacc_alloc' always returns a pointer")
                    .into_pointer_value();
                self.builder
                    .build_store(ptr, value)
                    .map_err(|error| error.to_string())?;
                Ok(ptr.into())
            }
        }
    }
}
