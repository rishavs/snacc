//! Places, moves, borrows, and view liveness (Specification 011 sections 3,
//! 6.4, and 13; Specification 012 section 7; Specification 016 sections 6-8).

use super::*;

/// Splits `a.b.c` into its innermost atom and the field names selected from it.
pub(crate) fn flatten<'a, 'src>(
    expr: &'a Spanned<Expr<'src>>,
) -> (&'a Spanned<Expr<'src>>, Vec<Spanned<&'src str>>) {
    let mut fields = Vec::new();
    let mut current = expr;
    while let Expr::Member(base, name) = &current.0 {
        fields.push(*name);
        current = base;
    }
    fields.reverse();
    (current, fields)
}

/// Specification 016 section 4.3: field access and method calls automatically
/// dereference as many box layers as member resolution requires. Peeling
/// stops at the first non-box type, so a plain struct or union passes through
/// unchanged and `Box<Box<T>>` peels both layers.
pub(crate) fn deref_box(types: &Types, ty: Ty) -> Ty {
    let mut current = ty;
    while let Ty::Box(id) = current {
        current = types.box_pointee(id);
    }
    current
}

/// Walks a field path from a root type, reporting the first failure.
pub(crate) fn walk_fields(
    ctx: &mut Ctx<'_>,
    root_ty: Ty,
    fields: &[Spanned<&str>],
) -> Option<(Vec<usize>, Ty)> {
    let mut path = Vec::new();
    let mut current = root_ty;
    for (name, span) in fields {
        // Specification 016 section 4.3: cross as many box layers as needed
        // to reach the struct that actually owns this field.
        current = deref_box(&ctx.types, current);
        let Ty::User(id) = current else {
            let owner = ctx.name(current);
            ctx.error(
                *span,
                format!("'{owner}' is not a struct, so it has no field '{name}'"),
            );
            return None;
        };
        let Some((index, ty)) = ctx.types.field(id, name) else {
            let owner = ctx.types.def(id).name().to_string();
            let msg = if ctx.method_index.contains_key(&(id, (*name).to_string())) {
                format!("'{owner}.{name}' is a method; a method requires a receiver call")
            } else if ctx.types.def(id).fields().is_none() {
                format!("'{owner}' is not a struct, so it has no field '{name}'")
            } else {
                format!("'{owner}' has no field '{name}'")
            };
            ctx.error(*span, msg);
            return None;
        };
        path.push(index);
        current = ty;
    }
    Some((path, current))
}

/// Resolves the root of a place without reporting anything, so a caller can
/// fall back to value or type-path resolution.
pub(crate) fn place_root<'src>(
    ctx: &Ctx<'src>,
    env: &Env<'src>,
    root: &Expr<'src>,
) -> Option<(PlaceRoot, Ty, bool)> {
    match root {
        Expr::SelfRef => ctx
            .self_ty
            // Specification 012 section 9: `self` is writable inside its own
            // method body; caller permission is enforced at each call site.
            .map(|ty| (PlaceRoot::SelfRef, ty, true)),
        Expr::Local(name) => {
            env.iter()
                .rev()
                .find(|binding| binding.name == *name)
                .map(|binding| {
                    (
                        PlaceRoot::Local((*name).to_string()),
                        binding.ty,
                        binding.mutable,
                    )
                })
        }
        _ => None,
    }
}

pub(crate) fn as_place<'src>(
    ctx: &mut Ctx<'src>,
    env: &Env<'src>,
    expr: &Spanned<Expr<'src>>,
) -> PlaceOutcome {
    let (root, fields) = flatten(expr);
    let Some((place_root, root_ty, mutable)) = place_root(ctx, env, &root.0) else {
        return PlaceOutcome::NotAPlace;
    };
    let Some((path, ty)) = walk_fields(ctx, root_ty, &fields) else {
        return PlaceOutcome::Reported;
    };
    PlaceOutcome::Resolved(Resolved {
        place: Place {
            root: place_root,
            root_ty,
            path,
            ty,
        },
        mutable,
    })
}

/// Resolves a written place (an assignment target or an `is` subject).
pub(crate) fn resolve_place<'src>(
    ctx: &mut Ctx<'src>,
    env: &Env<'src>,
    path: &PlacePath<'src>,
) -> Option<Resolved> {
    let (root, root_ty, mutable) = match path.root {
        PlaceRootName::SelfRef => match ctx.self_ty {
            Some(ty) => (PlaceRoot::SelfRef, ty, true),
            None => {
                ctx.error(
                    path.root_span,
                    "'self' is only valid inside a method body".into(),
                );
                return None;
            }
        },
        PlaceRootName::Name(name) => {
            let Some(binding) = env.iter().rev().find(|binding| binding.name == name) else {
                ctx.error(
                    path.root_span,
                    format!("No such variable '{name}' in scope"),
                );
                return None;
            };
            (
                PlaceRoot::Local(name.to_string()),
                binding.ty,
                binding.mutable,
            )
        }
    };
    let (field_path, ty) = walk_fields(ctx, root_ty, &path.fields)?;
    Some(Resolved {
        place: Place {
            root,
            root_ty,
            path: field_path,
            ty,
        },
        mutable,
    })
}

/// Specification 016 section 6.1: tags a checked value with its [`UseMode`]
/// when it occupies one of the five consuming contexts (initialization,
/// assignment's right operand, a by-value argument, a function/method
/// result, or an aggregate constructor argument), and -- for a whole
/// move-only root -- runs the section 6.2 availability check this represents.
/// Every call site applies this before `coerce`, so a value later wrapped by
/// an implicit union or sum injection still carries the tag and, when
/// applicable, has already been checked. A value that is not a bare place
/// read (a call result, a fresh `box(...)`, a nested `if`, ...) has no root
/// to transfer and passes through unchanged.
///
/// Specification 016 section 6.4 rejects moving a move-only value out of a
/// field, union payload projection, or automatic box dereference -- only a
/// bare root (an empty path) may be wholly consumed. A union-test binding
/// (Specification 016 section 7.3) is the one case where an empty path is
/// still not a legitimate whole root: it is always a branch-scoped alias to
/// its tested place's payload, so `env` is consulted to reject that case too.
pub(crate) fn mark_consumed<'src>(
    ctx: &mut Ctx<'src>,
    env: &Env<'src>,
    expr: TExpr,
    span: Span,
) -> TExpr {
    let TExpr::Place(place, _) = expr else {
        return expr;
    };
    let whole_root = place.path.is_empty() && !is_type_test_alias(env, &place.root);
    if whole_root {
        check_move(ctx, &place, span);
    } else if ctx.types.is_move_only(place.ty) {
        let name = ctx.place_name(&place);
        ctx.error(
            span,
            format!("'{name}' cannot be moved out of; only a complete owning root can be moved"),
        );
    }
    TExpr::Place(place, UseMode::Consume)
}

/// Whether `root` currently names a union- or sum-test binding rather than an
/// ordinary local, parameter, or `self` (Specification 016 section 7.3).
pub(crate) fn is_type_test_alias<'src>(env: &Env<'src>, root: &PlaceRoot) -> bool {
    let PlaceRoot::Local(name) = root else {
        return false;
    };
    env.iter()
        .rev()
        .find(|binding| binding.name == name.as_str())
        .is_some_and(|binding| binding.type_test_alias)
}

/// Specification 016 section 6.2: a consuming use of a move-only root
/// requires availability. Reports a use-after-move diagnostic naming the root
/// if it is already moved; otherwise marks it moved from this point forward.
/// A no-op for a copyable type, which stays an ordinary copy regardless of
/// how many times it is used (Specification 016 section 5.3).
pub(crate) fn check_move(ctx: &mut Ctx<'_>, place: &Place, span: Span) {
    if !ctx.types.is_move_only(place.ty) {
        return;
    }
    reject_live_view_source(ctx, &place.root, span);
    if ctx.move_state.contains_key(&place.root) {
        ctx.error(
            span,
            format!("'{}' is already moved, so this use is invalid", place.root),
        );
        return;
    }
    ctx.move_state.insert(place.root.clone(), span);
}

pub(crate) fn is_builtin_view(ty: Ty) -> bool {
    matches!(ty, Ty::ViewByte | Ty::ViewUnicode | Ty::View(_))
}

pub(crate) fn is_borrowed_type(ctx: &Ctx<'_>, ty: Ty) -> bool {
    fn visit(ctx: &Ctx<'_>, ty: Ty, seen: &mut std::collections::HashSet<Ty>) -> bool {
        if !seen.insert(ty) {
            return false;
        }
        match ty {
            Ty::ViewByte | Ty::ViewUnicode => true,
            Ty::Sum(id) => ctx
                .types
                .sum_members(id)
                .iter()
                .copied()
                .any(|member| visit(ctx, member, seen)),
            Ty::User(id) => {
                if let Some(fields) = ctx.types.def(id).fields() {
                    fields.iter().any(|(_, field)| visit(ctx, *field, seen))
                } else if let Some(members) = ctx.types.union_members(id) {
                    members.iter().any(|member| {
                        ctx.types.def(*member).fields().is_some_and(|fields| {
                            fields.iter().any(|(_, field)| visit(ctx, *field, seen))
                        })
                    })
                } else {
                    ctx.types
                        .represented_target(id)
                        .is_some_and(|target| visit(ctx, target, seen))
                }
            }
            Ty::Box(id) => visit(ctx, ctx.types.box_pointee(id), seen),
            Ty::View(_) => true,
            Ty::Array(id) | Ty::List(id) => match ctx.types.collection(id) {
                CollectionDef::Array { elem, .. } | CollectionDef::List { elem } => {
                    visit(ctx, *elem, seen)
                }
                _ => false,
            },
            Ty::Map(id) => match ctx.types.collection(id) {
                CollectionDef::Map { key, value } => {
                    visit(ctx, *key, seen) || visit(ctx, *value, seen)
                }
                _ => false,
            },
            Ty::Set(id) => match ctx.types.collection(id) {
                CollectionDef::Set { elem } => visit(ctx, *elem, seen),
                _ => false,
            },
            _ => false,
        }
    }
    visit(ctx, ty, &mut std::collections::HashSet::new())
}

pub(crate) fn view_sources(ctx: &Ctx<'_>, value: &TExpr) -> Vec<PlaceRoot> {
    fn merge(into: &mut Vec<PlaceRoot>, sources: impl IntoIterator<Item = PlaceRoot>) {
        for source in sources {
            if !into.contains(&source) {
                into.push(source);
            }
        }
    }

    match value {
        TExpr::ViewFromString(value, _) => view_sources(ctx, value),
        TExpr::CollectionView(value, _) => view_sources(ctx, value),
        TExpr::CollectionSlice { value, .. } => view_sources(ctx, value),
        TExpr::ViewSlice { value, .. } => view_sources(ctx, value),
        TExpr::Place(place, _) => match &place.root {
            PlaceRoot::Local(name) => ctx
                .view_borrows
                .iter()
                .find(|borrow| borrow.view_name == *name)
                .map_or_else(|| vec![place.root.clone()], |borrow| borrow.sources.clone()),
            PlaceRoot::SelfRef => vec![PlaceRoot::SelfRef],
        },
        TExpr::Construct { fields, .. } => {
            let mut sources = Vec::new();
            for (_, field) in fields {
                merge(&mut sources, view_sources(ctx, field));
            }
            sources
        }
        TExpr::FieldRead { base, .. }
        | TExpr::Represent { value: base, .. }
        | TExpr::Inject { value: base, .. }
        | TExpr::InjectSum { value: base, .. }
        | TExpr::LiftSum { value: base, .. } => view_sources(ctx, base),
        TExpr::If(form) => {
            let mut sources = Vec::new();
            for (_, block) in &form.arms {
                if let Some(value) = &block.result {
                    merge(&mut sources, view_sources(ctx, value));
                }
            }
            if let Some(block) = &form.else_branch
                && let Some(value) = &block.result
            {
                merge(&mut sources, view_sources(ctx, value));
            }
            sources
        }
        TExpr::Print(value, ty) if is_builtin_view(*ty) => view_sources(ctx, value),
        _ => Vec::new(),
    }
}

pub(crate) fn reject_live_view_source(ctx: &mut Ctx<'_>, root: &PlaceRoot, span: Span) {
    if let Some(borrow) = ctx
        .view_borrows
        .iter()
        .find(|borrow| borrow.sources.contains(root))
    {
        ctx.error(
            span,
            format!(
                "cannot move or replace '{}': view '{}' still borrows it",
                root, borrow.view_name
            ),
        );
    }
}

pub(crate) fn expr_mentions_local<'src>(expression: &Expr<'src>, name: &str) -> bool {
    match expression {
        Expr::Local(local) => *local == name,
        Expr::List(items) => items.iter().any(|item| expr_mentions_local(&item.0, name)),
        Expr::Member(base, _) => expr_mentions_local(&base.0, name),
        Expr::Index(base, index) => {
            expr_mentions_local(&base.0, name) || expr_mentions_local(&index.0, name)
        }
        Expr::Binary(left, _, right) => {
            expr_mentions_local(&left.0, name) || expr_mentions_local(&right.0, name)
        }
        Expr::Unary(_, value)
        | Expr::ReturnOnError(value)
        | Expr::Print(value)
        | Expr::Box(value) => expr_mentions_local(&value.0, name),
        Expr::Call(callee, arguments) => {
            expr_mentions_local(&callee.0, name)
                || arguments
                    .0
                    .iter()
                    .any(|argument| expr_mentions_local(&argument.value.0, name))
        }
        Expr::GenericCall(callee, _, arguments) => {
            expr_mentions_local(&callee.0, name)
                || arguments
                    .0
                    .iter()
                    .any(|argument| expr_mentions_local(&argument.value.0, name))
        }
        Expr::Interpolated(parts) => parts.iter().any(|part| match part {
            crate::ast::StringPart::Literal(_) => false,
            crate::ast::StringPart::Expression(expression) => {
                expr_mentions_local(&expression.0, name)
            }
        }),
        Expr::Error
        | Expr::Value(_)
        | Expr::MapNew(_, _)
        | Expr::SetNew(_)
        | Expr::SelfRef
        | Expr::BuiltinType(_) => false,
    }
}

pub(crate) fn block_mentions_local<'src>(block: &Block<'src>, name: &str) -> bool {
    block
        .elements
        .iter()
        .any(|element| element_mentions_local(element, name))
}

pub(crate) fn element_mentions_local<'src>(
    element: &Spanned<BlockElement<'src>>,
    name: &str,
) -> bool {
    match &element.0 {
        BlockElement::Let { value, .. } | BlockElement::Assign { value, .. } => {
            expr_mentions_local(&value.0, name)
        }
        BlockElement::IndexedAssign { target, value } => {
            expr_mentions_local(&target.0, name) || expr_mentions_local(&value.0, name)
        }
        BlockElement::While {
            condition, body, ..
        } => expr_mentions_local(&condition.0, name) || block_mentions_local(body, name),
        BlockElement::For { iterable, body, .. } => {
            expr_mentions_local(&iterable.0, name) || block_mentions_local(body, name)
        }
        BlockElement::Return(value, _) => value
            .as_ref()
            .is_some_and(|value| expr_mentions_local(&value.0, name)),
        BlockElement::Defer { call, .. } | BlockElement::Expr(call) => {
            expr_mentions_local(&call.0, name)
        }
        BlockElement::If(form) => {
            form.arms.iter().any(|(condition, body)| {
                let condition_uses = match condition {
                    Condition::Expr(expression) => expr_mentions_local(&expression.0, name),
                    Condition::TypeTest(test) => {
                        matches!(test.place.root, PlaceRootName::Name(root) if root == name)
                    }
                };
                condition_uses || block_mentions_local(body, name)
            }) || form
                .else_branch
                .as_ref()
                .is_some_and(|body| block_mentions_local(body, name))
        }
        BlockElement::Break(_) => false,
    }
}

pub(crate) fn prune_view_borrows<'src>(
    ctx: &mut Ctx<'src>,
    remaining: &[Spanned<BlockElement<'src>>],
    scope: usize,
) {
    ctx.view_borrows.retain(|borrow| {
        borrow.scope < scope
            || borrow.persistent
            || ctx.cleanup_scopes.iter().flatten().any(|entry| {
                matches!(entry, TCleanup::Deferred(deferred) if deferred.dependencies.contains(&PlaceRoot::Local(borrow.view_name.clone())))
            })
            || remaining
                .iter()
                .any(|element| element_mentions_local(element, &borrow.view_name))
    });
}

/// Specification 016 section 6.2: a root is available after a merge only when
/// available on every reachable predecessor, so the merged moved-set is the
/// union of every predecessor's moved-set -- moved on even one predecessor is
/// enough to make it unavailable afterward. Keeps whichever span is found
/// first; which one survives does not matter; only whether the root is moved
/// at all does.
pub(crate) fn merge_moves(exits: Vec<HashMap<PlaceRoot, Span>>) -> HashMap<PlaceRoot, Span> {
    let mut merged = HashMap::new();
    for exit in exits {
        for (root, span) in exit {
            merged.entry(root).or_insert(span);
        }
    }
    merged
}

/// Conservatively merges borrow provenance at a control-flow join. A view
/// that can borrow either source protects both until a later assignment or
/// last-use pruning proves the active path no longer matters.
pub(crate) fn merge_view_borrows(exits: Vec<Vec<ViewBorrow>>) -> Vec<ViewBorrow> {
    let mut merged: Vec<ViewBorrow> = Vec::new();
    for exit in exits {
        for borrow in exit {
            if let Some(existing) = merged
                .iter_mut()
                .find(|existing| existing.view_name == borrow.view_name)
            {
                for source in borrow.sources {
                    if !existing.sources.contains(&source) {
                        existing.sources.push(source);
                    }
                }
                existing.scope = existing.scope.min(borrow.scope);
                existing.persistent |= borrow.persistent;
            } else {
                merged.push(borrow);
            }
        }
    }
    merged
}

/// Specification 016 section 8.1's checked cleanup plan: every binding in
/// `env[scope..]` (a block's own locally declared bindings, or -- called
/// with `scope: 0` once a function/method body finishes -- its parameters)
/// that is move-only and still available (absent from `ctx.move_state`) at
/// this exact point, in reverse declaration order so the last one bound is
/// the first destroyed (spec: "locals drop in reverse successful-
/// initialization order"). A union-/sum-test alias is excluded even though
/// its own path is empty: it is never an independent owning root
/// (`type_test_alias`'s own doc comment), so it is never a legitimate drop
/// target any more than it is a legitimate whole-root move.
pub(crate) fn compute_drops(ctx: &Ctx<'_>, env: &Env<'_>, scope: usize) -> Vec<Place> {
    let mut drops: Vec<Place> = env[scope..]
        .iter()
        .filter(|binding| {
            !binding.type_test_alias
                && ctx.types.is_move_only(binding.ty)
                && !ctx
                    .move_state
                    .contains_key(&PlaceRoot::Local(binding.name.to_string()))
        })
        .map(|binding| Place {
            root: PlaceRoot::Local(binding.name.to_string()),
            root_ty: binding.ty,
            path: Vec::new(),
            ty: binding.ty,
        })
        .collect();
    drops.reverse();
    drops
}

/// Parameters are registered before every body-local cleanup action, so their
/// drops belong at the end of the body's reverse-registration plan. Keeping
/// them in that one plan lets a deferred move disarm the corresponding drop
/// on both successful and error-classified implicit exits.
pub(crate) fn append_parameter_drops(ctx: &Ctx<'_>, env: &Env<'_>, body: &mut TBlock) {
    body.cleanup
        .extend(compute_drops(ctx, env, 0).into_iter().map(TCleanup::Drop));
}

/// Builds an exit plan from currently armed entries. Returns exit every
/// currently open lexical scope; `scope_start` lets `break` retain outer
/// scopes. Parameter drops are added when no local cleanup entry represents
/// that root.
pub(crate) fn cleanup_for_exit(
    ctx: &mut Ctx<'_>,
    env: &Env<'_>,
    scope_start: usize,
    error_exit: Option<bool>,
) -> Vec<TCleanup> {
    let mut entries = Vec::new();
    let mut known_roots = Vec::new();
    for scope in ctx.cleanup_scopes.iter().skip(scope_start) {
        for entry in scope {
            match entry {
                TCleanup::Drop(place) => {
                    known_roots.push(place.root.clone());
                    if !ctx.move_state.contains_key(&place.root) {
                        entries.push(TCleanup::Drop(place.clone()));
                    }
                }
                TCleanup::Deferred(deferred) => {
                    entries.push(TCleanup::Deferred(Rc::clone(deferred)));
                }
            }
        }
    }
    for place in compute_drops(ctx, env, 0).into_iter().rev() {
        if !known_roots.contains(&place.root) {
            entries.push(TCleanup::Drop(place));
        }
    }
    entries.reverse();
    validate_cleanup_dependencies(ctx, &entries, error_exit);
    entries
}

pub(crate) fn cleanup_for_exit_from_entries(
    ctx: &mut Ctx<'_>,
    entries: Vec<TCleanup>,
    error_exit: Option<bool>,
) -> Vec<TCleanup> {
    let entries: Vec<TCleanup> = entries
        .into_iter()
        .filter(|entry| {
            !matches!(entry, TCleanup::Drop(place) if ctx.move_state.contains_key(&place.root))
        })
        .rev()
        .collect();
    validate_cleanup_dependencies(ctx, &entries, error_exit);
    entries
}

pub(crate) fn validate_cleanup_dependencies(
    ctx: &mut Ctx<'_>,
    entries: &[TCleanup],
    error_exit: Option<bool>,
) {
    let mut unavailable: HashSet<PlaceRoot> = ctx.move_state.keys().cloned().collect();
    for entry in entries {
        let TCleanup::Deferred(deferred) = entry else {
            continue;
        };
        if deferred.on_error && error_exit == Some(false) {
            continue;
        }
        for root in &deferred.dependencies {
            if unavailable.contains(root) {
                ctx.error(
                    deferred.span,
                    format!(
                        "deferred call cannot use '{root}' at scope exit because its current value is unavailable"
                    ),
                );
            }
        }
        unavailable.extend(deferred.consumes.iter().cloned());
    }
}

pub(crate) fn is_error_type(ctx: &Ctx<'_>, ty: Ty) -> bool {
    matches!(ty, Ty::User(id) if ctx.types.def(id).name() == "Error")
}

/// Classifies an exit result when its active member is statically visible.
/// `None` means runtime-dependent and therefore requires validating both
/// unconditional and error-only cleanup paths.
pub(crate) fn expression_error_exit(ctx: &Ctx<'_>, value: &TExpr) -> Option<bool> {
    match value {
        TExpr::Place(place, _) if is_error_type(ctx, place.ty) => Some(true),
        TExpr::Place(place, _) if matches!(place.ty, Ty::Sum(_)) => None,
        TExpr::Construct { type_id, .. } if ctx.types.def(*type_id).name() == "Error" => Some(true),
        TExpr::InjectSum { member, .. } => Some(is_error_type(ctx, *member)),
        TExpr::LiftSum { value, .. } | TExpr::Represent { value, .. } => {
            expression_error_exit(ctx, value)
        }
        TExpr::If(form) => {
            let mut facts = form
                .arms
                .iter()
                .filter_map(|(_, block)| block.result.as_ref())
                .map(|value| expression_error_exit(ctx, value))
                .collect::<Vec<_>>();
            if let Some(block) = &form.else_branch
                && let Some(value) = &block.result
            {
                facts.push(expression_error_exit(ctx, value));
            }
            facts
                .first()
                .copied()
                .filter(|first| facts.iter().all(|fact| fact == first))
                .flatten()
        }
        TExpr::Call(_, _) | TExpr::MethodCall(_) => None,
        // A continued `return_on_error` expression necessarily contains its
        // non-Error success projection; its Error path has already exited.
        TExpr::ReturnOnError { .. } => Some(false),
        _ => Some(false),
    }
}

/// Specification 011 section 3: two places overlap when they are identical or
/// one is reached by selecting fields from the other. Two paths that first
/// differ at sibling field indices are disjoint.
pub(crate) fn overlaps(left: &Place, right: &Place) -> bool {
    if left.root != right.root {
        return false;
    }
    let shared = left.path.len().min(right.path.len());
    left.path[..shared] == right.path[..shared]
}

/// Specification 011 section 6.4: every pair of reference arguments, and each
/// reference argument against an addressable method receiver. A temporary
/// receiver has independent storage and cannot overlap a caller place.
/// Specification 016 section 7.2's closing sentence extends this through
/// boxes: `moves` is every whole-root move-only by-value argument in the same
/// call (Specification 016 Task B's `check_args` collects it alongside
/// `references`), and none of them may overlap a reference argument either --
/// a borrowed allocation cannot also be moved out from under the call.
pub(crate) fn reject_overlap(
    ctx: &mut Ctx<'_>,
    references: &[(String, Place, Span)],
    moves: &[(String, Place, Span)],
    receiver: Option<&Place>,
) {
    for (index, (name, place, span)) in references.iter().enumerate() {
        if let Some(receiver) = receiver
            && overlaps(place, receiver)
        {
            let argument = ctx.place_name(place);
            let subject = ctx.place_name(receiver);
            ctx.error(
                *span,
                format!(
                    "reference argument '{argument}' for parameter '{name}' overlaps the \
                     receiver '{subject}', which the method may access through 'self'"
                ),
            );
        }
        for (other_name, other, _) in &references[index + 1..] {
            if overlaps(place, other) {
                let left = ctx.place_name(place);
                let right = ctx.place_name(other);
                ctx.error(
                    *span,
                    format!(
                        "reference arguments '{left}' and '{right}' overlap, so parameters \
                         '{name}' and '{other_name}' cannot both have exclusive access"
                    ),
                );
            }
        }
        for (moved_name, moved_place, _) in moves {
            if overlaps(place, moved_place) {
                let argument = ctx.place_name(place);
                let moved = ctx.place_name(moved_place);
                ctx.error(
                    *span,
                    format!(
                        "reference argument '{argument}' for parameter '{name}' overlaps the \
                         moved argument '{moved}' for parameter '{moved_name}', so it cannot \
                         be borrowed in the same call that moves it"
                    ),
                );
            }
        }
    }
}
