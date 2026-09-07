//! Statements, blocks, and control flow (Specification 026 sections 5-8).

use super::*;

/// Checks both forms of `return_on_error`. The statement form is selected by
/// the block checker, not by a second parser production, so the operand is
/// still checked exactly once and retains the ordinary move rules.
pub(crate) fn check_return_on_error<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    value: &Spanned<Expr<'src>>,
    span: Span,
    statement_form: bool,
) -> CheckedReturnOnError {
    let (value, value_ty) = check_expr(ctx, env, value);
    let fallback = || {
        if statement_form {
            CheckedReturnOnError::Statement(TStmt::Expr(TExpr::Nil))
        } else {
            CheckedReturnOnError::Expr {
                value: TExpr::Nil,
                ty: Ty::Nil,
            }
        }
    };
    let Some(error_ty) = ctx.types.top_level("Error").map(Ty::User) else {
        ctx.error(span, "the predeclared 'Error' type is unavailable".into());
        return fallback();
    };
    let Ty::Sum(sum) = value_ty else {
        ctx.error(
            span,
            "'return_on_error' requires an inline sum containing exact 'Error'".into(),
        );
        return fallback();
    };
    let members = ctx.types.sum_members(sum);
    if !members.contains(&error_ty) {
        ctx.error(
            span,
            "'return_on_error' requires an inline sum containing exact 'Error'".into(),
        );
        return fallback();
    }
    let successes: Vec<Ty> = members
        .iter()
        .copied()
        .filter(|member| *member != error_ty)
        .collect();
    let Some(Some(result)) = ctx.callable_result else {
        ctx.error(
            span,
            "'return_on_error' is only valid inside a fallible callable".into(),
        );
        return fallback();
    };
    let Ty::Sum(result_sum) = result else {
        ctx.error(
            span,
            "'return_on_error' requires a fallible inline-sum result".into(),
        );
        return fallback();
    };
    if !ctx.types.sum_members(result_sum).contains(&error_ty) {
        ctx.error(
            span,
            "'return_on_error' requires an enclosing result containing exact 'Error'".into(),
        );
        return fallback();
    }
    if statement_form {
        if successes.as_slice() != [Ty::Nil] {
            ctx.error(
                span,
                "'return_on_error' statement form requires an operand of exactly 'Nil | Error'"
                    .into(),
            );
            return fallback();
        }
        let value = mark_consumed(ctx, env, value, span);
        let cleanup = cleanup_for_exit(ctx, env, 0, Some(true));
        return CheckedReturnOnError::Statement(TStmt::ReturnOnError {
            value,
            sum,
            result,
            cleanup,
        });
    }
    if !successes.iter().any(|member| *member != Ty::Nil) {
        ctx.error(
            span,
            "'return_on_error' requires at least one non-Nil success member".into(),
        );
        return fallback();
    }
    let success = if let [success] = successes.as_slice() {
        *success
    } else {
        let mut reduced = successes;
        reduced.sort();
        Ty::Sum(ctx.types.intern_sum(reduced))
    };
    let value = mark_consumed(ctx, env, value, span);
    let cleanup = cleanup_for_exit(ctx, env, 0, Some(true));
    CheckedReturnOnError::Expr {
        value: TExpr::ReturnOnError {
            value: Box::new(value),
            sum,
            success,
            result,
            cleanup,
        },
        ty: success,
    }
}

/// The first reachable block element after an unconditional callable return
/// or a conditional whose every reachable branch returns (Specification 026
/// section 7).
const UNREACHABLE_AFTER_RETURN: &str = "this code is unreachable: every path that reaches it has already returned from this \
     function or method";

/// Checks a block, threading Specification 026 section 6's flow-outcome
/// classification through statement, `return`, and `if` handling.
/// `expected` is `Some(ty)` for a value-required block (a function/method
/// body that declares a result, or a value-form `if` branch) and `None` for
/// a no-result block; every element but the last is always an ordinary
/// statement. The last element may satisfy the block's value requirement --
/// when `expected.is_some()` -- with a trailing expression, a value-form
/// `if`, or (new in this specification) by executing `return` on every
/// reachable path, which needs no value of its own (section 6). Returns the
/// checked block and whether every reachable path through it executes an
/// explicit `return`: the "callable return" outcome, which a caller excludes
/// from its own move-state and reachability merges (section 8) exactly as it
/// would an unreachable predecessor.
pub(crate) fn check_block<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    block: &Block<'src>,
    expected: Option<Ty>,
) -> (TBlock, bool) {
    let scope = env.len();
    let borrow_scope = ctx.cleanup_scopes.len();
    ctx.cleanup_scopes.push(Vec::new());
    let mut statements = Vec::new();
    let mut result = None;
    let mut returns = false;
    let mut reported_unreachable = false;
    let last = block.elements.len().wrapping_sub(1);
    for (index, element) in block.elements.iter().enumerate() {
        prune_view_borrows(ctx, &block.elements[index..], borrow_scope);
        // Specification 026 section 7: only the first unreachable element is
        // reported; flow bookkeeping below still runs for dead code (using
        // `|=` so a later, non-returning nested construct can never make an
        // already-unreachable point look reachable again), so dead code is
        // still fully type-checked for unrelated errors.
        let already_unreachable = returns;
        if already_unreachable && !reported_unreachable {
            ctx.error(element.1, UNREACHABLE_AFTER_RETURN.into());
            reported_unreachable = true;
        }
        let is_last = index == last;
        if let BlockElement::Let { .. } = &element.0
            && !(is_last && !already_unreachable && expected.is_some())
        {
            let statement = check_stmt(ctx, env, element);
            if let TStmt::Let { name, ty, .. } = &statement
                && ctx.types.is_move_only(*ty)
            {
                ctx.cleanup_scopes
                    .last_mut()
                    .expect("a checked block owns a cleanup scope")
                    .push(TCleanup::Drop(Place {
                        root: PlaceRoot::Local(name.clone()),
                        root_ty: *ty,
                        path: Vec::new(),
                        ty: *ty,
                    }));
            }
            statements.push(statement);
            continue;
        }
        match &element.0 {
            BlockElement::Defer {
                on_error,
                call,
                span,
            } => {
                if is_last && !already_unreachable && expected.is_some() {
                    let name = ctx.name(expected.expect("guarded by expected.is_some() above"));
                    ctx.error(
                        element.1,
                        format!(
                            "this block must end in an expression of type '{name}', but it ends in a deferred call"
                        ),
                    );
                }
                if let Some(deferred) = check_defer(ctx, env, *on_error, call, *span) {
                    ctx.cleanup_scopes
                        .last_mut()
                        .expect("a checked block owns a cleanup scope")
                        .push(TCleanup::Deferred(Rc::new(deferred)));
                }
            }
            BlockElement::Return(value, span) => {
                statements.push(check_return(ctx, env, value, *span));
                returns = true;
            }
            BlockElement::If(form) => {
                // Only the block's last element may supply its required
                // value, and only when nothing above already returned.
                let value_slot = if is_last && !already_unreachable {
                    expected
                } else {
                    None
                };
                let (checked, if_returns) = check_if(ctx, env, form, value_slot);
                match checked {
                    FlowIf::Value(value) => result = Some(value),
                    FlowIf::Stmt(stmt_if) => statements.push(TStmt::If(stmt_if)),
                }
                returns |= if_returns;
            }
            BlockElement::Expr(expression)
                if is_last && !already_unreachable && expected.is_some() =>
            {
                let expected_ty = expected.expect("guarded by expected.is_some() above");
                let (value, ty) = check_expr(ctx, env, expression);
                // Specification 016 section 6.1: a value block's trailing
                // expression is always a function/method result, or feeds one
                // of the other four consuming contexts one level further out
                // (Specification 016 section 6.2's `if`-arm example).
                let value = mark_consumed(ctx, env, value, expression.1);
                result = Some(if matches!(value, TExpr::ReturnOnError { .. }) {
                    coerce_return_success(ctx, value, ty, expected_ty, expression.1)
                } else {
                    coerce(ctx, value, ty, expected_ty, expression.1)
                });
            }
            _ if is_last && !already_unreachable && expected.is_some() => {
                let name = ctx.name(expected.expect("guarded by expected.is_some() above"));
                ctx.error(
                    element.1,
                    format!(
                        "this block must end in an expression of type '{name}', \
                         but it ends in a statement"
                    ),
                );
                statements.push(check_stmt(ctx, env, element));
            }
            _ => {
                statements.push(check_stmt(ctx, env, element));
            }
        }
    }
    if !returns
        && result.is_none()
        && block.elements.is_empty()
        && let Some(expected_ty) = expected
    {
        let name = ctx.name(expected_ty);
        ctx.error(
            block.span,
            format!("this block must end in an expression of type '{name}', but it is empty"),
        );
    }
    // Specification 026 section 8: a block that always returns already
    // attached its own cleanup to each `TStmt::Return` (`check_return`), so
    // it never falls off its own end; a normal-completion drop list here
    // would simply never run and would risk destroying a value the return
    // already transferred to the caller.
    let cleanup = ctx
        .cleanup_scopes
        .pop()
        .expect("a checked block owns a cleanup scope");
    let cleanup = if returns {
        Vec::new()
    } else {
        let error_exit = result
            .as_ref()
            .and_then(|value| expression_error_exit(ctx, value));
        cleanup_for_exit_from_entries(ctx, cleanup, error_exit)
    };
    ctx.view_borrows
        .retain(|borrow| borrow.scope < borrow_scope);
    env.truncate(scope);
    (
        TBlock {
            statements,
            result,
            result_ty: expected,
            cleanup,
        },
        returns,
    )
}

/// Checks one arm's condition, leaving any type-test binding visible in `env`
/// for the arm body only. The caller restores `env` after the body.
/// Specification 018 section 6 extends the tested place from a named union to
/// an inline sum, so the place is resolved once here and then dispatched to
/// whichever member-lookup rules its type requires.
pub(crate) fn check_arm_condition<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    condition: &Condition<'src>,
) -> TCondition {
    match condition {
        Condition::Expr(expression) => TCondition::Expr(check_condition(ctx, env, expression)),
        Condition::TypeTest(test) => {
            let Some(resolved) = resolve_place(ctx, env, &test.place) else {
                return TCondition::Expr(TExpr::Bool(false));
            };
            let Resolved { mut place, mutable } = resolved;
            // Specification 016 section 4.3: `tree is Tree.Branch(branch)`
            // tests the union stored *through* a `Box<Tree>` subject exactly
            // as it would a bare `Tree`, so the subject is dereferenced
            // before checking whether it is a union or an inline sum.
            place.ty = deref_box(&ctx.types, place.ty);
            match place.ty {
                Ty::User(id) if ctx.types.union_members(id).is_some() => {
                    match check_type_test(ctx, test, place, id) {
                        Some(checked) => {
                            // Specification 016 section 7.3: the binding
                            // shares the tested place's root mutability.
                            bind_type_test(
                                env,
                                test.binding,
                                &checked.binding,
                                mutable,
                                ctx.cleanup_scopes.len(),
                            );
                            TCondition::Test(checked)
                        }
                        None => TCondition::Expr(TExpr::Bool(false)),
                    }
                }
                Ty::Sum(sum) => match check_sum_type_test(ctx, test, place, sum) {
                    Some(checked) => {
                        bind_type_test(
                            env,
                            test.binding,
                            &checked.binding,
                            mutable,
                            ctx.cleanup_scopes.len(),
                        );
                        TCondition::SumTest(checked)
                    }
                    None => TCondition::Expr(TExpr::Bool(false)),
                },
                other => {
                    let name = ctx.name(other);
                    ctx.error(
                        test.place.span,
                        format!(
                            "the left side of 'is' must have a union type or an inline sum \
                             type, but '{}' has type '{name}'",
                            test.place
                        ),
                    );
                    TCondition::Expr(TExpr::Bool(false))
                }
            }
        }
    }
}

/// Pushes a proven type-test binding into scope for the arm body, if the
/// syntactic test carried one. Specification 012 section 7 originally made
/// every such binding immutable; Specification 016 section 7.3 generalizes
/// that to "mutable exactly when the tested place's root is," which reduces
/// to the old rule whenever the tested root itself is immutable.
pub(crate) fn bind_type_test<'src>(
    env: &mut Env<'src>,
    written: Option<Spanned<&'src str>>,
    checked: &Option<(String, Ty)>,
    mutable: bool,
    scope: usize,
) {
    if let (Some((name, _)), Some((_, ty))) = (written, checked) {
        env.push(Binding {
            name,
            ty: *ty,
            mutable,
            scope,
            // Specification 016 section 7.3: the binding is a branch-scoped
            // alias to the tested place's active payload, never an
            // independent owning root, so `mark_consumed` must reject
            // consuming it whole exactly as it already rejects consuming one
            // of its fields (Specification 016 section 6.4).
            type_test_alias: true,
        });
    }
}

pub(crate) fn check_condition<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    condition: &Spanned<Expr<'src>>,
) -> TExpr {
    let (value, ty) = check_expr(ctx, env, condition);
    if ty == Ty::Nil {
        ctx.error(
            condition.1,
            "a standalone 'nil' cannot be used as a condition; give it a sum type".into(),
        );
    }
    TExpr::Truthiness(Box::new(value), ty)
}

/// What an `if`/`elseif` chain proves about member coverage.
pub(crate) struct ChainFact {
    /// Every arm is a type test over one syntactic place, and every direct
    /// member of that place's union or inline sum is tested exactly once.
    exhaustive: bool,
    /// The qualified names of members a same-place chain fails to handle.
    missing: Vec<String>,
}

/// A named-union member is itself a user-defined type, so wrapping its
/// `TypeId` as `Ty::User` lets a union test and a sum test share one
/// "which member did this arm test" representation below.
pub(crate) fn tested_member(condition: &TCondition) -> Option<(&Place, Ty)> {
    match condition {
        TCondition::Test(test) => Some((&test.place, Ty::User(test.member))),
        TCondition::SumTest(test) => Some((&test.place, test.member)),
        TCondition::Expr(_) => None,
    }
}

/// Specification 010 section 12.4, extended by Specification 018 section 6 to
/// an inline sum. Proves -- never guesses -- whether a chain covers every
/// member, and reports unreachable duplicate branches.
pub(crate) fn analyze_chain(
    ctx: &mut Ctx<'_>,
    arms: &[(TCondition, TBlock)],
    spans: &[Span],
) -> ChainFact {
    let mut subject: Option<Place> = None;
    let mut tested: Vec<Ty> = Vec::new();
    let mut chain = true;
    for ((condition, _), span) in arms.iter().zip(spans) {
        let Some((place, member)) = tested_member(condition) else {
            chain = false;
            break;
        };
        match &subject {
            None => subject = Some(place.clone()),
            Some(first) if first == place => {}
            Some(_) => {
                chain = false;
                break;
            }
        }
        if tested.contains(&member) {
            let name = ctx.name(member);
            ctx.error(
                *span,
                format!(
                    "'{name}' is already handled by an earlier branch, so this branch \
                     is unreachable"
                ),
            );
        } else {
            tested.push(member);
        }
    }
    let covered = chain
        .then_some(subject.as_ref())
        .flatten()
        .and_then(|place| match place.ty {
            Ty::User(id) => ctx.types.union_members(id).map(|members| {
                members
                    .iter()
                    .map(|member| Ty::User(*member))
                    .collect::<Vec<Ty>>()
            }),
            Ty::Sum(id) => Some(ctx.types.sum_members(id).to_vec()),
            _ => None,
        });
    let Some(members) = covered else {
        return ChainFact {
            exhaustive: false,
            missing: Vec::new(),
        };
    };
    let missing: Vec<String> = members
        .iter()
        .filter(|member| !tested.contains(member))
        .map(|member| ctx.name(*member))
        .collect();
    ChainFact {
        exhaustive: missing.is_empty() && !members.is_empty(),
        missing,
    }
}

/// What one checked `if` form produces, decided purely by whether a value
/// was requested at its position and whether it turned out to return on
/// every reachable path (Specification 026 section 6).
pub(crate) enum FlowIf {
    Value(TExpr),
    Stmt(TStmtIf),
}

/// Checks an `if` form as a block element and classifies its Specification
/// 026 section 6 flow outcome. `expected` is `Some(ty)` only when this `if`
/// occupies the one position that may supply a block's required value (the
/// last element of a value-required block, per `check_block`); every other
/// call site passes `None`, exactly like the pre-026 statement-form rule
/// that a mid-block `if` never produces a value. Each arm and `else` is
/// itself checked through `check_block` with that same `expected`, so a
/// nested `return` composes for free: an arm that returns supplies no value
/// of its own (`TBlock::result` stays `None`) and is excluded from this
/// `if`'s own move-state exits, since it never reaches the code after the
/// `if` (section 8). Returns whether every reachable arm -- and `else`, or
/// an exhaustive type-test chain standing in for one -- returns; a caller
/// uses this to keep tracking reachability and move availability past the
/// `if` itself.
pub(crate) fn check_if<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    form: &IfForm<'src>,
    expected: Option<Ty>,
) -> (FlowIf, bool) {
    // Specification 016 section 6.2: every arm branches from the same entry
    // state, so each is checked against a fresh snapshot rather than the
    // previous arm's leftover moves.
    let entry = ctx.move_state.clone();
    let entry_views = ctx.view_borrows.clone();
    let mut arms = Vec::new();
    let mut spans = Vec::new();
    let mut exits = Vec::new();
    let mut view_exits = Vec::new();
    let mut arm_returns = Vec::new();
    for (condition, body) in &form.arms {
        ctx.move_state = entry.clone();
        ctx.view_borrows = entry_views.clone();
        let scope = env.len();
        let checked_condition = check_arm_condition(ctx, env, condition);
        let (checked_body, returns) = check_block(ctx, env, body, expected);
        env.truncate(scope);
        spans.push(condition.span());
        // Specification 026 section 8: a branch that unconditionally returns
        // never reaches the point after the `if`, so its exit move state is
        // not a real predecessor there and must not be merged into it.
        if !returns {
            exits.push(ctx.move_state.clone());
            view_exits.push(ctx.view_borrows.clone());
        }
        arm_returns.push(returns);
        arms.push((checked_condition, checked_body));
    }
    let fact = analyze_chain(ctx, &arms, &spans);
    let mut else_returns = true;
    let else_branch = match &form.else_branch {
        Some(body) => {
            if fact.exhaustive {
                ctx.error(
                    form.span,
                    "this type-test chain already covers every direct member, so the \
                     'else' branch is unreachable"
                        .into(),
                );
            }
            ctx.move_state = entry.clone();
            ctx.view_borrows = entry_views.clone();
            let (checked, returns) = check_block(ctx, env, body, expected);
            if !returns {
                exits.push(ctx.move_state.clone());
                view_exits.push(ctx.view_borrows.clone());
            }
            else_returns = returns;
            Some(checked)
        }
        None if fact.exhaustive => None,
        None => {
            if let Some(expected_ty) = expected {
                let name = ctx.name(expected_ty);
                let msg = if fact.missing.is_empty() {
                    format!(
                        "an 'if' that produces a value of type '{name}' requires an 'else' \
                         branch"
                    )
                } else {
                    format!(
                        "this type-test chain produces a value of type '{name}' without an \
                         'else' branch, but does not handle {}",
                        fact.missing.join(", ")
                    )
                };
                ctx.error(form.span, msg);
            }
            None
        }
    };
    let covers_every_path = else_branch.is_some() || fact.exhaustive;
    // An exhaustive type-test chain or an explicit `else` covers every path,
    // so no execution reaches the point after the `if` without entering one
    // of the recorded arms above. Otherwise the fall-through edge is
    // reachable too, carrying the entry state forward unchanged
    // (Specification 016 section 6.2's merge rule) -- when a value is
    // required (`expected.is_some()`) and coverage is missing, the branch
    // above already diagnosed it, but the merge still must not silently drop
    // moves for the rest of a malformed program.
    if !covers_every_path {
        exits.push(entry.clone());
        view_exits.push(entry_views.clone());
    }
    let if_returns =
        covers_every_path && arm_returns.iter().all(|returns| *returns) && else_returns;
    ctx.move_state = if exits.is_empty() {
        entry
    } else {
        merge_moves(exits)
    };
    ctx.view_borrows = if view_exits.is_empty() {
        entry_views
    } else {
        merge_view_borrows(view_exits)
    };
    match expected {
        // Specification 026 section 6: every reachable branch returning
        // makes the `if` itself a callable return that supplies no value, so
        // its checked representation is a statement even in a value-needed
        // position.
        Some(ty) if !if_returns => (
            FlowIf::Value(TExpr::If(Box::new(TValueIf {
                arms,
                else_branch,
                exhaustive: fact.exhaustive,
                ty,
            }))),
            if_returns,
        ),
        _ => (
            FlowIf::Stmt(TStmtIf {
                arms,
                else_branch,
                exhaustive: fact.exhaustive,
            }),
            if_returns,
        ),
    }
}

pub(crate) fn check_defer<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    on_error: bool,
    call: &Spanned<Expr<'src>>,
    span: Span,
) -> Option<TDeferred> {
    let Expr::Call(callee, arguments) = &call.0 else {
        ctx.error(
            span,
            "a deferred action must be a direct function or method call".into(),
        );
        return None;
    };
    // A deferred call is checked now but executes only at scope exit. Preserve
    // the ordinary checked argument tree while restoring move availability so
    // a by-value argument is not consumed when the defer is merely armed.
    let move_state = ctx.move_state.clone();
    let Some(checked) = check_call(ctx, env, span, callee, arguments) else {
        ctx.move_state = move_state;
        return None;
    };
    // Checking the stored call exposes every root its eventual evaluation may
    // consume, including roots consumed inside nested argument calls. The
    // defer declaration only arms that evaluation, so restore availability
    // after recording the delta for exit-time cleanup validation.
    let consumes = ctx
        .move_state
        .keys()
        .filter(|root| !move_state.contains_key(*root))
        .cloned()
        .collect();
    ctx.move_state = move_state;
    let statement = match checked {
        CheckedCall::Function {
            name,
            args,
            result: None,
        } => TStmt::Call(name, args),
        CheckedCall::Method { call, result: None } => TStmt::MethodCall(call),
        CheckedCall::Statement(_) => {
            ctx.error(
                span,
                "a deferred action must be a direct function or method call".into(),
            );
            return None;
        }
        CheckedCall::Function {
            result: Some(_), ..
        }
        | CheckedCall::Method {
            result: Some(_), ..
        }
        | CheckedCall::Value(_, _) => {
            ctx.error(span, "a deferred call must not produce a value".into());
            return None;
        }
    };
    if on_error {
        let valid = matches!(ctx.callable_result, Some(Some(Ty::Sum(sum)))
        if ctx.types.sum_members(sum).iter().any(|ty| {
            matches!(ty, Ty::User(id) if ctx.types.def(*id).name() == "Error")
        }));
        if !valid {
            ctx.error(
                span,
                "'defer_on_error' is only valid in a callable whose result contains Error".into(),
            );
            return None;
        }
    }
    let dependencies = env
        .iter()
        .filter(|binding| expr_mentions_local(&call.0, binding.name))
        .map(|binding| PlaceRoot::Local(binding.name.to_string()))
        .collect();
    Some(TDeferred {
        on_error,
        call: statement,
        span,
        dependencies,
        consumes,
    })
}

pub(crate) fn check_stmt<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    element: &Spanned<BlockElement<'src>>,
) -> TStmt {
    match &element.0 {
        BlockElement::Let {
            mutable,
            name,
            name_span,
            ty,
            value,
        } => {
            let declared = resolve_type(ctx, ty);
            // The initializer is checked before the name is in scope, so it can
            // never refer to the variable being created.
            let (checked, value_ty) = if matches!(&value.0, Expr::List(_)) {
                check_collection_literal(ctx, env, value, declared)
            } else {
                check_expr(ctx, env, value)
            };
            // Specification 016 section 6.1: initialization is a consuming
            // context.
            let checked = mark_consumed(ctx, env, checked, value.1);
            let checked = coerce(ctx, checked, value_ty, declared, value.1);
            declare(ctx, name, *name_span, "Variable");
            env.push(Binding {
                name,
                ty: declared,
                mutable: *mutable,
                scope: ctx.cleanup_scopes.len().saturating_sub(1),
                type_test_alias: false,
            });
            if is_borrowed_type(ctx, declared) {
                let sources = view_sources(ctx, &checked);
                if sources.is_empty() {
                    ctx.error(
                        value.1,
                        "a stored view requires a named owning source; this temporary would not live long enough"
                            .into(),
                    );
                } else {
                    ctx.view_borrows.push(ViewBorrow {
                        view_name: (*name).to_string(),
                        sources,
                        scope: ctx.cleanup_scopes.len().saturating_sub(1),
                        persistent: false,
                    });
                }
            }
            TStmt::Let {
                mutable: *mutable,
                name: (*name).to_string(),
                ty: declared,
                value: checked,
            }
        }
        BlockElement::IndexedAssign { target, value } => {
            let Expr::Index(base, index) = &target.0 else {
                ctx.error(
                    target.1,
                    "only a map index can be an indexed assignment target".into(),
                );
                return TStmt::Expr(TExpr::Nil);
            };
            let Some(resolved) = (match as_place(ctx, env, base) {
                PlaceOutcome::Resolved(resolved) => Some(resolved),
                PlaceOutcome::Reported | PlaceOutcome::NotAPlace => None,
            }) else {
                return TStmt::Expr(TExpr::Nil);
            };
            if !resolved.mutable {
                ctx.error(
                    base.1,
                    format!(
                        "'{}' is not declared 'mut' and cannot be indexed-assigned",
                        ctx.place_name(&resolved.place)
                    ),
                );
            }
            let collection_ty = resolved.place.ty;
            reject_live_view_source(ctx, &resolved.place.root, target.1);
            if matches!(collection_ty, Ty::Array(_) | Ty::List(_)) {
                let elem = match collection_ty {
                    Ty::Array(id) | Ty::List(id) => match ctx.types.collection(id) {
                        CollectionDef::Array { elem, .. } | CollectionDef::List { elem } => *elem,
                        _ => unreachable!("sequence type has non-sequence metadata"),
                    },
                    _ => unreachable!("guarded by sequence collection type"),
                };
                let (checked_index, index_ty) = check_expr(ctx, env, index);
                if index_ty != Ty::Int64 {
                    ctx.mismatch(index.1, Ty::Int64, index_ty);
                }
                let (checked_value, value_ty) = check_expr(ctx, env, value);
                let checked_value = mark_consumed(ctx, env, checked_value, value.1);
                let checked_value = coerce(ctx, checked_value, value_ty, elem, value.1);
                return TStmt::SequenceIndexAssign {
                    receiver: resolved.place,
                    index: checked_index,
                    value: checked_value,
                    elem,
                };
            }
            let Ty::Map(id) = collection_ty else {
                ctx.error(
                    base.1,
                    format!(
                        "'{}' is not a map, so it cannot be indexed-assigned",
                        ctx.name(collection_ty)
                    ),
                );
                return TStmt::Expr(TExpr::Nil);
            };
            let (map_key_ty, map_value_ty) = match ctx.types.collection(id) {
                CollectionDef::Map { key, value } => (*key, *value),
                _ => unreachable!("map type has non-map metadata"),
            };
            let (checked_index, index_ty) = check_expr(ctx, env, index);
            let (checked_key, key_ty) = if map_key_ty == Ty::String {
                (
                    coerce(ctx, checked_index, index_ty, Ty::ViewByte, index.1),
                    Ty::ViewByte,
                )
            } else {
                if index_ty != map_key_ty {
                    ctx.mismatch(index.1, map_key_ty, index_ty);
                }
                (checked_index, index_ty)
            };
            let (checked_value, value_ty) = check_expr(ctx, env, value);
            let checked_value = mark_consumed(ctx, env, checked_value, value.1);
            let checked_value = coerce(ctx, checked_value, value_ty, map_value_ty, value.1);
            TStmt::Expr(TExpr::MapInsert {
                receiver: resolved.place,
                key: Box::new(checked_key),
                value: Box::new(checked_value),
                key_ty,
                value_ty: map_value_ty,
                require_existing: true,
            })
        }
        BlockElement::Assign { place, value } => {
            let target = resolve_place(ctx, env, place);
            if let Some(target) = &target {
                reject_live_view_source(ctx, &target.place.root, value.1);
            }
            let (checked, value_ty) = check_expr(ctx, env, value);
            // Specification 016 section 6.1: an assignment's right operand is
            // a consuming context.
            let checked = mark_consumed(ctx, env, checked, value.1);
            match target {
                Some(resolved) => {
                    if !resolved.mutable {
                        ctx.error(
                            place.root_span,
                            format!(
                                "'{}' is not declared 'mut' and cannot be assigned",
                                place.root
                            ),
                        );
                    }
                    // Specification 016 section 6.3: a move whose source
                    // overlaps its destination is rejected, including
                    // `value = value` and a projection assigned from its own
                    // owning root (e.g. `container.field = container`). A
                    // source with its own non-empty path is handled instead
                    // by `mark_consumed`'s subplace-move rejection above, so
                    // only a whole-root source is checked here.
                    if ctx.types.is_move_only(resolved.place.ty)
                        && let TExpr::Place(source, UseMode::Consume) = &checked
                        && source.path.is_empty()
                        && overlaps(&resolved.place, source)
                    {
                        let dest = ctx.place_name(&resolved.place);
                        let source_name = ctx.place_name(source);
                        ctx.error(
                            value.1,
                            format!(
                                "'{source_name}' and '{dest}' overlap, so this assignment \
                                 cannot destroy '{dest}' before '{source_name}' finishes moving \
                                 into it"
                            ),
                        );
                    }
                    let checked = coerce(ctx, checked, value_ty, resolved.place.ty, value.1);
                    if resolved.place.path.is_empty() && is_borrowed_type(ctx, resolved.place.ty) {
                        let root_name = resolved.place.root.to_string();
                        let sources = view_sources(ctx, &checked);
                        let binding_scope = env
                            .iter()
                            .rev()
                            .find(|binding| binding.name == root_name)
                            .map_or_else(
                                || ctx.cleanup_scopes.len().saturating_sub(1),
                                |binding| binding.scope,
                            );
                        ctx.view_borrows
                            .retain(|borrow| borrow.view_name != root_name);
                        if sources.is_empty() {
                            ctx.error(
                                value.1,
                                "a stored view requires a named owning source; this temporary would not live long enough"
                                    .into(),
                            );
                        } else {
                            ctx.view_borrows.push(ViewBorrow {
                                view_name: root_name,
                                sources,
                                scope: binding_scope,
                                persistent: false,
                            });
                        }
                    }
                    if resolved.place.root == PlaceRoot::SelfRef
                        && let Some(method) = ctx.current_method
                    {
                        ctx.direct_writes[method.index()] = true;
                    }
                    // Specification 016 section 6.3: the old destination is
                    // destroyed before the new value is installed, unless
                    // there is nothing live there to destroy -- either the
                    // destination is copyable, or it is a whole root that is
                    // currently moved (reinitializing a moved mutable local,
                    // the closing sentence below, installs a value where
                    // none was live). A field destination (non-empty path)
                    // always has a live value: the checker requires every
                    // field of a constructed aggregate to be initialized, so
                    // there is no partially-built aggregate a field
                    // assignment could be reaching into.
                    let drop_before = ctx.types.is_move_only(resolved.place.ty)
                        && !(resolved.place.path.is_empty()
                            && ctx.move_state.contains_key(&resolved.place.root));
                    // Specification 016 section 6.3's closing sentence:
                    // assigning the whole root installs a fresh value, so it
                    // is available again regardless of whether it was moved.
                    // A field assignment (a non-empty path) reinitializes no
                    // root and is left untouched.
                    if resolved.place.path.is_empty() {
                        ctx.move_state.remove(&resolved.place.root);
                    }
                    TStmt::Assign {
                        place: resolved.place,
                        value: checked,
                        drop_before,
                    }
                }
                None => TStmt::Expr(checked),
            }
        }
        BlockElement::While {
            condition, body, ..
        } => {
            let condition = check_condition(ctx, env, condition);
            // Specification 016 section 6.2: a `while` body is checked to a
            // fixed point. The body's own single checked pass (below) already
            // uses the pre-loop state, exactly as its first real iteration
            // would; what a naive single pass cannot see is a later
            // iteration reusing the same move once the first has already
            // consumed it. Since this checker's control flow never branches
            // on move-availability, a root's exit state as a function of its
            // entry state is always one of "unconditionally reinitialized",
            // "unconditionally moved", or "untouched" -- never a function
            // that depends on which one held going in -- so comparing this
            // one real pass's entry and exit finds every root the loop can
            // legitimately double-move without needing to re-run the body.
            let pre = ctx.move_state.clone();
            let pre_views = ctx.view_borrows.clone();
            ctx.loops.push(ctx.cleanup_scopes.len());
            let (body, body_returns) = check_block(ctx, env, body, None);
            ctx.loops.pop();
            let post = ctx.move_state.clone();
            let post_views = ctx.view_borrows.clone();
            for (root, move_span) in &post {
                if !pre.contains_key(root) {
                    ctx.error(
                        *move_span,
                        format!(
                            "'{root}' is moved here, but this is inside a 'while' body, so a \
                             later iteration would find '{root}' already moved"
                        ),
                    );
                }
            }
            // The loop may run zero or more times, so the state after it must
            // hold on both the zero-iteration edge (`pre`) and the edge that
            // runs the body at least once (`post`); by the reasoning above,
            // `post` already reflects every later iteration too. When the
            // body always returns (Specification 026), it never falls
            // through to re-check the condition, so `post` is not a real
            // predecessor of "after the loop" -- only the zero-iteration edge
            // is (section 6's "no proof of nontermination" also applies in
            // reverse: a loop that always returns when entered still might
            // never be entered).
            ctx.move_state = if body_returns {
                pre
            } else {
                merge_moves(vec![pre, post])
            };
            ctx.view_borrows = if body_returns {
                pre_views
            } else {
                merge_view_borrows(vec![pre_views, post_views])
            };
            TStmt::While { condition, body }
        }
        BlockElement::For {
            value,
            key,
            iterable,
            body,
            span,
        } => {
            let (checked_iterable, iterable_ty) = check_expr(ctx, env, iterable);
            let (value_ty, key_ty) = match iterable_ty {
                Ty::Array(id) | Ty::List(id) | Ty::View(id) => match ctx.types.collection(id) {
                    CollectionDef::Array { elem, .. }
                    | CollectionDef::List { elem }
                    | CollectionDef::View { elem } => (*elem, None),
                    _ => unreachable!("sequence iterable has non-sequence metadata"),
                },
                Ty::ViewByte => (Ty::Byte, None),
                Ty::ViewUnicode => (Ty::Unicode, None),
                Ty::Map(id) => match ctx.types.collection(id) {
                    CollectionDef::Map { key, value } => (*value, Some(*key)),
                    _ => unreachable!("map iterable has non-map metadata"),
                },
                Ty::Set(id) => match ctx.types.collection(id) {
                    CollectionDef::Set { elem } => (*elem, None),
                    _ => unreachable!("set iterable has non-set metadata"),
                },
                other => {
                    ctx.error(*span, format!("'{}' is not iterable", ctx.name(other)));
                    (Ty::Nil, None)
                }
            };
            if key.is_some() != key_ty.is_some() {
                let expected = if key_ty.is_some() {
                    "a key and value binding"
                } else {
                    "one value binding"
                };
                ctx.error(*span, format!("this iterable requires {expected}"));
            }
            if key_ty.is_some() {
                if !matches!(
                    key_ty,
                    Some(
                        Ty::Byte
                            | Ty::UInt16
                            | Ty::UInt32
                            | Ty::UInt64
                            | Ty::Int64
                            | Ty::Bool
                            | Ty::Unicode
                            | Ty::String
                    )
                ) {
                    ctx.error(
                        *span,
                        "map iteration requires a supported scalar or String key".into(),
                    );
                }
            } else if matches!(iterable_ty, Ty::Set(_))
                && !matches!(
                    value_ty,
                    Ty::Byte
                        | Ty::UInt16
                        | Ty::UInt32
                        | Ty::UInt64
                        | Ty::Int64
                        | Ty::Bool
                        | Ty::Unicode
                        | Ty::String
                )
            {
                ctx.error(
                    *span,
                    "set iteration currently supports scalar elements".into(),
                );
            }
            declare(ctx, value.0, value.1, "Loop variable");
            let key_string = key.map(|(name, name_span)| {
                declare(ctx, name, name_span, "Loop variable");
                name.to_string()
            });
            let pre = ctx.move_state.clone();
            let pre_views = ctx.view_borrows.clone();
            let scope = env.len();
            env.push(Binding {
                name: value.0,
                ty: value_ty,
                mutable: false,
                scope: ctx.cleanup_scopes.len(),
                type_test_alias: true,
            });
            if let Some((name, _)) = key {
                env.push(Binding {
                    name,
                    ty: key_ty.unwrap_or(Ty::Nil),
                    mutable: false,
                    scope: ctx.cleanup_scopes.len(),
                    type_test_alias: true,
                });
            }
            let iteration_sources = view_sources(ctx, &checked_iterable);
            if !iteration_sources.is_empty() {
                ctx.view_borrows.push(ViewBorrow {
                    view_name: format!("for {}", value.0),
                    sources: iteration_sources,
                    scope: ctx.cleanup_scopes.len(),
                    persistent: true,
                });
            }
            ctx.loops.push(ctx.cleanup_scopes.len());
            let (checked_body, body_returns) = check_block(ctx, env, body, None);
            ctx.loops.pop();
            env.truncate(scope);
            let post = ctx.move_state.clone();
            let post_views = ctx.view_borrows.clone();
            for (root, move_span) in &post {
                if !pre.contains_key(root) {
                    ctx.error(
                        *move_span,
                        format!(
                            "'{root}' is moved here, but this is inside a 'for' body, so a later iteration would find '{root}' already moved"
                        ),
                    );
                }
            }
            ctx.move_state = if body_returns {
                pre
            } else {
                merge_moves(vec![pre, post])
            };
            ctx.view_borrows = if body_returns {
                pre_views
            } else {
                merge_view_borrows(vec![pre_views, post_views])
            };
            TStmt::For {
                value_name: value.0.to_string(),
                value_ty,
                key_name: key_string,
                key_ty,
                iterable: checked_iterable,
                collection_ty: iterable_ty,
                body: checked_body,
            }
        }
        BlockElement::Break(span) => {
            if ctx.loops.is_empty() {
                ctx.error(
                    *span,
                    "'break' is only valid inside a 'while' body or a 'for' body".into(),
                );
            }
            let cleanup = if let Some(scope_start) = ctx.loops.last().copied() {
                cleanup_for_exit(ctx, env, scope_start, Some(false))
            } else {
                Vec::new()
            };
            TStmt::Break { cleanup }
        }
        BlockElement::Defer { .. } => {
            ctx.unknown = Some("defer reached statement checking instead of block checking");
            TStmt::Expr(TExpr::Nil)
        }
        BlockElement::Return(value, span) => check_return(ctx, env, value, *span),
        BlockElement::If(form) => match check_if(ctx, env, form, None).0 {
            FlowIf::Stmt(stmt_if) => TStmt::If(stmt_if),
            FlowIf::Value(_) => {
                unreachable!("check_if only produces a value when a value was requested")
            }
        },
        BlockElement::Expr(expression) => {
            if let Expr::ReturnOnError(value) = &expression.0 {
                return match check_return_on_error(ctx, env, value, expression.1, true) {
                    CheckedReturnOnError::Statement(statement) => statement,
                    CheckedReturnOnError::Expr { .. } => {
                        unreachable!("statement-form return_on_error produced a value")
                    }
                };
            }
            // A call to a declaration without a result is a call statement, not
            // an expression whose value is discarded.
            if let Expr::Call(callee, arguments) = &expression.0 {
                return match check_call(ctx, env, expression.1, callee, arguments) {
                    Some(CheckedCall::Function {
                        name,
                        args,
                        result: None,
                    }) => TStmt::Call(name, args),
                    Some(CheckedCall::Function { name, args, .. }) => {
                        TStmt::Expr(TExpr::Call(name, args))
                    }
                    Some(CheckedCall::Method { call, result: None }) => TStmt::MethodCall(call),
                    Some(CheckedCall::Method { call, .. }) => {
                        TStmt::Expr(TExpr::MethodCall(Box::new(call)))
                    }
                    Some(CheckedCall::Statement(statement)) => statement,
                    Some(CheckedCall::Value(value, _)) => TStmt::Expr(value),
                    None => TStmt::Expr(TExpr::Nil),
                };
            }
            if let Expr::GenericCall(callee, _type_args, arguments) = &expression.0 {
                return match check_call(ctx, env, expression.1, callee, arguments) {
                    Some(CheckedCall::Function {
                        name,
                        args,
                        result: None,
                    }) => TStmt::Call(name, args),
                    Some(CheckedCall::Function { name, args, .. }) => {
                        TStmt::Expr(TExpr::Call(name, args))
                    }
                    Some(CheckedCall::Method { call, result: None }) => TStmt::MethodCall(call),
                    Some(CheckedCall::Method { call, .. }) => {
                        TStmt::Expr(TExpr::MethodCall(Box::new(call)))
                    }
                    Some(CheckedCall::Statement(statement)) => statement,
                    Some(CheckedCall::Value(value, _)) => TStmt::Expr(value),
                    None => TStmt::Expr(TExpr::Nil),
                };
            }
            TStmt::Expr(check_expr(ctx, env, expression).0)
        }
    }
}

/// Checks a `return` statement (Specification 026 section 5). `value`'s
/// presence is checked against `ctx.callable_result` alone -- never the
/// syntactic kind of the immediately enclosing block -- distinguishing three
/// facts: outside every callable (`None`), a no-result callable
/// (`Some(None)`), and a result-declaring callable (`Some(Some(ty))`).
pub(crate) fn check_return<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    value: &Option<Spanned<Expr<'src>>>,
    span: Span,
) -> TStmt {
    let Some(expected) = ctx.callable_result else {
        ctx.error(
            span,
            "'return' is only valid inside a function or method body".into(),
        );
        // Still type-checked for its own internal errors, even though there
        // is no callable result to check it against.
        if let Some(expr) = value {
            check_expr(ctx, env, expr);
        }
        return TStmt::Return {
            value: None,
            result: None,
            cleanup: Vec::new(),
        };
    };
    let checked_value = match (expected, value) {
        (Some(result_ty), Some(expr)) => {
            let (checked, ty) = check_expr(ctx, env, expr);
            // Specification 026 section 8: the returned expression is a
            // consuming context, exactly like a value block's trailing
            // expression -- a move-only value transfers to the caller.
            let checked = mark_consumed(ctx, env, checked, expr.1);
            Some(coerce(ctx, checked, ty, result_ty, expr.1))
        }
        (Some(result_ty), None) => {
            let name = ctx.name(result_ty);
            ctx.error(
                span,
                format!(
                    "bare 'return' is not valid here; this callable declares a result of \
                     type '{name}', so 'return' needs a value"
                ),
            );
            None
        }
        (None, Some(expr)) => {
            ctx.error(
                expr.1,
                "this callable declares no result, so 'return' cannot return a value; use \
                 bare 'return' instead"
                    .into(),
            );
            check_expr(ctx, env, expr);
            None
        }
        (None, None) => None,
    };
    // Specification 026 section 8: the result above is fully materialized
    // (and, for a moved root, marked unavailable) before this cleanup plan is
    // computed, exactly like a function's parameter-drop cleanup -- every scope
    // still open at this point, innermost first, is included in one flat
    // list because `env`'s declaration order already nests that way.
    let error_exit = checked_value
        .as_ref()
        .and_then(|value| expression_error_exit(ctx, value));
    let cleanup = cleanup_for_exit(ctx, env, 0, error_exit);
    TStmt::Return {
        value: checked_value,
        result: expected,
        cleanup,
    }
}
