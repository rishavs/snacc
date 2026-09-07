//! Structural equality and `return_on_error` lowering (Specification 029 Phase 3).

use super::*;

impl<'ctx> Codegen<'ctx, '_> {
    pub(crate) fn value_if(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        form: &TValueIf,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let function = self.current_function();
        let merge = self.context.append_basic_block(function, "if_merge");
        let mut incoming: Vec<(BasicValueEnum<'ctx>, BasicBlock<'ctx>)> = Vec::new();
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
            self.branch_value(env, loops, body, merge, &mut incoming)?;
            env.truncate(scope);
            self.builder.position_at_end(next_block);
        }
        match (&form.else_branch, form.exhaustive) {
            (Some(body), _) => self.branch_value(env, loops, body, merge, &mut incoming)?,
            (None, true) => self.exhausted()?,
            (None, false) => {
                return Err(internal(
                    "a value-producing 'if' has neither an 'else' nor a proven-exhaustive chain",
                ));
            }
        }

        self.builder.position_at_end(merge);
        if incoming.is_empty() {
            return Err(internal(
                "a value-producing 'if' had no branch that produces a value",
            ));
        }
        let phi = self
            .builder
            .build_phi(self.ty(form.ty), "if_value")
            .map_err(|error| error.to_string())?;
        let incoming: Vec<(&dyn BasicValue<'ctx>, BasicBlock<'ctx>)> = incoming
            .iter()
            .map(|(value, block)| (value as &dyn BasicValue<'ctx>, *block))
            .collect();
        phi.add_incoming(&incoming);
        Ok(phi.as_basic_value())
    }

    /// Lowers one branch of a value-producing `if`, recording its incoming phi
    /// edge only when the branch actually reaches the merge block.
    pub(crate) fn branch_value(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        body: &TBlock,
        merge: BasicBlock<'ctx>,
        incoming: &mut Vec<(BasicValueEnum<'ctx>, BasicBlock<'ctx>)>,
    ) -> Result<(), String> {
        let (value, terminated) = self.block(env, loops, body)?;
        if terminated {
            return Ok(());
        }
        let value = value.ok_or("a value-producing 'if' branch produced no value")?;
        let end = self
            .builder
            .get_insert_block()
            .expect("a lowered branch has an insertion block");
        self.builder
            .build_unconditional_branch(merge)
            .map_err(|error| error.to_string())?;
        incoming.push((value, end));
        Ok(())
    }

    /// Type-directed structural equality producing an `i1`
    /// (Specification 010 sections 7.3, 8.4, and 9.2). Specification 018
    /// section 8 extends this structurally to an inline sum, reusing
    /// [`Self::equal_union`]'s tag-then-active-member strategy through
    /// [`Self::equal_sum`].
    pub(crate) fn equal(
        &self,
        ty: Ty,
        left: BasicValueEnum<'ctx>,
        right: BasicValueEnum<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        match ty {
            Ty::User(id) => match self.def(id) {
                // A represented type is its target at runtime, so it delegates.
                TypeDef::Represented { target, .. } => self.equal(*target, left, right),
                TypeDef::Struct { fields, .. } | TypeDef::UnionMember { fields, .. } => {
                    self.equal_fields(fields, left, right)
                }
                TypeDef::Union { members, .. } => self.equal_union(members, left, right),
            },
            Ty::Sum(id) => self.equal_sum(&self.program.sums[id.index()], left, right),
            Ty::Array(id) | Ty::List(id) | Ty::View(id) => {
                let elem = match &self.program.collections[id.index()] {
                    crate::types::CollectionDef::Array { elem, .. }
                    | crate::types::CollectionDef::List { elem }
                    | crate::types::CollectionDef::View { elem } => *elem,
                    _ => return Err(internal("collection equality has non-sequence metadata")),
                };
                self.equal_collection(elem, left, right)
            }
            Ty::String | Ty::ViewByte | Ty::ViewUnicode => {
                let descriptor_ty = if ty == Ty::String {
                    self.string_type().into()
                } else {
                    self.view_type().into()
                };
                let left = self.descriptor_ptr(left, descriptor_ty, "equal_left")?;
                let right = self.descriptor_ptr(right, descriptor_ty, "equal_right")?;
                let equal = if ty == Ty::String {
                    self.invoke(
                        self.runtime_import(Family::String, "equal", Ty::Nil, Ty::Nil)?,
                        &[left.into(), right.into()],
                    )?
                    .try_as_basic_value()
                    .expect_basic("string equality returns a byte")
                    .into_int_value()
                } else {
                    self.invoke(
                        self.runtime_import(Family::View, "equal", Ty::Nil, Ty::Nil)?,
                        &[left.into(), right.into()],
                    )?
                    .try_as_basic_value()
                    .expect_basic("view equality returns a byte")
                    .into_int_value()
                };
                self.builder
                    .build_int_compare(
                        IntPredicate::NE,
                        equal,
                        self.context.i8_type().const_zero(),
                        "equal",
                    )
                    .map_err(|error| error.to_string())
            }
            _ if is_float(ty) => self
                .builder
                .build_float_compare(
                    FloatPredicate::OEQ,
                    left.into_float_value(),
                    right.into_float_value(),
                    "eq",
                )
                .map_err(|error| error.to_string()),
            _ => self
                .builder
                .build_int_compare(
                    IntPredicate::EQ,
                    left.into_int_value(),
                    right.into_int_value(),
                    "eq",
                )
                .map_err(|error| error.to_string()),
        }
    }

    /// Compares two same-typed sequence descriptors by length and then by
    /// increasing element index. The descriptor's third field is intentionally
    /// ignored: capacity is storage state, not sequence content.
    pub(crate) fn equal_collection(
        &self,
        elem: Ty,
        left: BasicValueEnum<'ctx>,
        right: BasicValueEnum<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        let left = as_struct(left)?;
        let right = as_struct(right)?;
        let left_ptr = self
            .builder
            .build_extract_value(left, 0, "left_collection_ptr")
            .map_err(|error| error.to_string())?
            .into_pointer_value();
        let right_ptr = self
            .builder
            .build_extract_value(right, 0, "right_collection_ptr")
            .map_err(|error| error.to_string())?
            .into_pointer_value();
        let left_len = self
            .builder
            .build_extract_value(left, 1, "left_collection_len")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let right_len = self
            .builder
            .build_extract_value(right, 1, "right_collection_len")
            .map_err(|error| error.to_string())?
            .into_int_value();

        let function = self.current_function();
        let length_match = self
            .context
            .append_basic_block(function, "collection_eq_length_match");
        let loop_block = self
            .context
            .append_basic_block(function, "collection_eq_loop");
        let element_block = self
            .context
            .append_basic_block(function, "collection_eq_element");
        let all_equal = self
            .context
            .append_basic_block(function, "collection_eq_all_equal");
        let done = self
            .context
            .append_basic_block(function, "collection_eq_done");
        let entry = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("collection equality has no insertion block"))?;
        let lengths_equal = self
            .builder
            .build_int_compare(IntPredicate::EQ, left_len, right_len, "lengths_equal")
            .map_err(|error| error.to_string())?;
        self.builder
            .build_conditional_branch(lengths_equal, length_match, done)
            .map_err(|error| error.to_string())?;

        self.builder.position_at_end(length_match);
        self.builder
            .build_unconditional_branch(loop_block)
            .map_err(|error| error.to_string())?;

        self.builder.position_at_end(loop_block);
        let index = self
            .builder
            .build_phi(self.context.i64_type(), "collection_eq_index")
            .map_err(|error| error.to_string())?;
        let zero = self.context.i64_type().const_zero();
        index.add_incoming(&[(&zero, length_match)]);
        let index_value = index.as_basic_value().into_int_value();
        let more = self
            .builder
            .build_int_compare(IntPredicate::ULT, index_value, left_len, "more_elements")
            .map_err(|error| error.to_string())?;
        self.builder
            .build_conditional_branch(more, element_block, all_equal)
            .map_err(|error| error.to_string())?;

        self.builder.position_at_end(element_block);
        let left_element_ptr = unsafe {
            self.builder
                .build_gep(self.ty(elem), left_ptr, &[index_value], "left_element_ptr")
        }
        .map_err(|error| error.to_string())?;
        let right_element_ptr = unsafe {
            self.builder.build_gep(
                self.ty(elem),
                right_ptr,
                &[index_value],
                "right_element_ptr",
            )
        }
        .map_err(|error| error.to_string())?;
        let left_element = self
            .builder
            .build_load(self.ty(elem), left_element_ptr, "left_element")
            .map_err(|error| error.to_string())?;
        let right_element = self
            .builder
            .build_load(self.ty(elem), right_element_ptr, "right_element")
            .map_err(|error| error.to_string())?;
        let equal = self.equal(elem, left_element, right_element)?;
        let mismatch = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("collection element equality has no block"))?;
        let next = self
            .context
            .append_basic_block(function, "collection_eq_next");
        self.builder
            .build_conditional_branch(equal, next, done)
            .map_err(|error| error.to_string())?;
        self.builder.position_at_end(next);
        let next_index = self
            .builder
            .build_int_add(
                index_value,
                self.context.i64_type().const_int(1, false),
                "next_index",
            )
            .map_err(|error| error.to_string())?;
        self.builder
            .build_unconditional_branch(loop_block)
            .map_err(|error| error.to_string())?;
        index.add_incoming(&[(&next_index, next)]);

        self.builder.position_at_end(all_equal);
        let all_equal_end = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("collection equality completion has no block"))?;
        self.builder
            .build_unconditional_branch(done)
            .map_err(|error| error.to_string())?;

        self.builder.position_at_end(done);
        let false_value = self.context.bool_type().const_zero();
        let true_value = self.context.bool_type().const_all_ones();
        self.phi_bool(&[
            (false_value, entry),
            (false_value, mismatch),
            (true_value, all_equal_end),
        ])
    }

    /// Fields compare in declaration order with short-circuiting. All values of
    /// an empty struct type are equal.
    pub(crate) fn equal_fields(
        &self,
        fields: &[(String, Ty)],
        left: BasicValueEnum<'ctx>,
        right: BasicValueEnum<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        let extract = |value: BasicValueEnum<'ctx>, index: usize| {
            self.builder
                .build_extract_value(as_struct(value)?, index as u32, "field")
                .map_err(|error| error.to_string())
        };
        match fields.len() {
            0 => return Ok(self.context.bool_type().const_all_ones()),
            1 => return self.equal(fields[0].1, extract(left, 0)?, extract(right, 0)?),
            _ => {}
        }
        let function = self.current_function();
        let done = self.context.append_basic_block(function, "eq_done");
        let unequal = self.context.bool_type().const_zero();
        let mut incoming: Vec<(IntValue<'ctx>, BasicBlock<'ctx>)> = Vec::new();
        let last = fields.len() - 1;
        for (index, (_, field_ty)) in fields.iter().enumerate() {
            let equal = self.equal(*field_ty, extract(left, index)?, extract(right, index)?)?;
            let current = self
                .builder
                .get_insert_block()
                .expect("a comparison has an insertion block");
            if index == last {
                self.builder
                    .build_unconditional_branch(done)
                    .map_err(|error| error.to_string())?;
                incoming.push((equal, current));
            } else {
                let next = self.context.append_basic_block(function, "eq_next");
                self.builder
                    .build_conditional_branch(equal, next, done)
                    .map_err(|error| error.to_string())?;
                // The short-circuit edge reaches `done` only when this field
                // differed, so it carries `false`.
                incoming.push((unequal, current));
                self.builder.position_at_end(next);
            }
        }
        self.builder.position_at_end(done);
        self.phi_bool(&incoming)
    }

    /// Union equality compares tags first and only then the active member; no
    /// inactive member field is ever read (Specification 010 section 15.4).
    pub(crate) fn equal_union(
        &self,
        members: &[TypeId],
        left: BasicValueEnum<'ctx>,
        right: BasicValueEnum<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        let function = self.current_function();
        let done = self.context.append_basic_block(function, "union_eq_done");
        let matched = self
            .context
            .append_basic_block(function, "union_eq_matched");
        let left_tag = self
            .builder
            .build_extract_value(as_struct(left)?, 0, "tag")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let right_tag = self
            .builder
            .build_extract_value(as_struct(right)?, 0, "tag")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let same = self
            .builder
            .build_int_compare(IntPredicate::EQ, left_tag, right_tag, "tag_eq")
            .map_err(|error| error.to_string())?;
        let entry = self
            .builder
            .get_insert_block()
            .expect("a comparison has an insertion block");
        self.builder
            .build_conditional_branch(same, matched, done)
            .map_err(|error| error.to_string())?;
        // Different tags are unequal without touching either payload.
        let mut incoming = vec![(self.context.bool_type().const_zero(), entry)];

        self.builder.position_at_end(matched);
        let unknown = self
            .context
            .append_basic_block(function, "union_eq_unknown");
        let mut cases = Vec::new();
        for member in members {
            let tag = self.member_tag(*member)?;
            let block = self.context.append_basic_block(function, "union_eq_member");
            cases.push((
                self.context.i32_type().const_int(u64::from(tag), false),
                block,
            ));
        }
        self.builder
            .build_switch(left_tag, unknown, &cases)
            .map_err(|error| error.to_string())?;
        // A stored tag outside the union's members means construction wrote one
        // that does not exist.
        self.builder.position_at_end(unknown);
        self.exhausted()?;

        for (member, (_, block)) in members.iter().zip(cases) {
            let field = self.member_tag(*member)? + 1;
            self.builder.position_at_end(block);
            let left = self
                .builder
                .build_extract_value(as_struct(left)?, field, "member")
                .map_err(|error| error.to_string())?;
            let right = self
                .builder
                .build_extract_value(as_struct(right)?, field, "member")
                .map_err(|error| error.to_string())?;
            let equal = self.equal(Ty::User(*member), left, right)?;
            let current = self
                .builder
                .get_insert_block()
                .expect("a comparison has an insertion block");
            self.builder
                .build_unconditional_branch(done)
                .map_err(|error| error.to_string())?;
            incoming.push((equal, current));
        }

        self.builder.position_at_end(done);
        self.phi_bool(&incoming)
    }

    /// An inline sum's equality: identical strategy to [`Self::equal_union`]
    /// (Specification 018 section 8 reuses named-union equality unchanged),
    /// except a member's deterministic tag is its position in `members`
    /// rather than a `TypeId`'s own tag, since a sum member is not always one.
    pub(crate) fn equal_sum(
        &self,
        members: &[Ty],
        left: BasicValueEnum<'ctx>,
        right: BasicValueEnum<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        let function = self.current_function();
        let done = self.context.append_basic_block(function, "sum_eq_done");
        let matched = self.context.append_basic_block(function, "sum_eq_matched");
        let left_tag = self
            .builder
            .build_extract_value(as_struct(left)?, 0, "tag")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let right_tag = self
            .builder
            .build_extract_value(as_struct(right)?, 0, "tag")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let same = self
            .builder
            .build_int_compare(IntPredicate::EQ, left_tag, right_tag, "tag_eq")
            .map_err(|error| error.to_string())?;
        let entry = self
            .builder
            .get_insert_block()
            .expect("a comparison has an insertion block");
        self.builder
            .build_conditional_branch(same, matched, done)
            .map_err(|error| error.to_string())?;
        // Different tags are unequal without touching either payload.
        let mut incoming = vec![(self.context.bool_type().const_zero(), entry)];

        self.builder.position_at_end(matched);
        let unknown = self.context.append_basic_block(function, "sum_eq_unknown");
        let cases: Vec<_> = (0..members.len())
            .map(|tag| {
                (
                    self.context.i32_type().const_int(tag as u64, false),
                    self.context.append_basic_block(function, "sum_eq_member"),
                )
            })
            .collect();
        self.builder
            .build_switch(left_tag, unknown, &cases)
            .map_err(|error| error.to_string())?;
        // A stored tag outside the sum's members means construction wrote one
        // that does not exist.
        self.builder.position_at_end(unknown);
        self.exhausted()?;

        for (tag, (member, (_, block))) in members.iter().zip(cases).enumerate() {
            let field = tag as u32 + 1;
            self.builder.position_at_end(block);
            let left = self
                .builder
                .build_extract_value(as_struct(left)?, field, "member")
                .map_err(|error| error.to_string())?;
            let right = self
                .builder
                .build_extract_value(as_struct(right)?, field, "member")
                .map_err(|error| error.to_string())?;
            let equal = self.equal(*member, left, right)?;
            let current = self
                .builder
                .get_insert_block()
                .expect("a comparison has an insertion block");
            self.builder
                .build_unconditional_branch(done)
                .map_err(|error| error.to_string())?;
            incoming.push((equal, current));
        }

        self.builder.position_at_end(done);
        self.phi_bool(&incoming)
    }

    pub(crate) fn phi_bool(
        &self,
        incoming: &[(IntValue<'ctx>, BasicBlock<'ctx>)],
    ) -> Result<IntValue<'ctx>, String> {
        let phi = self
            .builder
            .build_phi(self.context.bool_type(), "eq")
            .map_err(|error| error.to_string())?;
        let edges: Vec<(&dyn BasicValue<'ctx>, BasicBlock<'ctx>)> = incoming
            .iter()
            .map(|(value, block)| (value as &dyn BasicValue<'ctx>, *block))
            .collect();
        phi.add_incoming(&edges);
        Ok(phi.as_basic_value().into_int_value())
    }

    /// Lowers the shared control-flow operation used by expression- and
    /// statement-form `return_on_error`. The source and enclosing result sums
    /// may differ; the error payload is therefore re-tagged into the declared
    /// result before the checked cleanup plan runs.
    pub(crate) fn lower_return_on_error(
        &self,
        env: &mut Env<'ctx>,
        loops: &mut Loops<'ctx>,
        value_expr: &TExpr,
        sum: SumId,
        success: Ty,
        result: Ty,
        cleanup: &[TCleanup],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let value = self.expr(env, loops, value_expr)?;
        let aggregate = as_struct(value)?;
        let tag = self
            .builder
            .build_extract_value(aggregate, 0, "error_tag")
            .map_err(|error| error.to_string())?
            .into_int_value();
        let error_ty = self
            .program
            .types
            .iter()
            .position(|def| def.name() == "Error")
            .map(|index| Ty::User(TypeId(index as u32)))
            .ok_or_else(|| internal("the predeclared Error type is missing"))?;
        let error_tag = self.sum_member_tag(sum, error_ty)?;
        let function = self.current_function();
        let error_block = self.context.append_basic_block(function, "return_error");
        let success_block = self.context.append_basic_block(function, "return_success");
        let merge = self.context.append_basic_block(function, "return_ok");
        let is_error = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                tag,
                self.context
                    .i32_type()
                    .const_int(u64::from(error_tag), false),
                "is_error",
            )
            .map_err(|error| error.to_string())?;
        self.builder
            .build_conditional_branch(is_error, error_block, success_block)
            .map_err(|error| error.to_string())?;

        self.builder.position_at_end(error_block);
        let error_value = if result == Ty::Sum(sum) {
            value
        } else {
            let Ty::Sum(result_sum) = result else {
                return Err(internal("return_on_error result is not an inline sum"));
            };
            let payload = self
                .builder
                .build_extract_value(aggregate, error_tag + 1, "error_payload")
                .map_err(|error| error.to_string())?;
            let zeroed = self.struct_ty(result)?.const_zero();
            let result_tag = self.sum_member_tag(result_sum, error_ty)?;
            let tagged = self
                .builder
                .build_insert_value(
                    zeroed,
                    self.context
                        .i32_type()
                        .const_int(u64::from(result_tag), false),
                    0,
                    "error_result_tag",
                )
                .map_err(|error| error.to_string())?
                .into_struct_value();
            self.builder
                .build_insert_value(tagged, payload, result_tag + 1, "error_result_payload")
                .map_err(|error| error.to_string())?
                .into_struct_value()
                .into()
        };
        self.cleanup(
            env,
            loops,
            cleanup,
            Some(self.context.bool_type().const_int(1, false)),
        )?;
        self.builder
            .build_return(Some(&error_value))
            .map_err(|error| error.to_string())?;

        self.builder.position_at_end(success_block);
        let success_value = match success {
            Ty::Nil => self.context.i8_type().const_zero().into(),
            Ty::Sum(reduced_sum) => {
                let reduced_members = self.program.sums[reduced_sum.index()].clone();
                let done = self.context.append_basic_block(function, "return_ok_sum");
                let unknown = self
                    .context
                    .append_basic_block(function, "return_ok_unknown");
                let cases: Vec<_> = reduced_members
                    .iter()
                    .map(|member| {
                        (
                            self.context.i32_type().const_int(
                                u64::from(
                                    self.sum_member_tag(sum, *member)
                                        .expect("checker selected a sum member"),
                                ),
                                false,
                            ),
                            self.context
                                .append_basic_block(function, "return_ok_member"),
                        )
                    })
                    .collect();
                self.builder
                    .build_switch(tag, unknown, &cases)
                    .map_err(|error| error.to_string())?;
                self.builder.position_at_end(unknown);
                self.exhausted()?;
                let zeroed = self.struct_ty(Ty::Sum(reduced_sum))?.const_zero();
                let mut incoming = Vec::with_capacity(cases.len());
                for (member, (_, member_block)) in reduced_members.iter().zip(cases) {
                    self.builder.position_at_end(member_block);
                    let source_tag = self.sum_member_tag(sum, *member)?;
                    let payload = if *member == Ty::Nil {
                        self.context.i8_type().const_zero().into()
                    } else {
                        self.builder
                            .build_extract_value(aggregate, source_tag + 1, "success")
                            .map_err(|error| error.to_string())?
                    };
                    let reduced_tag = self.sum_member_tag(reduced_sum, *member)?;
                    let tagged = self
                        .builder
                        .build_insert_value(
                            zeroed,
                            self.context
                                .i32_type()
                                .const_int(u64::from(reduced_tag), false),
                            0,
                            "success_tag",
                        )
                        .map_err(|error| error.to_string())?
                        .into_struct_value();
                    let injected = self
                        .builder
                        .build_insert_value(tagged, payload, reduced_tag + 1, "success_payload")
                        .map_err(|error| error.to_string())?;
                    let current = self
                        .builder
                        .get_insert_block()
                        .ok_or_else(|| internal("return_on_error sum arm has no block"))?;
                    self.builder
                        .build_unconditional_branch(done)
                        .map_err(|error| error.to_string())?;
                    incoming.push((injected.into_struct_value(), current));
                }
                self.builder.position_at_end(done);
                let phi = self
                    .builder
                    .build_phi(self.struct_ty(Ty::Sum(reduced_sum))?, "success_sum")
                    .map_err(|error| error.to_string())?;
                let incoming: Vec<(&dyn BasicValue<'ctx>, BasicBlock<'ctx>)> = incoming
                    .iter()
                    .map(|(value, block)| (value as &dyn BasicValue<'ctx>, *block))
                    .collect();
                phi.add_incoming(&incoming);
                phi.as_basic_value()
            }
            _ => self
                .builder
                .build_extract_value(
                    aggregate,
                    self.sum_member_tag(sum, success)? + 1,
                    "success_value",
                )
                .map_err(|error| error.to_string())?,
        };
        self.builder
            .build_unconditional_branch(merge)
            .map_err(|error| error.to_string())?;
        let success_end = self
            .builder
            .get_insert_block()
            .ok_or_else(|| internal("return_on_error success block has no end"))?;
        self.builder.position_at_end(merge);
        let phi = self
            .builder
            .build_phi(self.ty(success), "success_value")
            .map_err(|error| error.to_string())?;
        phi.add_incoming(&[(&success_value, success_end)]);
        Ok(phi.as_basic_value())
    }
}
