//! Declaration collection, signatures, and generics (Specification 010
//! section 19 phase 2).

use super::*;

pub(crate) fn type_ref_is_param(ty: &TypeRef<'_>, params: &HashSet<&str>) -> bool {
    matches!(ty, TypeRef::Named(segments) if segments.len() == 1 && params.contains(segments[0].0))
}

pub(crate) fn type_ref_mentions_param(ty: &TypeRef<'_>, params: &HashSet<&str>) -> bool {
    match ty {
        TypeRef::Named(segments) => segments.len() == 1 && params.contains(segments[0].0),
        TypeRef::Apply { args, .. } | TypeRef::Sum(args) => args
            .iter()
            .any(|(argument, _)| type_ref_mentions_param(argument, params)),
        TypeRef::Box(inner)
        | TypeRef::View(inner)
        | TypeRef::Array(inner, _)
        | TypeRef::List(inner)
        | TypeRef::Set(inner) => type_ref_mentions_param(&inner.0, params),
        TypeRef::Map(key, value) => {
            type_ref_mentions_param(&key.0, params) || type_ref_mentions_param(&value.0, params)
        }
        TypeRef::Builtin(_) => false,
    }
}

pub(crate) fn generic_operation(errors: &mut Vec<Error>, span: Span, operation: &str) {
    errors.push(Error {
        span,
        msg: format!(
            "cannot use an unconstrained generic type parameter with {operation}; add a concrete type or a future capability bound"
        ),
    });
}

pub(crate) fn validate_generic_expr<'src>(
    expression: &Spanned<Expr<'src>>,
    generic_vars: &HashSet<&'src str>,
    opaque_vars: &HashSet<&'src str>,
    errors: &mut Vec<Error>,
) -> bool {
    match &expression.0 {
        Expr::Local(name) => generic_vars.contains(name),
        Expr::Binary(left, op, right) => {
            let uses = validate_generic_expr(left, generic_vars, opaque_vars, errors)
                || validate_generic_expr(right, generic_vars, opaque_vars, errors);
            if uses {
                let operation = match op {
                    crate::ast::BinaryOp::Add
                    | crate::ast::BinaryOp::Sub
                    | crate::ast::BinaryOp::Mul
                    | crate::ast::BinaryOp::Div => "arithmetic",
                    crate::ast::BinaryOp::Eq | crate::ast::BinaryOp::NotEq => "equality",
                    crate::ast::BinaryOp::Less
                    | crate::ast::BinaryOp::LessEq
                    | crate::ast::BinaryOp::Greater
                    | crate::ast::BinaryOp::GreaterEq => "comparison",
                    crate::ast::BinaryOp::And | crate::ast::BinaryOp::Or => "logical operations",
                };
                generic_operation(errors, expression.1, operation);
            }
            uses
        }
        Expr::Unary(_, value) => {
            let uses = validate_generic_expr(value, generic_vars, opaque_vars, errors);
            if uses {
                generic_operation(errors, expression.1, "unary operations");
            }
            uses
        }
        Expr::Print(value) => {
            let uses = validate_generic_expr(value, generic_vars, opaque_vars, errors);
            if uses {
                generic_operation(errors, expression.1, "printing");
            }
            uses
        }
        Expr::Member(base, _) => {
            let uses = validate_generic_expr(base, generic_vars, opaque_vars, errors);
            if matches!(&base.0, Expr::Local(name) if opaque_vars.contains(name)) {
                generic_operation(errors, expression.1, "field or method access");
            }
            uses
        }
        Expr::Index(base, index) => {
            let uses = validate_generic_expr(base, generic_vars, opaque_vars, errors)
                || validate_generic_expr(index, generic_vars, opaque_vars, errors);
            if matches!(&base.0, Expr::Local(name) if opaque_vars.contains(name)) {
                generic_operation(errors, expression.1, "indexing");
            }
            uses
        }
        Expr::Call(callee, args) => {
            validate_generic_expr(callee, generic_vars, opaque_vars, errors)
                || args
                    .0
                    .iter()
                    .any(|arg| validate_generic_expr(&arg.value, generic_vars, opaque_vars, errors))
        }
        Expr::GenericCall(callee, _, args) => {
            validate_generic_expr(callee, generic_vars, opaque_vars, errors)
                || args
                    .0
                    .iter()
                    .any(|arg| validate_generic_expr(&arg.value, generic_vars, opaque_vars, errors))
        }
        Expr::Interpolated(parts) => {
            let uses = parts.iter().any(|part| match part {
                crate::ast::StringPart::Literal(_) => false,
                crate::ast::StringPart::Expression(value) => {
                    validate_generic_expr(value, generic_vars, opaque_vars, errors)
                }
            });
            if uses {
                generic_operation(errors, expression.1, "string interpolation");
            }
            uses
        }
        Expr::List(items) => items
            .iter()
            .any(|item| validate_generic_expr(item, generic_vars, opaque_vars, errors)),
        Expr::ReturnOnError(value) => {
            let uses = validate_generic_expr(value, generic_vars, opaque_vars, errors);
            if uses {
                generic_operation(errors, expression.1, "error propagation");
            }
            uses
        }
        Expr::Box(value) => validate_generic_expr(value, generic_vars, opaque_vars, errors),
        Expr::Error
        | Expr::Value(_)
        | Expr::SelfRef
        | Expr::BuiltinType(_)
        | Expr::MapNew(_, _)
        | Expr::SetNew(_) => false,
    }
}

pub(crate) fn validate_generic_block<'src>(
    block: &Block<'src>,
    generic_params: &HashSet<&'src str>,
    ctx: &Ctx<'src>,
    generic_vars: &mut HashSet<&'src str>,
    opaque_vars: &mut HashSet<&'src str>,
    errors: &mut Vec<Error>,
) {
    for element in &block.elements {
        match &element.0 {
            BlockElement::Let {
                name, ty, value, ..
            } => {
                validate_generic_signature_type(ty, generic_params, ctx, false, errors);
                validate_generic_expr(value, generic_vars, opaque_vars, errors);
                if expr_is_literal(value) && type_ref_is_param(&ty.0, generic_params) {
                    generic_operation(errors, value.1, "a generic-parameter literal or value");
                }
                if type_ref_mentions_param(&ty.0, generic_params) {
                    generic_vars.insert(name);
                }
                if type_ref_is_param(&ty.0, generic_params) {
                    opaque_vars.insert(name);
                }
            }
            BlockElement::Assign { value, .. } | BlockElement::IndexedAssign { value, .. } => {
                validate_generic_expr(value, generic_vars, opaque_vars, errors);
            }
            BlockElement::While {
                condition, body, ..
            } => {
                if validate_generic_expr(condition, generic_vars, opaque_vars, errors) {
                    generic_operation(errors, condition.1, "truthiness");
                }
                let mut nested = generic_vars.clone();
                let mut nested_opaque = opaque_vars.clone();
                validate_generic_block(
                    body,
                    generic_params,
                    ctx,
                    &mut nested,
                    &mut nested_opaque,
                    errors,
                );
            }
            BlockElement::For { iterable, body, .. } => {
                if validate_generic_expr(iterable, generic_vars, opaque_vars, errors) {
                    generic_operation(errors, iterable.1, "iteration");
                }
                let mut nested = generic_vars.clone();
                let mut nested_opaque = opaque_vars.clone();
                validate_generic_block(
                    body,
                    generic_params,
                    ctx,
                    &mut nested,
                    &mut nested_opaque,
                    errors,
                );
            }
            BlockElement::Return(value, _span) => {
                if let Some(value) = value {
                    validate_generic_expr(value, generic_vars, opaque_vars, errors);
                }
            }
            BlockElement::Defer { call, .. } => {
                validate_generic_expr(call, generic_vars, opaque_vars, errors);
            }
            BlockElement::If(form) => {
                for (condition, body) in &form.arms {
                    match condition {
                        Condition::Expr(value) => {
                            if validate_generic_expr(value, generic_vars, opaque_vars, errors) {
                                generic_operation(errors, value.1, "truthiness");
                            }
                        }
                        Condition::TypeTest(test) => {
                            if matches!(test.place.root, PlaceRootName::Name(name) if generic_vars.contains(name))
                            {
                                generic_operation(errors, test.span, "type tests");
                            }
                        }
                    }
                    let mut nested = generic_vars.clone();
                    let mut nested_opaque = opaque_vars.clone();
                    validate_generic_block(
                        body,
                        generic_params,
                        ctx,
                        &mut nested,
                        &mut nested_opaque,
                        errors,
                    );
                }
                if let Some(body) = &form.else_branch {
                    let mut nested = generic_vars.clone();
                    let mut nested_opaque = opaque_vars.clone();
                    validate_generic_block(
                        body,
                        generic_params,
                        ctx,
                        &mut nested,
                        &mut nested_opaque,
                        errors,
                    );
                }
            }
            BlockElement::Expr(value) => {
                validate_generic_expr(value, generic_vars, opaque_vars, errors);
            }
            BlockElement::Break(_) => {}
        }
    }
}

pub(crate) fn expr_is_literal(expression: &Spanned<Expr<'_>>) -> bool {
    matches!(expression.0, Expr::Value(_))
}

pub(crate) fn validate_generic_signature_type(
    ty: &Spanned<TypeRef<'_>>,
    params: &HashSet<&str>,
    ctx: &Ctx<'_>,
    nil_member: bool,
    errors: &mut Vec<Error>,
) {
    match &ty.0 {
        TypeRef::Builtin(TypeName::Nil) if !nil_member => errors.push(Error {
            span: ty.1,
            msg: types::STANDALONE_NIL.to_string(),
        }),
        TypeRef::Builtin(_) => {}
        TypeRef::Named(path) => {
            if path.len() == 1 && params.contains(path[0].0) {
                return;
            }
            let Some(root) = ctx.types.top_level(path[0].0) else {
                errors.push(Error {
                    span: path[0].1,
                    msg: format!("Unknown type '{}'", path[0].0),
                });
                return;
            };
            if path.len() == 2 {
                if ctx.types.member(root, path[1].0).is_none() {
                    errors.push(Error {
                        span: path[1].1,
                        msg: format!("Unknown type '{}.{}'", path[0].0, path[1].0),
                    });
                }
            } else if path.len() > 2 {
                errors.push(Error {
                    span: ty.1,
                    msg: "a qualified type name has at most two components".into(),
                });
            }
        }
        TypeRef::Apply { path, args } => {
            if path.len() != 1 {
                errors.push(Error {
                    span: ty.1,
                    msg: "generic type applications must name a top-level type".into(),
                });
            } else if let Some(declaration) = ctx.generic_types.get(path[0].0) {
                if declaration.generic_params.len() != args.len() {
                    errors.push(Error {
                        span: ty.1,
                        msg: format!(
                            "generic type '{}' expects {} type arguments, found {}",
                            path[0].0,
                            declaration.generic_params.len(),
                            args.len()
                        ),
                    });
                }
            } else {
                errors.push(Error {
                    span: path[0].1,
                    msg: format!("Unknown generic type '{}'", path[0].0),
                });
            }
            for argument in args {
                validate_generic_signature_type(argument, params, ctx, false, errors);
            }
        }
        TypeRef::Sum(members) => {
            for member in members {
                validate_generic_signature_type(member, params, ctx, true, errors);
            }
        }
        TypeRef::Box(inner)
        | TypeRef::View(inner)
        | TypeRef::Array(inner, _)
        | TypeRef::List(inner)
        | TypeRef::Set(inner) => {
            validate_generic_signature_type(inner, params, ctx, false, errors);
        }
        TypeRef::Map(key, value) => {
            validate_generic_signature_type(key, params, ctx, false, errors);
            validate_generic_signature_type(value, params, ctx, false, errors);
        }
    }
}

pub(crate) fn validate_generic_function<'src>(
    function: &Func<'src>,
    ctx: &Ctx<'src>,
    errors: &mut Vec<Error>,
) {
    let mut generic_params = HashSet::new();
    for (name, span) in &function.generic_params {
        if !generic_params.insert(*name) {
            errors.push(Error {
                span: *span,
                msg: format!("Generic parameter '{name}' already exists"),
            });
        }
    }
    let mut generic_vars = HashSet::new();
    let mut opaque_vars = HashSet::new();
    let mut value_params = HashSet::new();
    for param in &function.args {
        if !value_params.insert(param.name) {
            errors.push(Error {
                span: param.span,
                msg: format!("Parameter '{}' already exists", param.name),
            });
        }
        validate_generic_signature_type(&param.ty, &generic_params, ctx, false, errors);
        if type_ref_mentions_param(&param.ty.0, &generic_params) {
            generic_vars.insert(param.name);
        }
        if type_ref_is_param(&param.ty.0, &generic_params) {
            opaque_vars.insert(param.name);
        }
    }
    if let Some(result) = &function.ret {
        validate_generic_signature_type(result, &generic_params, ctx, false, errors);
    }
    validate_generic_block(
        &function.body,
        &generic_params,
        ctx,
        &mut generic_vars,
        &mut opaque_vars,
        errors,
    );
}

/// Runs the ordinary checker over every generic declaration even when no
/// concrete application is reachable. Private nominal stand-ins preserve the
/// opacity and distinct identity of each type parameter; the dedicated
/// capability pass above remains responsible for explaining operations that
/// are categorically unavailable on unconstrained parameters.
pub(crate) fn validate_generic_function_body<'src>(
    function: &Func<'src>,
    ctx: &Ctx<'src>,
    errors: &mut Vec<Error>,
) {
    let capability_spans: HashSet<(usize, usize)> = errors
        .iter()
        .map(|error| (error.span.start, error.span.end))
        .collect();
    let mut scratch = ctx.generic_scratch();
    for (name, _) in &function.generic_params {
        let id = scratch.types.reserve_generic_parameter(name);
        scratch.generic_subst.insert(name, Ty::User(id));
    }
    let params = resolve_generic_params(&mut scratch, &function.args);
    let result = function
        .ret
        .as_ref()
        .map(|ty| resolve_type(&mut scratch, ty));
    let mut env = Env::new();
    begin_region(&mut scratch, &mut env, &function.args, &params, None);
    scratch.callable_result = Some(result);
    let (mut body, _) = check_block(&mut scratch, &mut env, &function.body, result);
    append_parameter_drops(&scratch, &env, &mut body);

    errors.extend(
        scratch
            .errors
            .into_iter()
            .filter(|error| !capability_spans.contains(&(error.span.start, error.span.end))),
    );
}

/// Starts one function-wide binding region: clears the reserved-name set, binds
/// every parameter, and records the receiver type for a method.
pub(crate) fn begin_region<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    args: &[Param<'src>],
    params: &[TParam],
    self_ty: Option<Ty>,
) -> Vec<TParam> {
    ctx.declared.clear();
    ctx.loops.clear();
    ctx.move_state.clear();
    ctx.view_borrows.clear();
    ctx.self_ty = self_ty;
    for (arg, param) in args.iter().zip(params) {
        declare(ctx, arg.name, arg.span, "Parameter");
        env.push(Binding {
            name: arg.name,
            ty: param.ty,
            // Specification 012 section 8: ordinary parameters are immutable.
            // Specification 011 section 19 phase 2 step 1: a reference parameter
            // is a mutable root of type `T`, exactly like a `let mut` local, so
            // every read, write, field selection, and receiver use in the body
            // goes through the machinery that already exists for those.
            mutable: param.mode == ParamMode::Reference,
            scope: 0,
            type_test_alias: false,
        });
    }
    params.to_vec()
}

/// The least fixed point of "this method may write its receiver": a method
/// writes it when it writes directly, or when it calls -- transitively, and
/// through cycles -- a receiver-writing method on a `self`-rooted receiver.
/// Monotone and order-independent, so the result is deterministic.
pub(crate) fn solve_receiver_writes(direct: &[bool], edges: &[(MethodId, MethodId)]) -> Vec<bool> {
    let mut writes = direct.to_vec();
    loop {
        let mut changed = false;
        for (caller, callee) in edges {
            if writes[callee.index()] && !writes[caller.index()] {
                writes[caller.index()] = true;
                changed = true;
            }
        }
        if !changed {
            return writes;
        }
    }
}

pub(crate) fn check_duplicate_params(ctx: &mut Ctx<'_>, params: &[Param<'_>]) {
    let mut seen: Vec<&str> = Vec::new();
    for param in params {
        if seen.contains(&param.name) {
            ctx.error(
                param.span,
                format!("Parameter '{}' already exists", param.name),
            );
        } else {
            seen.push(param.name);
        }
    }
}

/// Records a function-wide binding, reporting a duplicate rather than creating
/// a second layer for the same name.
pub(crate) fn declare<'src>(ctx: &mut Ctx<'src>, name: &'src str, span: Span, kind: &str) {
    if ctx.declared.contains(&name) {
        ctx.error(span, format!("{kind} '{name}' already exists"));
    } else {
        ctx.declared.push(name);
    }
}

pub(crate) fn is_rust_identifier(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Resolves a written type in a `let`. Function, method, field, and bridge
/// types are resolved once during declaration collection.
pub(crate) fn resolve_generic_params(ctx: &mut Ctx<'_>, params: &[Param<'_>]) -> Vec<TParam> {
    params
        .iter()
        .map(|param| TParam {
            name: param.name.to_string(),
            ty: resolve_type(ctx, &param.ty),
            mode: param.mode,
        })
        .collect()
}

pub(crate) fn generic_name(ctx: &Ctx<'_>, name: &str, args: &[Ty]) -> String {
    let encoded = args
        .iter()
        .map(|ty| {
            ctx.name(*ty)
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("$");
    format!("$snacc$generic${name}${encoded}")
}

pub(crate) fn instantiate_generic_type(
    ctx: &mut Ctx<'_>,
    name: &str,
    args: &[Ty],
    span: Span,
) -> Option<TypeId> {
    let (generic_params, body, declaration_span) = {
        let declaration = ctx.generic_types.get(name).copied()?;
        (
            declaration.generic_params.clone(),
            declaration.body.clone(),
            declaration.span,
        )
    };
    if generic_params.len() != args.len() {
        ctx.error(
            span,
            format!(
                "generic type '{name}' expects {} type arguments, found {}",
                generic_params.len(),
                args.len()
            ),
        );
        return None;
    }
    let TypeBody::Struct(fields) = body else {
        ctx.error(span, format!("generic type '{name}' must be a struct"));
        return None;
    };
    let key = format!("{name}<{args:?}>");
    if let Some(id) = ctx.types.generic_specialization(name, args) {
        return Some(id);
    }
    let display = format!(
        "{name}<{}>",
        args.iter()
            .map(|ty| ctx.name(*ty))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut chain = ctx.generic_chain.clone();
    chain.extend(ctx.generic_type_stack.iter().cloned());
    chain.push(display.clone());
    if ctx.generic_depth + ctx.generic_type_stack.len() >= MAX_SPECIALIZATION_DEPTH {
        ctx.error(
            declaration_span,
            format!(
                "generic specialization depth exceeds {MAX_SPECIALIZATION_DEPTH}; requested at {}..{}; instantiation chain: {}",
                span.start,
                span.end,
                chain.join(" -> ")
            ),
        );
        return None;
    }
    let already_in_progress = ctx.generic_type_in_progress.contains(&key);
    if !already_in_progress && ctx.specialization_count >= MAX_SPECIALIZATIONS {
        ctx.error(
            declaration_span,
            format!(
                "generic specialization limit exceeded (maximum {MAX_SPECIALIZATIONS}); requested at {}..{}; instantiation chain: {}",
                span.start,
                span.end,
                chain.join(" -> ")
            ),
        );
        return None;
    }
    let id = ctx
        .types
        .reserve_generic_struct(key.clone(), display.clone());
    if already_in_progress {
        return Some(id);
    }
    ctx.specialization_count += 1;
    ctx.generic_type_in_progress.insert(key.clone());
    ctx.generic_type_stack.push(display);
    let substitutions: HashMap<&str, Ty> = generic_params
        .iter()
        .zip(args)
        .map(|((param, _), ty)| (*param, *ty))
        .collect();
    let previous = std::mem::replace(&mut ctx.generic_subst, substitutions);
    let resolved = fields
        .iter()
        .map(|field| (field.name.to_string(), resolve_type(ctx, &field.ty)))
        .collect();
    ctx.generic_subst = previous;
    ctx.types.finish_generic_struct(id, resolved);
    ctx.generic_type_stack.pop();
    ctx.generic_type_in_progress.remove(&key);
    if let Some(cycle) = ctx.types.generic_layout_cycle(id) {
        ctx.error(
            span,
            format!(
                "Type '{}' has an infinite value layout: {}",
                ctx.types.def(id).name(),
                cycle.join(" -> ")
            ),
        );
        return None;
    }
    ctx.types.refresh_generic_properties();
    ctx.generic_type_finished.insert(key);
    Some(id)
}

pub(crate) fn resolve_type(ctx: &mut Ctx<'_>, ty: &Spanned<TypeRef<'_>>) -> Ty {
    match &ty.0 {
        // Specification 012 section 10: a local declaration is an ordinary
        // value-type position, so a standalone `Nil` is rejected here exactly
        // as it is during declaration collection.
        TypeRef::Builtin(TypeName::Nil) => {
            ctx.error(ty.1, types::STANDALONE_NIL.to_string());
            Ty::Nil
        }
        TypeRef::Builtin(name) => Ty::from(*name),
        TypeRef::Named(segments) => {
            let (first, first_span) = segments[0];
            if segments.len() == 1
                && let Some(ty) = ctx.generic_subst.get(first)
            {
                return *ty;
            }
            let Some(root) = ctx.types.top_level(first) else {
                ctx.error(first_span, format!("Unknown type '{first}'"));
                return Ty::Nil;
            };
            match segments.len() {
                1 => Ty::User(root),
                2 => {
                    let (member, span) = segments[1];
                    match ctx.types.member(root, member) {
                        Some(id) => Ty::User(id),
                        None => {
                            ctx.error(span, format!("Unknown type '{first}.{member}'"));
                            Ty::Nil
                        }
                    }
                }
                _ => {
                    ctx.error(
                        ty.1,
                        "a qualified type name has at most two components".into(),
                    );
                    Ty::Nil
                }
            }
        }
        TypeRef::Apply { path, args } => {
            if path.len() != 1 {
                ctx.error(
                    ty.1,
                    "generic type applications must name a top-level type".into(),
                );
                return Ty::Nil;
            }
            let resolved: Vec<Ty> = args.iter().map(|arg| resolve_type(ctx, arg)).collect();
            match ctx
                .types
                .generic_specialization(path[0].0, &resolved)
                .or_else(|| instantiate_generic_type(ctx, path[0].0, &resolved, ty.1))
            {
                Some(id) => Ty::User(id),
                None => {
                    ctx.error(
                        ty.1,
                        format!("unknown generic type specialization '{}'", ty.0),
                    );
                    Ty::Nil
                }
            }
        }
        TypeRef::Sum(members) => resolve_sum(ctx, members, ty.1),
        // Specification 016 section 4.1: the pointee resolves through
        // ordinary type resolution, exactly like declaration collection's
        // `resolve` in `types.rs`. Neither `Ref<T>` nor a no-result type has
        // a `TypeRef` spelling that reaches here, so every pointee is already
        // a storable value type.
        TypeRef::Box(inner) => {
            let pointee = resolve_type(ctx, inner);
            Ty::Box(ctx.types.intern_box(pointee))
        }
        TypeRef::View(inner) => match resolve_type(ctx, inner) {
            Ty::Byte => Ty::ViewByte,
            Ty::Unicode => Ty::ViewUnicode,
            other => Ty::View(
                ctx.types
                    .intern_collection(CollectionDef::View { elem: other }),
            ),
        },
        TypeRef::Array(inner, len) => {
            let elem = resolve_type(ctx, inner);
            Ty::Array(
                ctx.types
                    .intern_collection(CollectionDef::Array { elem, len: *len }),
            )
        }
        TypeRef::List(inner) => {
            let elem = resolve_type(ctx, inner);
            Ty::List(ctx.types.intern_collection(CollectionDef::List { elem }))
        }
        TypeRef::Map(key, value) => {
            let key = resolve_type(ctx, key);
            let value = resolve_type(ctx, value);
            Ty::Map(
                ctx.types
                    .intern_collection(CollectionDef::Map { key, value }),
            )
        }
        TypeRef::Set(inner) => {
            let elem = resolve_type(ctx, inner);
            Ty::Set(ctx.types.intern_collection(CollectionDef::Set { elem }))
        }
    }
}

/// Every built-in type keyword's segment spelling, as accepted by a type
/// test naming a direct sum member (Specification 018 section 6). Shares no
/// code with `resolve_type`'s `TypeRef::Builtin` arm because a type test
/// receives a bare name string from the parser, not a `TypeName`.
pub(crate) fn builtin_type_name(name: &str) -> Option<TypeName> {
    Some(match name {
        "Float64" => TypeName::Float64,
        "Int64" => TypeName::Int64,
        "Bool" => TypeName::Bool,
        "Nil" => TypeName::Nil,
        "String" => TypeName::String,
        "Unicode" => TypeName::Unicode,
        "Byte" => TypeName::Byte,
        "UInt16" => TypeName::UInt16,
        "UInt32" => TypeName::UInt32,
        "UInt64" => TypeName::UInt64,
        "Float32" => TypeName::Float32,
        _ => return None,
    })
}

/// Specification 018 section 4: resolves every syntactic member, expanding a
/// nested sum (from a parenthesized group) into its own already-flattened
/// members, then applies the member-set rules shared with declaration
/// collection (`resolve_sum` in `types.rs`). A member that itself reports an
/// error is dropped rather than kept as `resolve_type`'s `Ty::Nil` filler, so
/// a genuinely unrelated failure never masquerades as a repeated or lone
/// `Nil` member.
pub(crate) fn resolve_sum(ctx: &mut Ctx<'_>, members: &[Spanned<TypeRef<'_>>], span: Span) -> Ty {
    let mut raw: Vec<(Option<Ty>, Span)> = Vec::new();
    for member in members {
        // `resolve_type`'s `TypeRef::Builtin(TypeName::Nil)` arm always
        // rejects a standalone `Nil` because that arm is normally reached
        // only by one; `Nil` as a sum member is the valid, expected spelling
        // this specification adds, so it bypasses that rejection here.
        if let TypeRef::Builtin(TypeName::Nil) = &member.0 {
            raw.push((Some(Ty::Nil), member.1));
            continue;
        }
        let before = ctx.errors.len();
        let resolved = resolve_type(ctx, member);
        if ctx.errors.len() != before {
            raw.push((None, member.1));
            continue;
        }
        match resolved {
            Ty::Sum(id) => {
                for flattened in ctx.types.sum_members(id).to_vec() {
                    raw.push((Some(flattened), member.1));
                }
            }
            other => raw.push((Some(other), member.1)),
        }
    }
    let outcome = types::dedupe_sum(&raw);
    if outcome.any_unresolved {
        return Ty::Nil;
    }
    for (ty, dup_span) in &outcome.duplicates {
        let name = ctx.name(*ty);
        ctx.error(
            *dup_span,
            format!("'{name}' is repeated in this sum type; each member must be distinct"),
        );
    }
    if outcome.distinct.len() < 2 {
        let msg = if outcome.distinct == [Ty::Nil] {
            types::NIL_NEEDS_A_SUM_SIBLING
        } else {
            types::SUM_TOO_FEW_MEMBERS
        };
        ctx.error(span, msg.to_string());
        return Ty::Nil;
    }
    let mut distinct = outcome.distinct;
    distinct.sort();
    Ty::Sum(ctx.types.intern_sum(distinct))
}
