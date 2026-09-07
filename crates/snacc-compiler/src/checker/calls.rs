//! Calls, methods, constructors, and bridge arguments (Specification 010
//! sections 6.1 and 7; Specification 011).

use super::*;

/// Resolves an expression used as the namespace of an associated-function
/// call. A local binding wins over a type name, matching ordinary call-head
/// resolution, and this never turns values into first-class type objects.
pub(crate) fn static_receiver_type(
    ctx: &Ctx<'_>,
    env: &Env<'_>,
    expression: &Spanned<Expr<'_>>,
) -> Option<Ty> {
    match &expression.0 {
        Expr::BuiltinType(name) => Some(Ty::from(*name)),
        Expr::Local(name) if !env.iter().any(|binding| binding.name == *name) => {
            ctx.types.top_level(name).map(Ty::User)
        }
        Expr::Member(base, (member, _)) => {
            let Expr::Local(root_name) = &base.0 else {
                return None;
            };
            if env.iter().any(|binding| binding.name == *root_name) {
                return None;
            }
            let root = ctx.types.top_level(root_name)?;
            ctx.types.member(root, member).map(Ty::User)
        }
        _ => None,
    }
}

/// Specification 010 section 6.1: resolves one call head. A qualified path
/// whose first component is an in-scope local, parameter, or `self` is a
/// receiver access; that test runs before any type or callable lookup. A bare
/// `name(...)` never calls a local, because Snacc has no function values.
pub(crate) fn check_call<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    callee: &Spanned<Expr<'src>>,
    arguments: &Spanned<Vec<Arg<'src>>>,
) -> Option<CheckedCall> {
    let args = &arguments.0;
    match &callee.0 {
        // `Int64(id)` removes one represented layer.
        Expr::BuiltinType(name) => Some(check_convert(ctx, env, span, Ty::from(*name), args)),
        // A bare `name(...)` resolves only a top-level callable or a type
        // constructor: it never reaches the local binding namespace, because
        // Snacc has no function values.
        Expr::Local(name) => {
            if ctx.sigs.contains_key(*name) {
                return check_function_call(ctx, env, span, name, args);
            }
            if let Some(id) = ctx.types.top_level(name) {
                return Some(check_type_call(ctx, env, span, id, args, callee.1));
            }
            let msg = if env.iter().any(|binding| binding.name == *name) {
                format!(
                    "'{name}' is a variable; Snacc has no function values, so it cannot be called"
                )
            } else {
                format!("'{name}' is not callable")
            };
            ctx.error(callee.1, msg);
            None
        }
        Expr::GenericCall(callee, type_args, arguments) => {
            check_generic_function_call(ctx, env, span, callee, type_args, arguments)
        }
        Expr::Member(base, (member, member_span)) => {
            if let Expr::BuiltinType(TypeName::String) = &base.0
                && matches!(*member, "from_utf8" | "from_unicode")
            {
                return check_string_static(ctx, env, span, member, *member_span, args);
            }
            if let Some(receiver) = static_receiver_type(ctx, env, base) {
                let qualified = format!("{}.{}", ctx.name(receiver), member);
                if ctx.sigs.contains_key(&qualified) {
                    return check_function_call(ctx, env, span, &qualified, args);
                }
            }
            match as_place(ctx, env, base) {
                PlaceOutcome::Resolved(resolved) => {
                    if ctx.move_state.contains_key(&resolved.place.root) {
                        ctx.error(
                            base.1,
                            format!(
                                "'{}' is already moved, so this use is invalid",
                                resolved.place.root
                            ),
                        );
                        return None;
                    }
                    let receiver_ty = resolved.place.ty;
                    if let Some(statement) = check_list_mutation(
                        ctx,
                        env,
                        span,
                        resolved.place.clone(),
                        resolved.mutable,
                        member,
                        args,
                    ) {
                        return Some(statement);
                    }
                    if let Some(statement) = check_map_set_method(
                        ctx,
                        env,
                        span,
                        resolved.place.clone(),
                        resolved.mutable,
                        member,
                        args,
                    ) {
                        return Some(statement);
                    }
                    if let Some(call) = check_builtin_method(
                        ctx,
                        env,
                        span,
                        TExpr::Place(resolved.place.clone(), UseMode::Copy),
                        receiver_ty,
                        member,
                        args,
                    ) {
                        return Some(call);
                    }
                    let self_rooted = resolved.place.root == PlaceRoot::SelfRef;
                    let description = resolved.place.root.to_string();
                    check_method_call(
                        ctx,
                        env,
                        span,
                        TReceiver::Place(resolved.place),
                        receiver_ty,
                        resolved.mutable,
                        self_rooted,
                        description,
                        member,
                        *member_span,
                        args,
                    )
                }
                PlaceOutcome::Reported => None,
                PlaceOutcome::NotAPlace => {
                    // `Union.Member(...)` names a member constructor.
                    if let Expr::Local(first) = &base.0
                        && let Some(root) = ctx.types.top_level(first)
                    {
                        return match ctx.types.member(root, member) {
                            Some(id) => Some(check_type_call(ctx, env, span, id, args, callee.1)),
                            None => {
                                let owner = ctx.types.def(root).name().to_string();
                                ctx.error(
                                    *member_span,
                                    format!("'{owner}' has no member type '{member}'"),
                                );
                                None
                            }
                        };
                    }
                    // Otherwise a method on a computed value.
                    let before = ctx.errors.len();
                    let (value, ty) = check_expr(ctx, env, base);
                    if ctx.errors.len() != before {
                        return None;
                    }
                    if matches!(
                        ty,
                        Ty::String
                            | Ty::ViewByte
                            | Ty::ViewUnicode
                            | Ty::Array(_)
                            | Ty::List(_)
                            | Ty::View(_)
                            | Ty::Map(_)
                            | Ty::Set(_)
                    ) {
                        return check_builtin_method(ctx, env, span, value, ty, member, args);
                    }
                    check_method_call(
                        ctx,
                        env,
                        span,
                        TReceiver::Value(value, ty),
                        ty,
                        false,
                        false,
                        "a temporary".into(),
                        member,
                        *member_span,
                        args,
                    )
                }
            }
        }
        _ => {
            ctx.error(
                callee.1,
                "only calling a function, type, or method by name is supported".into(),
            );
            None
        }
    }
}

/// Checks the closed map/set operation surface. Keys remain restricted by the
/// collection contract, while values may be any fully storable non-borrowed
/// type; the backend selects typed or opaque-byte runtime entry points from
/// the checked value type.
pub(crate) fn check_map_set_method<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    receiver: Place,
    mutable: bool,
    name: &str,
    args: &[Arg<'src>],
) -> Option<CheckedCall> {
    let (kind, key_ty, value_ty) = match receiver.ty {
        Ty::Map(id) => match ctx.types.collection(id) {
            CollectionDef::Map { key, value } => (0u8, *key, Some(*value)),
            _ => unreachable!("map type has non-map metadata"),
        },
        Ty::Set(id) => match ctx.types.collection(id) {
            CollectionDef::Set { elem } => (1u8, *elem, None),
            _ => unreachable!("set type has non-set metadata"),
        },
        _ => return None,
    };
    let supported_key = matches!(
        key_ty,
        Ty::Byte
            | Ty::UInt16
            | Ty::UInt32
            | Ty::UInt64
            | Ty::Int64
            | Ty::Bool
            | Ty::Unicode
            | Ty::String
    );
    if !supported_key {
        ctx.error(
            span,
            if kind == 0 {
                "Map operations require a supported scalar or String key".into()
            } else {
                "Set operations require a supported scalar or String element".into()
            },
        );
        return Some(CheckedCall::Value(TExpr::Nil, Ty::Nil));
    }
    if matches!(name, "insert" | "delete" | "take" | "clear" | "reserve") {
        reject_live_view_source(ctx, &receiver.root, span);
    }
    match (kind, name) {
        (0, "insert") => {
            reject_named_args(ctx, "Map.insert", args);
            if !mutable {
                ctx.error(span, "Map.insert requires a mutable map receiver".into());
            }
            if args.len() != 2 {
                ctx.error(
                    span,
                    format!("Map.insert expects 2 arguments, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
                return Some(CheckedCall::Value(TExpr::Bool(false), Ty::Bool));
            }
            let (key, key_found) = check_expr(ctx, env, &args[0].value);
            if key_found != key_ty {
                ctx.mismatch(args[0].value.1, key_ty, key_found);
            }
            let key = mark_consumed(ctx, env, key, args[0].value.1);
            let (value, found) = check_expr(ctx, env, &args[1].value);
            let value_ty = value_ty.expect("map operation has a value type");
            let value = mark_consumed(ctx, env, value, args[1].value.1);
            let value = coerce(ctx, value, found, value_ty, args[1].value.1);
            Some(CheckedCall::Value(
                TExpr::MapInsert {
                    receiver,
                    key: Box::new(key),
                    value: Box::new(value),
                    key_ty,
                    value_ty,
                    require_existing: false,
                },
                Ty::Bool,
            ))
        }
        (0, "contains") | (0, "delete") | (0, "take") => {
            reject_named_args(ctx, "a map operation", args);
            if args.len() != 1 {
                ctx.error(
                    span,
                    format!("Map.{name} expects 1 argument, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
                return Some(CheckedCall::Value(TExpr::Bool(false), Ty::Bool));
            }
            let (key, found) = check_expr(ctx, env, &args[0].value);
            let (query, query_ty) = if key_ty == Ty::String {
                (
                    coerce(ctx, key, found, Ty::ViewByte, args[0].value.1),
                    Ty::ViewByte,
                )
            } else {
                if found != key_ty {
                    ctx.mismatch(args[0].value.1, key_ty, found);
                }
                (key, found)
            };
            if name != "contains" && !mutable {
                ctx.error(span, format!("Map.{name} requires a mutable map receiver"));
            }
            if name == "contains" {
                Some(CheckedCall::Value(
                    TExpr::MapContains {
                        receiver: Box::new(TExpr::Place(receiver.clone(), UseMode::Copy)),
                        key: Box::new(query),
                        key_ty: query_ty,
                        value_ty: value_ty.expect("map operation has a value type"),
                    },
                    Ty::Bool,
                ))
            } else if name == "delete" {
                Some(CheckedCall::Value(
                    TExpr::MapDelete {
                        receiver,
                        key: Box::new(query),
                        key_ty: query_ty,
                        value_ty: value_ty.expect("map operation has a value type"),
                    },
                    Ty::Bool,
                ))
            } else {
                Some(CheckedCall::Value(
                    TExpr::MapTake {
                        receiver,
                        key: Box::new(query),
                        key_ty: query_ty,
                        value_ty: value_ty.expect("map operation has a value type"),
                    },
                    value_ty.expect("map operation has a value type"),
                ))
            }
        }
        (0, "clear") => {
            reject_named_args(ctx, "Map.clear", args);
            if !mutable {
                ctx.error(span, "Map.clear requires a mutable map receiver".into());
            }
            if !args.is_empty() {
                ctx.error(
                    span,
                    format!("Map.clear expects no arguments, found {}", args.len()),
                );
            }
            Some(CheckedCall::Statement(TStmt::MapClear {
                receiver,
                key_ty,
                value_ty: value_ty.expect("map operation has a value type"),
            }))
        }
        (0, "reserve") => {
            reject_named_args(ctx, "Map.reserve", args);
            if !mutable {
                ctx.error(span, "Map.reserve requires a mutable map receiver".into());
            }
            if args.len() != 1 {
                ctx.error(
                    span,
                    format!("Map.reserve expects 1 argument, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
                return Some(CheckedCall::Statement(TStmt::MapReserve {
                    receiver,
                    minimum: TExpr::Num(NumLiteral::Int(0)),
                    key_ty,
                    value_ty: value_ty.expect("map operation has a value type"),
                }));
            }
            let (minimum, minimum_ty) = check_expr(ctx, env, &args[0].value);
            if minimum_ty != Ty::Int64 {
                ctx.mismatch(args[0].value.1, Ty::Int64, minimum_ty);
            }
            Some(CheckedCall::Statement(TStmt::MapReserve {
                receiver,
                minimum,
                key_ty,
                value_ty: value_ty.expect("map operation has a value type"),
            }))
        }
        (1, "insert") | (1, "delete") | (1, "contains") => {
            reject_named_args(ctx, "a set operation", args);
            if args.len() != 1 {
                ctx.error(
                    span,
                    format!("Set.{name} expects 1 argument, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
                return Some(CheckedCall::Value(TExpr::Bool(false), Ty::Bool));
            }
            let (value, found) = check_expr(ctx, env, &args[0].value);
            let value = if key_ty == Ty::String {
                if found != Ty::String {
                    ctx.mismatch(args[0].value.1, Ty::String, found);
                }
                if name == "insert" {
                    mark_consumed(ctx, env, value, args[0].value.1)
                } else {
                    value
                }
            } else {
                if found != key_ty {
                    ctx.mismatch(args[0].value.1, key_ty, found);
                }
                value
            };
            if name != "contains" && !mutable {
                ctx.error(span, format!("Set.{name} requires a mutable set receiver"));
            }
            if name == "contains" || name == "delete" {
                let query = if key_ty == Ty::String {
                    coerce(ctx, value, found, Ty::ViewByte, args[0].value.1)
                } else {
                    value
                };
                if name == "contains" {
                    Some(CheckedCall::Value(
                        TExpr::SetContains {
                            receiver: Box::new(TExpr::Place(receiver, UseMode::Copy)),
                            value: Box::new(query),
                            elem: key_ty,
                        },
                        Ty::Bool,
                    ))
                } else {
                    Some(CheckedCall::Value(
                        TExpr::SetDelete {
                            receiver,
                            value: Box::new(query),
                            elem: key_ty,
                        },
                        Ty::Bool,
                    ))
                }
            } else if name == "insert" {
                Some(CheckedCall::Value(
                    TExpr::SetInsert {
                        receiver,
                        value: Box::new(value),
                        elem: key_ty,
                    },
                    Ty::Bool,
                ))
            } else {
                unreachable!("set operation name was checked above")
            }
        }
        (1, "clear") => {
            reject_named_args(ctx, "Set.clear", args);
            if !mutable {
                ctx.error(span, "Set.clear requires a mutable set receiver".into());
            }
            if !args.is_empty() {
                ctx.error(
                    span,
                    format!("Set.clear expects no arguments, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
            }
            Some(CheckedCall::Statement(TStmt::SetClear {
                receiver,
                elem: key_ty,
            }))
        }
        (1, "reserve") => {
            reject_named_args(ctx, "Set.reserve", args);
            if !mutable {
                ctx.error(span, "Set.reserve requires a mutable set receiver".into());
            }
            if args.len() != 1 {
                ctx.error(
                    span,
                    format!("Set.reserve expects 1 argument, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
                return Some(CheckedCall::Statement(TStmt::SetReserve {
                    receiver,
                    minimum: TExpr::Num(NumLiteral::Int(0)),
                    elem: key_ty,
                }));
            }
            let (minimum, minimum_ty) = check_expr(ctx, env, &args[0].value);
            if minimum_ty != Ty::Int64 {
                ctx.mismatch(args[0].value.1, Ty::Int64, minimum_ty);
            }
            Some(CheckedCall::Statement(TStmt::SetReserve {
                receiver,
                minimum,
                elem: key_ty,
            }))
        }
        _ => None,
    }
}

/// Checks the deliberately closed mutation surface exposed for lists. Scalar
/// elements use typed runtime entry points; all other storable elements use
/// the compiler-generated ownership descriptor and opaque-byte entry points.
pub(crate) fn check_list_mutation<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    receiver: Place,
    mutable: bool,
    name: &str,
    args: &[Arg<'src>],
) -> Option<CheckedCall> {
    let Ty::List(id) = receiver.ty else {
        return None;
    };
    let elem = match ctx.types.collection(id) {
        CollectionDef::List { elem } => *elem,
        _ => unreachable!("list type has non-list metadata"),
    };
    if matches!(
        name,
        "push" | "pop" | "insert" | "remove" | "clear" | "reserve"
    ) {
        reject_live_view_source(ctx, &receiver.root, span);
    }
    match name {
        "push" => {
            reject_named_args(ctx, "a List.push call", args);
            if !mutable {
                ctx.error(span, "List.push requires a mutable list receiver".into());
            }
            if args.len() != 1 {
                ctx.error(
                    span,
                    format!("List.push expects 1 argument, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
                return Some(CheckedCall::Statement(TStmt::ListPush {
                    receiver,
                    value: TExpr::Nil,
                    elem,
                }));
            }
            let (value, value_ty) = check_expr(ctx, env, &args[0].value);
            let value = mark_consumed(ctx, env, value, args[0].value.1);
            let value = coerce(ctx, value, value_ty, elem, args[0].value.1);
            Some(CheckedCall::Statement(TStmt::ListPush {
                receiver,
                value,
                elem,
            }))
        }
        "pop" => {
            reject_named_args(ctx, "a List.pop call", args);
            if !mutable {
                ctx.error(span, "List.pop requires a mutable list receiver".into());
            }
            if !args.is_empty() {
                ctx.error(
                    span,
                    format!("List.pop expects no arguments, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
            }
            Some(CheckedCall::Value(TExpr::ListPop { receiver, elem }, elem))
        }
        "remove" => {
            reject_named_args(ctx, "a List.remove call", args);
            if !mutable {
                ctx.error(span, "List.remove requires a mutable list receiver".into());
            }
            if args.len() != 1 {
                ctx.error(
                    span,
                    format!("List.remove expects 1 argument, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
                return Some(CheckedCall::Value(
                    TExpr::ListRemove {
                        receiver,
                        index: Box::new(TExpr::Num(NumLiteral::Int(0))),
                        elem,
                    },
                    elem,
                ));
            }
            let (index, index_ty) = check_expr(ctx, env, &args[0].value);
            if index_ty != Ty::Int64 {
                ctx.mismatch(args[0].value.1, Ty::Int64, index_ty);
            }
            Some(CheckedCall::Value(
                TExpr::ListRemove {
                    receiver,
                    index: Box::new(index),
                    elem,
                },
                elem,
            ))
        }
        "insert" => {
            reject_named_args(ctx, "a List.insert call", args);
            if !mutable {
                ctx.error(span, "List.insert requires a mutable list receiver".into());
            }
            if args.len() != 2 {
                ctx.error(
                    span,
                    format!("List.insert expects 2 arguments, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
                return Some(CheckedCall::Statement(TStmt::ListInsert {
                    receiver,
                    index: TExpr::Num(NumLiteral::Int(0)),
                    value: TExpr::Nil,
                    elem,
                }));
            }
            let (index, index_ty) = check_expr(ctx, env, &args[0].value);
            if index_ty != Ty::Int64 {
                ctx.mismatch(args[0].value.1, Ty::Int64, index_ty);
            }
            let (value, value_ty) = check_expr(ctx, env, &args[1].value);
            let value = mark_consumed(ctx, env, value, args[1].value.1);
            let value = coerce(ctx, value, value_ty, elem, args[1].value.1);
            Some(CheckedCall::Statement(TStmt::ListInsert {
                receiver,
                index,
                value,
                elem,
            }))
        }
        "reserve" => {
            reject_named_args(ctx, "a List.reserve call", args);
            if !mutable {
                ctx.error(span, "List.reserve requires a mutable list receiver".into());
            }
            if args.len() != 1 {
                ctx.error(
                    span,
                    format!("List.reserve expects 1 argument, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
                return Some(CheckedCall::Statement(TStmt::ListReserve {
                    receiver,
                    minimum: TExpr::Num(NumLiteral::Int(0)),
                    elem,
                }));
            }
            let (minimum, minimum_ty) = check_expr(ctx, env, &args[0].value);
            if minimum_ty != Ty::Int64 {
                ctx.mismatch(args[0].value.1, Ty::Int64, minimum_ty);
            }
            Some(CheckedCall::Statement(TStmt::ListReserve {
                receiver,
                minimum,
                elem,
            }))
        }
        "clear" => {
            reject_named_args(ctx, "a List.clear call", args);
            if !mutable {
                ctx.error(span, "List.clear requires a mutable list receiver".into());
            }
            if !args.is_empty() {
                ctx.error(
                    span,
                    format!("List.clear expects no arguments, found {}", args.len()),
                );
                for arg in args {
                    check_expr(ctx, env, &arg.value);
                }
            }
            Some(CheckedCall::Statement(TStmt::ListClear { receiver, elem }))
        }
        _ => None,
    }
}

pub(crate) fn check_builtin_method<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    receiver: TExpr,
    receiver_ty: Ty,
    name: &str,
    args: &[Arg<'src>],
) -> Option<CheckedCall> {
    if !matches!(
        receiver_ty,
        Ty::String
            | Ty::ViewByte
            | Ty::ViewUnicode
            | Ty::Array(_)
            | Ty::List(_)
            | Ty::View(_)
            | Ty::Map(_)
            | Ty::Set(_)
    ) {
        return None;
    }
    match name {
        "bytes" if receiver_ty == Ty::String && args.is_empty() => Some(CheckedCall::Value(
            TExpr::ViewFromString(Box::new(receiver), Ty::ViewByte),
            Ty::ViewByte,
        )),
        "unicode" if receiver_ty == Ty::String && args.is_empty() => Some(CheckedCall::Value(
            TExpr::ViewFromString(Box::new(receiver), Ty::ViewUnicode),
            Ty::ViewUnicode,
        )),
        "length" if matches!(receiver_ty, Ty::ViewByte | Ty::ViewUnicode) && args.is_empty() => {
            Some(CheckedCall::Value(
                TExpr::ViewLength(Box::new(receiver), receiver_ty),
                Ty::Int64,
            ))
        }
        "length"
            if matches!(
                receiver_ty,
                Ty::Array(_) | Ty::List(_) | Ty::View(_) | Ty::Map(_) | Ty::Set(_)
            ) && args.is_empty() =>
        {
            Some(CheckedCall::Value(
                TExpr::CollectionLength(Box::new(receiver)),
                Ty::Int64,
            ))
        }
        "is_empty"
            if matches!(
                receiver_ty,
                Ty::Array(_) | Ty::List(_) | Ty::View(_) | Ty::Map(_) | Ty::Set(_)
            ) && args.is_empty() =>
        {
            Some(CheckedCall::Value(
                TExpr::CollectionIsEmpty(Box::new(receiver)),
                Ty::Bool,
            ))
        }
        "capacity" if matches!(receiver_ty, Ty::List(_)) && args.is_empty() => Some(
            CheckedCall::Value(TExpr::CollectionCapacity(Box::new(receiver)), Ty::Int64),
        ),
        "view" if matches!(receiver_ty, Ty::Array(_) | Ty::List(_)) && args.is_empty() => {
            let elem = match receiver_ty {
                Ty::Array(id) | Ty::List(id) => match ctx.types.collection(id) {
                    CollectionDef::Array { elem, .. } | CollectionDef::List { elem } => *elem,
                    _ => unreachable!("sequence type has non-sequence metadata"),
                },
                _ => unreachable!("guarded by receiver type"),
            };
            let view = Ty::View(ctx.types.intern_collection(CollectionDef::View { elem }));
            Some(CheckedCall::Value(
                TExpr::CollectionView(Box::new(receiver), view),
                view,
            ))
        }
        "at" if receiver_ty == Ty::ViewByte && args.len() == 1 && args[0].name.is_none() => {
            let (index, index_ty) = check_expr(ctx, env, &args[0].value);
            if index_ty != Ty::Int64 {
                ctx.mismatch(args[0].value.1, Ty::Int64, index_ty);
            }
            let sum = ctx.types.intern_sum(vec![Ty::Nil, Ty::Byte]);
            Some(CheckedCall::Value(
                TExpr::ViewAt(Box::new(receiver), Box::new(index), Ty::ViewByte, sum),
                Ty::Sum(sum),
            ))
        }
        "scalar_at"
            if receiver_ty == Ty::ViewUnicode && args.len() == 1 && args[0].name.is_none() =>
        {
            let (index, index_ty) = check_expr(ctx, env, &args[0].value);
            if index_ty != Ty::Int64 {
                ctx.mismatch(args[0].value.1, Ty::Int64, index_ty);
            }
            let sum = ctx.types.intern_sum(vec![Ty::Nil, Ty::Unicode]);
            Some(CheckedCall::Value(
                TExpr::ViewAt(Box::new(receiver), Box::new(index), Ty::ViewUnicode, sum),
                Ty::Sum(sum),
            ))
        }
        "slice"
            if matches!(receiver_ty, Ty::ViewByte | Ty::ViewUnicode)
                && args.len() == 2
                && args.iter().all(|arg| arg.name.is_none()) =>
        {
            let (start, start_ty) = check_expr(ctx, env, &args[0].value);
            let (end, end_ty) = check_expr(ctx, env, &args[1].value);
            if start_ty != Ty::Int64 {
                ctx.mismatch(args[0].value.1, Ty::Int64, start_ty);
            }
            if end_ty != Ty::Int64 {
                ctx.mismatch(args[1].value.1, Ty::Int64, end_ty);
            }
            let sum = ctx.types.intern_sum(vec![Ty::Nil, receiver_ty]);
            Some(CheckedCall::Value(
                TExpr::ViewSlice {
                    value: Box::new(receiver),
                    start: Box::new(start),
                    end: Box::new(end),
                    view_ty: receiver_ty,
                    sum,
                },
                Ty::Sum(sum),
            ))
        }
        "slice"
            if matches!(receiver_ty, Ty::Array(_) | Ty::List(_) | Ty::View(_))
                && args.len() == 2
                && args.iter().all(|arg| arg.name.is_none()) =>
        {
            let elem = match receiver_ty {
                Ty::Array(id) | Ty::List(id) | Ty::View(id) => match ctx.types.collection(id) {
                    CollectionDef::Array { elem, .. }
                    | CollectionDef::List { elem }
                    | CollectionDef::View { elem } => *elem,
                    _ => unreachable!("sequence view metadata is not a sequence"),
                },
                _ => unreachable!("guarded by collection receiver type"),
            };
            let view_ty = match receiver_ty {
                Ty::View(_) => receiver_ty,
                Ty::Array(_) | Ty::List(_) => {
                    Ty::View(ctx.types.intern_collection(CollectionDef::View { elem }))
                }
                _ => unreachable!("guarded by collection receiver type"),
            };
            let (start, start_ty) = check_expr(ctx, env, &args[0].value);
            let (end, end_ty) = check_expr(ctx, env, &args[1].value);
            if start_ty != Ty::Int64 {
                ctx.mismatch(args[0].value.1, Ty::Int64, start_ty);
            }
            if end_ty != Ty::Int64 {
                ctx.mismatch(args[1].value.1, Ty::Int64, end_ty);
            }
            let sum = ctx.types.intern_sum(vec![Ty::Nil, view_ty]);
            Some(CheckedCall::Value(
                TExpr::CollectionSlice {
                    value: Box::new(receiver),
                    start: Box::new(start),
                    end: Box::new(end),
                    view_ty,
                    sum,
                    elem,
                },
                Ty::Sum(sum),
            ))
        }
        "clone" if receiver_ty == Ty::String && args.is_empty() => Some(CheckedCall::Value(
            TExpr::StringClone(Box::new(receiver)),
            Ty::String,
        )),
        "concat" if receiver_ty == Ty::String && args.len() == 1 && args[0].name.is_none() => {
            let (value, ty) = check_expr(ctx, env, &args[0].value);
            let accepted = matches!(
                ty,
                Ty::String
                    | Ty::ViewUnicode
                    | Ty::Unicode
                    | Ty::Byte
                    | Ty::UInt16
                    | Ty::UInt32
                    | Ty::UInt64
                    | Ty::Int64
                    | Ty::Float32
                    | Ty::Float64
                    | Ty::Bool
            );
            if !accepted {
                ctx.error(
                    args[0].value.1,
                    format!(
                        "String.concat does not accept '{}'; expected a text or scalar part",
                        ctx.name(ty)
                    ),
                );
            }
            let mut parts = match receiver {
                TExpr::StringConcat(parts) => parts,
                value => vec![TStringPart {
                    value,
                    ty: Ty::String,
                }],
            };
            parts.push(TStringPart { value, ty });
            Some(CheckedCall::Value(TExpr::StringConcat(parts), Ty::String))
        }
        "clone" | "concat" => {
            ctx.error(span, format!("String.{name} called with invalid arguments"));
            Some(CheckedCall::Value(TExpr::Nil, Ty::String))
        }
        _ => None,
    }
}

pub(crate) fn check_string_static<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    name: &str,
    member_span: Span,
    args: &[Arg<'src>],
) -> Option<CheckedCall> {
    if args.len() != 1 || args[0].name.is_some() {
        ctx.error(
            member_span,
            format!("String.{name} expects one positional view argument"),
        );
        return Some(CheckedCall::Value(TExpr::Nil, Ty::String));
    }
    let (value, ty) = check_expr(ctx, env, &args[0].value);
    match (name, ty) {
        ("from_unicode", Ty::ViewUnicode) => Some(CheckedCall::Value(
            TExpr::StringFromUnicode(Box::new(value)),
            Ty::String,
        )),
        ("from_utf8", Ty::ViewByte) => {
            let sum = ctx.types.intern_sum(vec![Ty::Nil, Ty::String]);
            Some(CheckedCall::Value(
                TExpr::StringFromUtf8(Box::new(value), sum),
                Ty::Sum(sum),
            ))
        }
        (_, found) => {
            ctx.error(
                span,
                format!("String.{name} received unsupported argument type '{}', expected a matching view", ctx.name(found)),
            );
            Some(CheckedCall::Value(TExpr::Nil, Ty::String))
        }
    }
}

/// Rejects the named-argument form outside struct construction.
pub(crate) fn reject_named_args(ctx: &mut Ctx<'_>, what: &str, args: &[Arg<'_>]) {
    for arg in args {
        if let Some((name, span)) = arg.name {
            ctx.error(
                span,
                format!(
                    "named argument '{name}' is not valid for {what}; named arguments \
                     are only used to construct a struct"
                ),
            );
        }
    }
}

pub(crate) fn check_function_call<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    name: &str,
    args: &[Arg<'src>],
) -> Option<CheckedCall> {
    let signature = ctx.sigs.get(name).cloned()?;
    reject_named_args(ctx, "a function call", args);
    if signature.params.len() != args.len() {
        ctx.error(
            span,
            format!(
                "'{name}' called with wrong number of arguments (expected {}, found {})",
                signature.params.len(),
                args.len()
            ),
        );
    }
    let values = check_args(
        ctx,
        env,
        args,
        &signature.params,
        name,
        None,
        ctx.externs.contains(name),
    );
    Some(CheckedCall::Function {
        name: name.to_string(),
        args: values,
        result: signature.result,
    })
}

pub(crate) fn check_generic_function_call<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    callee: &Spanned<Expr<'src>>,
    type_args: &Spanned<Vec<Spanned<TypeRef<'src>>>>,
    args: &Spanned<Vec<Arg<'src>>>,
) -> Option<CheckedCall> {
    let Expr::Local(name) = &callee.0 else {
        ctx.error(
            callee.1,
            "generic calls require an unqualified top-level function name".into(),
        );
        return None;
    };
    let function = {
        let generic_funcs = &ctx.generic_funcs;
        generic_funcs.get(name).copied()
    };
    let Some(function) = function else {
        let concrete: Vec<Ty> = type_args.0.iter().map(|ty| resolve_type(ctx, ty)).collect();
        if let Some(id) = instantiate_generic_type(ctx, name, &concrete, type_args.1) {
            return Some(check_type_call(ctx, env, span, id, &args.0, callee.1));
        }
        ctx.error(
            callee.1,
            format!("'{name}' is not a generic function or type"),
        );
        return None;
    };
    if function.generic_params.len() != type_args.0.len() {
        ctx.error(
            type_args.1,
            format!(
                "generic function '{name}' expects {} type arguments, found {}",
                function.generic_params.len(),
                type_args.0.len()
            ),
        );
        for arg in &args.0 {
            check_expr(ctx, env, &arg.value);
        }
        return None;
    }
    let concrete: Vec<Ty> = type_args.0.iter().map(|ty| resolve_type(ctx, ty)).collect();
    let substitutions: HashMap<&str, Ty> = function
        .generic_params
        .iter()
        .zip(&concrete)
        .map(|((param, _), ty)| (*param, *ty))
        .collect();
    let previous = std::mem::replace(&mut ctx.generic_subst, substitutions.clone());
    let params = resolve_generic_params(ctx, &function.args);
    let result = function.ret.as_ref().map(|ty| resolve_type(ctx, ty));
    ctx.generic_subst = previous;
    let mangled = generic_name(ctx, name, &concrete);
    let display = format!(
        "{name}<{}>",
        concrete
            .iter()
            .map(|ty| ctx.name(*ty))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut chain = ctx.generic_chain.clone();
    chain.push(display);
    let signature = FuncSig {
        params: params.clone(),
        result,
    };
    ctx.sigs.insert(mangled.clone(), signature);
    if !ctx.generic_seen.contains(&mangled) {
        if ctx.generic_depth >= MAX_SPECIALIZATION_DEPTH {
            ctx.error(
                span,
                format!(
                    "generic specialization depth exceeds {MAX_SPECIALIZATION_DEPTH}; instantiation chain: {}",
                    chain.join(" -> ")
                ),
            );
        } else if ctx.specialization_count >= MAX_SPECIALIZATIONS {
            ctx.error(
                span,
                format!(
                    "generic specialization limit exceeded (maximum {MAX_SPECIALIZATIONS}); instantiation chain: {}",
                    chain.join(" -> ")
                ),
            );
        } else {
            ctx.generic_seen.insert(mangled.clone());
            ctx.specialization_count += 1;
            ctx.generic_queue.push(GenericRequest {
                name: (*name).to_string(),
                args: concrete,
                depth: ctx.generic_depth + 1,
                use_span: span,
                chain,
            });
        }
    }
    reject_named_args(ctx, "a function call", &args.0);
    let values = check_args(ctx, env, &args.0, &params, &mangled, None, false);
    Some(CheckedCall::Function {
        name: mangled,
        args: values,
        result,
    })
}

/// Specification 011 sections 6.1-6.4. Arguments are processed left to right;
/// a value argument is checked and coerced as before, and a reference argument
/// resolves to exactly one place, which is then validated for mutability, exact
/// referent type, and disjointness from every other reference in the same call.
pub(crate) fn check_args<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    args: &[Arg<'src>],
    params: &[TParam],
    callee: &str,
    receiver: Option<&Place>,
    bridge: bool,
) -> Vec<TArg> {
    let mut checked = Vec::with_capacity(args.len());
    let mut references: Vec<(String, Place, Span)> = Vec::new();
    let mut moves: Vec<(String, Place, Span)> = Vec::new();
    let mut lends: Vec<(String, Place, Span)> = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        // An argument with no parameter is already reported as an arity error;
        // it is still checked so its own diagnostics are not swallowed.
        let Some(param) = params.get(index) else {
            checked.push(TArg::Value(check_expr(ctx, env, &arg.value).0));
            continue;
        };
        match param.mode {
            ParamMode::Value => {
                let (mut value, mut ty) = check_expr(ctx, env, &arg.value);
                if let Some((lent, place)) = lend_call_view(ctx, &value, ty, param.ty) {
                    lends.push((param.name.clone(), place, arg.value.1));
                    value = lent;
                    ty = param.ty;
                } else {
                    // Specification 016 section 6.1: an ordinary by-value
                    // argument consumes a move-only owning root.
                    value = mark_consumed(ctx, env, value, arg.value.1);
                }
                // Specification 016 section 7.2's closing sentence: a
                // borrowed allocation cannot be simultaneously moved, so a
                // whole-root move-only argument joins the same overlap
                // check as a reference argument in this same call.
                if let TExpr::Place(place, UseMode::Consume) = &value
                    && place.path.is_empty()
                    && ctx.types.is_move_only(place.ty)
                {
                    moves.push((param.name.clone(), place.clone(), arg.value.1));
                }
                let value = if bridge {
                    coerce_bridge_view(ctx, value, ty, param.ty, arg.value.1)
                } else {
                    coerce(ctx, value, ty, param.ty, arg.value.1)
                };
                checked.push(TArg::Value(value));
            }
            ParamMode::Reference => match check_reference_arg(ctx, env, arg, param, callee) {
                Some(place) => {
                    references.push((param.name.clone(), place.clone(), arg.value.1));
                    checked.push(TArg::Reference(place));
                }
                None => checked.push(TArg::Value(TExpr::Nil)),
            },
        }
    }
    reject_overlap(ctx, &references, &moves, receiver);
    for (name, lent, span) in &lends {
        for (moved_name, moved, _) in &moves {
            if overlaps(lent, moved) {
                ctx.error(
                    *span,
                    format!(
                        "view argument '{}' for parameter '{name}' overlaps the moved argument '{}' for parameter '{moved_name}'",
                        ctx.place_name(lent),
                        ctx.place_name(moved)
                    ),
                );
            }
        }
    }
    checked
}

/// Resolves one reference argument. A reference parameter forwarded from an
/// enclosing signature needs no special case: it is already a mutable root of
/// its referent type, so it reborrows through this same path.
pub(crate) fn check_reference_arg<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    arg: &Arg<'src>,
    param: &TParam,
    callee: &str,
) -> Option<Place> {
    let span = arg.value.1;
    match as_place(ctx, env, &arg.value) {
        PlaceOutcome::Resolved(resolved) => {
            if !resolved.mutable {
                let root = resolved.place.root.to_string();
                let name = &param.name;
                ctx.error(
                    span,
                    format!(
                        "'{root}' is not declared 'mut', so it cannot be passed to the \
                         reference parameter '{name}' of '{callee}'"
                    ),
                );
            }
            let mut place = resolved.place;
            // Specification 016 section 7.2: a `Box<T>` argument place lends
            // its pointee to a `Ref<T>` parameter automatically. A `Box<T>`
            // argument binding to a declared `Ref<Box<T>>` parameter instead
            // is already an exact match below and needs no special case --
            // the expected parameter type alone disambiguates, with no new
            // inference. The overlap check afterward still compares this
            // place's unchanged root and path, so two lends of the same
            // allocation (as the box or as its pointee) still overlap.
            if place.ty != param.ty
                && let Ty::Box(id) = place.ty
                && ctx.types.box_pointee(id) == param.ty
            {
                place.ty = param.ty;
            } else if place.ty != param.ty {
                // Specification 011 sections 6.1 and 9: exactly `T`. Neither
                // the `Int64`-to-`Float64` widening nor represented-type
                // equivalence applies, because the callee addresses the
                // caller's own storage.
                let expected = ctx.name(param.ty);
                let found = ctx.name(place.ty);
                let name = &param.name;
                ctx.error(
                    span,
                    format!(
                        "reference parameter '{name}' of '{callee}' requires a place of \
                         exactly type '{expected}', found '{found}'"
                    ),
                );
            }
            // Handing a `self`-rooted place to a reference parameter may write
            // it, exactly as an assignment to that place would, so it feeds the
            // same receiver-write fixed point (Specification 010 section 19
            // phase 4). Without this a caller could pass an immutable receiver.
            if place.root == PlaceRoot::SelfRef
                && let Some(method) = ctx.current_method
            {
                ctx.direct_writes[method.index()] = true;
            }
            Some(place)
        }
        PlaceOutcome::Reported => None,
        PlaceOutcome::NotAPlace => {
            // A malformed argument reports its own error first; only a
            // well-formed value -- a literal, a call result, arithmetic -- needs
            // the "not a place" diagnostic.
            let before = ctx.errors.len();
            check_expr(ctx, env, &arg.value);
            if ctx.errors.len() == before {
                let expected = ctx.name(param.ty);
                let name = &param.name;
                ctx.error(
                    span,
                    format!(
                        "reference parameter '{name}' of '{callee}' requires an initialized \
                         mutable place of type '{expected}', but this argument is a value \
                         with no storage"
                    ),
                );
            }
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn check_method_call<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    receiver: TReceiver,
    receiver_ty: Ty,
    mutable_root: bool,
    self_rooted: bool,
    description: String,
    name: &str,
    name_span: Span,
    args: &[Arg<'src>],
) -> Option<CheckedCall> {
    // Specification 016 section 4.3: a method call on a box automatically
    // dereferences to the pointee before resolution, exactly like field
    // access. `receiver_ty` itself is left as the caller's static type;
    // `TReceiver::Place`'s place is unaffected, so lowering (Task C) still
    // knows the receiver storage is boxed.
    let receiver_ty = deref_box(&ctx.types, receiver_ty);
    let Ty::User(id) = receiver_ty else {
        let owner = ctx.name(receiver_ty);
        ctx.error(name_span, format!("'{owner}' has no method '{name}'"));
        return None;
    };
    let Some(method) = ctx.method_index.get(&(id, name.to_string())).copied() else {
        let owner = ctx.types.def(id).name().to_string();
        let msg = if ctx.types.field(id, name).is_some() {
            format!("'{owner}.{name}' is a field, not a method, so it cannot be called")
        } else {
            format!("'{owner}' has no method '{name}'")
        };
        ctx.error(name_span, msg);
        return None;
    };
    reject_named_args(ctx, "a method call", args);
    let signature_params = ctx.method_sigs[method.index()].params.clone();
    let result = ctx.method_sigs[method.index()].result;
    if signature_params.len() != args.len() {
        let qualified = ctx.method_name(method);
        ctx.error(
            span,
            format!(
                "'{qualified}' called with wrong number of arguments (expected {}, found {})",
                signature_params.len(),
                args.len()
            ),
        );
    }
    // Specification 011 section 6.4: an addressable receiver participates in
    // overlap checking for the complete call, whether or not the method writes.
    let receiver_place = match &receiver {
        TReceiver::Place(place) => Some(place.clone()),
        TReceiver::Value(..) => None,
    };
    let qualified = ctx.method_name(method);
    let values = check_args(
        ctx,
        env,
        args,
        &signature_params,
        &qualified,
        receiver_place.as_ref(),
        false,
    );
    // The effect is not known yet, so the receiver check is deferred to the
    // fixed point (Specification 010 section 19 phase 4).
    ctx.receiver_calls.push(ReceiverCall {
        method,
        mutable_root: mutable_root && matches!(receiver, TReceiver::Place(_)),
        receiver: description,
        span,
    });
    if self_rooted && let Some(caller) = ctx.current_method {
        ctx.effect_edges.push((caller, method));
    }
    Some(CheckedCall::Method {
        call: TMethodCall {
            receiver,
            method,
            args: values,
        },
        result,
    })
}

/// Calling a user type: struct or member construction, or one represented
/// wrap/unwrap layer.
pub(crate) fn check_type_call<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    id: TypeId,
    args: &[Arg<'src>],
    head_span: Span,
) -> CheckedCall {
    enum Head {
        Represented,
        Union(String),
        Fields,
    }
    let head = match ctx.types.def(id) {
        TypeDef::Represented { .. } => Head::Represented,
        TypeDef::Union { name, .. } => Head::Union(name.clone()),
        TypeDef::Struct { .. } | TypeDef::UnionMember { .. } => Head::Fields,
    };
    match head {
        Head::Represented => check_convert(ctx, env, span, Ty::User(id), args),
        Head::Union(name) => {
            ctx.error(
                head_span,
                format!(
                    "'{name}' is a union; construction names one member type, not the \
                     union itself"
                ),
            );
            for arg in args {
                check_expr(ctx, env, &arg.value);
            }
            CheckedCall::Value(TExpr::Nil, Ty::Nil)
        }
        Head::Fields => check_construct(ctx, env, span, id, args),
    }
}

/// Specification 010 section 7.2: exactly one layer, exact immediate type, no
/// named arguments, and no general numeric cast.
pub(crate) fn check_convert<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    to: Ty,
    args: &[Arg<'src>],
) -> CheckedCall {
    reject_named_args(ctx, "a represented-type conversion", args);
    let [arg] = args else {
        let name = ctx.name(to);
        ctx.error(
            span,
            format!(
                "'{name}' converts exactly one positional value, but {} were supplied",
                args.len()
            ),
        );
        for arg in args {
            check_expr(ctx, env, &arg.value);
        }
        return CheckedCall::Value(TExpr::Nil, to);
    };
    let (value, from) = check_expr(ctx, env, &arg.value);
    let wraps = matches!(to, Ty::User(id) if ctx.types.represented_target(id) == Some(from));
    let unwraps = matches!(from, Ty::User(id) if ctx.types.represented_target(id) == Some(to));
    if !wraps && !unwraps {
        let target = ctx.name(to);
        let found = ctx.name(from);
        let msg = match to {
            Ty::User(id) => {
                let target_ty = ctx.types.represented_target(id);
                let immediate = target_ty
                    .map(|ty| ctx.name(ty))
                    .unwrap_or_else(|| target.clone());
                format!(
                    "'{target}' wraps exactly its immediate representation '{immediate}', \
                     found '{found}'"
                )
            }
            _ => format!(
                "'{target}' unwraps exactly one value of a type represented by \
                 '{target}', found '{found}'"
            ),
        };
        ctx.error(arg.value.1, msg);
    }
    CheckedCall::Value(
        TExpr::Represent {
            value: Box::new(value),
            ty: to,
        },
        to,
    )
}

/// Specification 010 sections 8.2-8.3: named fields, each exactly once, checked
/// and evaluated in written order.
pub(crate) fn check_construct<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    span: Span,
    id: TypeId,
    args: &[Arg<'src>],
) -> CheckedCall {
    let type_name = ctx.types.def(id).name().to_string();
    let fields: Vec<(String, Ty)> = ctx
        .types
        .def(id)
        .fields()
        .expect("a constructed type has fields")
        .to_vec();
    if fields.is_empty() {
        if !args.is_empty() {
            ctx.error(
                span,
                format!("'{type_name}' has no fields, so it is constructed with '()'"),
            );
        }
        return CheckedCall::Value(
            TExpr::Construct {
                type_id: id,
                fields: Vec::new(),
            },
            Ty::User(id),
        );
    }

    let mut checked: Vec<(usize, TExpr)> = Vec::new();
    let mut filled = vec![false; fields.len()];
    for arg in args {
        let Some((name, name_span)) = arg.name else {
            ctx.error(
                arg.value.1,
                format!(
                    "'{type_name}' requires named fields; positional construction of a \
                     non-empty struct is invalid"
                ),
            );
            check_expr(ctx, env, &arg.value);
            continue;
        };
        let Some(index) = fields.iter().position(|(field, _)| field == name) else {
            ctx.error(name_span, format!("'{type_name}' has no field '{name}'"));
            check_expr(ctx, env, &arg.value);
            continue;
        };
        // Arguments evaluate left to right in written order; the destination
        // index travels with the value so lowering can store in field order.
        let (value, ty) = check_expr(ctx, env, &arg.value);
        if filled[index] {
            ctx.error(
                name_span,
                format!("Field '{type_name}.{name}' is supplied more than once"),
            );
            continue;
        }
        filled[index] = true;
        let expected = fields[index].1;
        // Specification 016 section 6.1: an aggregate constructor argument is
        // a consuming context.
        let value = mark_consumed(ctx, env, value, arg.value.1);
        let value = coerce(ctx, value, ty, expected, arg.value.1);
        checked.push((index, value));
    }

    let missing: Vec<&str> = fields
        .iter()
        .zip(&filled)
        .filter(|(_, supplied)| !**supplied)
        .map(|((name, _), _)| name.as_str())
        .collect();
    if !missing.is_empty() {
        ctx.error(
            span,
            format!(
                "'{type_name}' is missing field {}",
                missing
                    .iter()
                    .map(|name| format!("'{name}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    CheckedCall::Value(
        TExpr::Construct {
            type_id: id,
            fields: checked,
        },
        Ty::User(id),
    )
}

/// Specification 010 section 12: the subject is a place with a union type and
/// the tested type is one direct member of that union. The caller
/// (`check_arm_condition`) has already resolved `place` and confirmed `union`
/// names a union.
pub(crate) fn check_type_test<'src>(
    ctx: &mut Ctx<'src>,
    test: &TypeTest<'src>,
    place: Place,
    union: TypeId,
) -> Option<TTypeTest> {
    let subject_name = ctx.name(place.ty);
    let member = match test.member.as_slice() {
        [(name, _)] => match ctx.types.member(union, name) {
            Some(id) => id,
            None => {
                let msg = if ctx.types.top_level(name) == Some(union) {
                    format!(
                        "'{}' already has type '{subject_name}', so this test is always true",
                        test.place
                    )
                } else {
                    format!("'{name}' is not a direct member of '{subject_name}'")
                };
                ctx.error(test.member_span, msg);
                return None;
            }
        },
        [(first, first_span), (second, _)] => {
            let Some(root) = ctx.types.top_level(first) else {
                ctx.error(*first_span, format!("Unknown type '{first}'"));
                return None;
            };
            let Some(id) = ctx.types.member(root, second) else {
                let owner = ctx.types.def(root).name().to_string();
                ctx.error(
                    test.member_span,
                    format!("'{owner}' has no member type '{second}'"),
                );
                return None;
            };
            if root != union {
                let named = ctx.types.def(id).name().to_string();
                ctx.error(
                    test.member_span,
                    format!("'{named}' is not a direct member of '{subject_name}'"),
                );
                return None;
            }
            id
        }
        _ => {
            ctx.error(
                test.member_span,
                "a tested member name has at most two components".into(),
            );
            return None;
        }
    };

    let (tag, nil) = match ctx.types.def(member) {
        TypeDef::UnionMember { tag, nil, .. } => (*tag, *nil),
        _ => (0, false),
    };
    let binding = match test.binding {
        Some((name, name_span)) if nil => {
            let _ = name;
            ctx.error(
                name_span,
                "'Nil' carries no value, so it cannot be bound by a type test".into(),
            );
            None
        }
        Some((name, name_span)) => {
            declare(ctx, name, name_span, "Binding");
            Some((name.to_string(), Ty::User(member)))
        }
        None => None,
    };
    Some(TTypeTest {
        place,
        member,
        tag,
        binding,
    })
}

/// Specification 018 section 6: the tested member is one direct member of an
/// inline sum, named by exactly one segment -- a built-in keyword or a
/// top-level type name. A test target is never a two-segment path: an inline
/// sum's members are never namespaced, and testing a member inside a
/// named-union member requires a second test after binding that union. The
/// caller (`check_arm_condition`) has already resolved `place`.
pub(crate) fn check_sum_type_test<'src>(
    ctx: &mut Ctx<'src>,
    test: &TypeTest<'src>,
    place: Place,
    sum: SumId,
) -> Option<TSumTypeTest> {
    let subject_name = ctx.name(place.ty);
    let [(name, name_span)] = test.member.as_slice() else {
        ctx.error(
            test.member_span,
            "a type test on an inline sum names exactly one direct member; testing a \
             member inside a named-union member requires a second test after binding \
             that union"
                .into(),
        );
        return None;
    };
    let member = match builtin_type_name(name) {
        Some(builtin) => Ty::from(builtin),
        None => match ctx.types.top_level(name) {
            Some(id) => Ty::User(id),
            None => {
                ctx.error(*name_span, format!("Unknown type '{name}'"));
                return None;
            }
        },
    };
    if !ctx.types.sum_members(sum).contains(&member) {
        ctx.error(
            *name_span,
            format!("'{name}' is not a direct member of '{subject_name}'"),
        );
        return None;
    }
    let binding = match test.binding {
        Some((name, name_span)) if member == Ty::Nil => {
            let _ = name;
            ctx.error(
                name_span,
                "'Nil' carries no value, so it cannot be bound by a type test".into(),
            );
            None
        }
        Some((name, name_span)) => {
            declare(ctx, name, name_span, "Binding");
            Some((name.to_string(), member))
        }
        None => None,
    };
    Some(TSumTypeTest {
        place,
        sum,
        member,
        binding,
    })
}
