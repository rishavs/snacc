//! Expression checking.

use super::*;

pub(crate) fn check_expr<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    expression: &Spanned<Expr<'src>>,
) -> (TExpr, Ty) {
    let span = expression.1;
    match &expression.0 {
        Expr::Error => {
            ctx.unknown = Some("a parser recovery node escaped into type checking");
            (TExpr::Num(NumLiteral::Int(0)), Ty::Int64)
        }
        Expr::Value(Value::Num(literal)) => {
            let ty = match literal {
                NumLiteral::Int(_) => Ty::Int64,
                NumLiteral::F64(_) => Ty::Float64,
                NumLiteral::U8(_) => Ty::Byte,
                NumLiteral::U16(_) => Ty::UInt16,
                NumLiteral::U32(_) => Ty::UInt32,
                NumLiteral::U64(_) => Ty::UInt64,
                NumLiteral::F32(_) => Ty::Float32,
            };
            (TExpr::Num(*literal), ty)
        }
        Expr::Value(Value::Bool(value)) => (TExpr::Bool(*value), Ty::Bool),
        Expr::Value(Value::Nil) => (TExpr::Nil, Ty::Nil),
        Expr::Value(Value::Str(value)) => (TExpr::StringLiteral(value.clone()), Ty::String),
        Expr::Value(Value::Unicode(value)) => (TExpr::Unicode(*value), Ty::Unicode),
        Expr::Interpolated(parts) => {
            let mut checked_parts = Vec::with_capacity(parts.len());
            for part in parts {
                let (part, part_ty) = match part {
                    crate::ast::StringPart::Literal(text) => {
                        (TExpr::StringLiteral(text.clone()), Ty::String)
                    }
                    crate::ast::StringPart::Expression(expression) => {
                        check_expr(ctx, env, expression)
                    }
                };
                let accepted = matches!(
                    part_ty,
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
                        span,
                        format!(
                            "string interpolation does not accept '{}'; expected a text or scalar part",
                            ctx.name(part_ty)
                        ),
                    );
                }
                checked_parts.push(TStringPart {
                    value: part,
                    ty: part_ty,
                });
            }
            (TExpr::StringConcat(checked_parts), Ty::String)
        }
        Expr::List(items) => {
            for item in items {
                check_expr(ctx, env, item);
            }
            ctx.error(span, "lists are not supported by the AOT backend".into());
            (TExpr::Nil, Ty::Nil)
        }
        Expr::MapNew(key, value) => {
            let key = resolve_type(ctx, key);
            let value = resolve_type(ctx, value);
            let id = ctx
                .types
                .intern_collection(CollectionDef::Map { key, value });
            (TExpr::CollectionNew(Ty::Map(id)), Ty::Map(id))
        }
        Expr::SetNew(inner) => {
            let elem = resolve_type(ctx, inner);
            let id = ctx.types.intern_collection(CollectionDef::Set { elem });
            (TExpr::CollectionNew(Ty::Set(id)), Ty::Set(id))
        }
        Expr::SelfRef => match ctx.self_ty {
            Some(ty) => (
                TExpr::Place(
                    Place {
                        root: PlaceRoot::SelfRef,
                        root_ty: ty,
                        path: Vec::new(),
                        ty,
                    },
                    UseMode::Copy,
                ),
                ty,
            ),
            None => {
                ctx.error(span, "'self' is only valid inside a method body".into());
                (TExpr::Nil, Ty::Nil)
            }
        },
        Expr::BuiltinType(name) => {
            ctx.error(
                span,
                format!("'{name}' is a type name; a type name alone is never a value"),
            );
            (TExpr::Nil, Ty::Nil)
        }
        Expr::Local(name) => {
            for binding in env.iter().rev() {
                if binding.name == *name {
                    let root = PlaceRoot::Local((*name).to_string());
                    if ctx.move_state.contains_key(&root) {
                        ctx.error(
                            span,
                            format!("'{root}' is already moved, so this use is invalid"),
                        );
                    }
                    return (
                        TExpr::Place(
                            Place {
                                root,
                                root_ty: binding.ty,
                                path: Vec::new(),
                                ty: binding.ty,
                            },
                            UseMode::Copy,
                        ),
                        binding.ty,
                    );
                }
            }
            if ctx.sigs.contains_key(*name) {
                ctx.error(
                    span,
                    format!("'{name}' is a function; functions cannot be used as values"),
                );
            } else if ctx.types.top_level(name).is_some() {
                ctx.error(
                    span,
                    format!("'{name}' is a type name; a type name alone is never a value"),
                );
            } else {
                ctx.error(span, format!("No such variable '{name}' in scope"));
            }
            (TExpr::Nil, Ty::Nil)
        }
        Expr::Member(base, (field, field_span)) => {
            match as_place(ctx, env, expression) {
                PlaceOutcome::Resolved(resolved) => {
                    if ctx.move_state.contains_key(&resolved.place.root) {
                        ctx.error(
                            expression.1,
                            format!(
                                "'{}' is already moved, so this use is invalid",
                                resolved.place.root
                            ),
                        );
                    }
                    let ty = resolved.place.ty;
                    (TExpr::Place(resolved.place, UseMode::Copy), ty)
                }
                PlaceOutcome::Reported => (TExpr::Nil, Ty::Nil),
                PlaceOutcome::NotAPlace => {
                    // A qualified type path is not a value on its own.
                    if let Expr::Local(first) = &base.0
                        && let Some(root) = ctx.types.top_level(first)
                    {
                        let owner = ctx.types.def(root).name().to_string();
                        let msg = if ctx.types.member(root, field).is_some() {
                            format!(
                                "'{owner}.{field}' is a type name; a type name alone is never a value"
                            )
                        } else {
                            format!("'{owner}' has no member type '{field}'")
                        };
                        ctx.error(span, msg);
                        return (TExpr::Nil, Ty::Nil);
                    }
                    let before = ctx.errors.len();
                    let (value, raw_base_ty) = check_expr(ctx, env, base);
                    if ctx.errors.len() != before {
                        return (TExpr::Nil, Ty::Nil);
                    }
                    // Specification 016 section 4.3: automatic dereference
                    // applies here too, since a fresh `box(...)` value (not a
                    // place) can still be the base of a field chain.
                    // `raw_base_ty` (un-dereferenced) is kept for the checked
                    // node itself, so lowering knows how many box layers to
                    // peel; `base_ty` (dereferenced) is used only to resolve
                    // the field here.
                    let base_ty = deref_box(&ctx.types, raw_base_ty);
                    let Ty::User(id) = base_ty else {
                        let owner = ctx.name(base_ty);
                        ctx.error(
                            *field_span,
                            format!("'{owner}' is not a struct, so it has no field '{field}'"),
                        );
                        return (TExpr::Nil, Ty::Nil);
                    };
                    let Some((index, ty)) = ctx.types.field(id, field) else {
                        let owner = ctx.types.def(id).name().to_string();
                        let msg = if ctx.method_index.contains_key(&(id, (*field).to_string())) {
                            format!(
                                "'{owner}.{field}' is a method; a method requires a receiver call"
                            )
                        } else if ctx.types.def(id).fields().is_none() {
                            format!("'{owner}' is not a struct, so it has no field '{field}'")
                        } else {
                            format!("'{owner}' has no field '{field}'")
                        };
                        ctx.error(*field_span, msg);
                        return (TExpr::Nil, Ty::Nil);
                    };
                    (
                        TExpr::FieldRead {
                            base: Box::new(value),
                            base_ty: raw_base_ty,
                            index,
                            ty,
                        },
                        ty,
                    )
                }
            }
        }
        Expr::Index(base, index) => {
            let (collection, collection_ty) = check_expr(ctx, env, base);
            let (checked_index, index_ty) = check_expr(ctx, env, index);
            if let Ty::Map(id) = collection_ty {
                let (key_ty, value_ty) = match ctx.types.collection(id) {
                    CollectionDef::Map { key, value } => (*key, *value),
                    _ => unreachable!("map type has non-map metadata"),
                };
                if ctx.types.is_move_only(value_ty) {
                    ctx.error(
                        span,
                        "an indexed map read cannot move a move-only value out of its map; use take()"
                            .into(),
                    );
                    return (TExpr::Nil, Ty::Nil);
                }
                let (key, query_ty) = if key_ty == Ty::String {
                    (
                        coerce(ctx, checked_index, index_ty, Ty::ViewByte, index.1),
                        Ty::ViewByte,
                    )
                } else {
                    if index_ty != key_ty {
                        ctx.mismatch(index.1, key_ty, index_ty);
                    }
                    (checked_index, index_ty)
                };
                return (
                    TExpr::MapIndex {
                        receiver: Box::new(collection),
                        key: Box::new(key),
                        key_ty: query_ty,
                        value_ty,
                    },
                    value_ty,
                );
            }
            if index_ty != Ty::Int64 {
                ctx.mismatch(index.1, Ty::Int64, index_ty);
            }
            let elem = match collection_ty {
                Ty::Array(id) | Ty::List(id) | Ty::View(id) => match ctx.types.collection(id) {
                    CollectionDef::Array { elem, .. }
                    | CollectionDef::List { elem }
                    | CollectionDef::View { elem } => Some(*elem),
                    _ => None,
                },
                Ty::ViewByte => Some(Ty::Byte),
                _ => None,
            };
            let Some(elem) = elem else {
                ctx.error(
                    base.1,
                    format!(
                        "'{}' is not an indexable collection",
                        ctx.name(collection_ty)
                    ),
                );
                return (TExpr::Nil, Ty::Nil);
            };
            if ctx.types.is_move_only(elem) {
                ctx.error(
                    span,
                    "an indexed read cannot move a move-only element out of its collection".into(),
                );
            }
            (
                TExpr::CollectionIndex {
                    collection: Box::new(collection),
                    index: Box::new(checked_index),
                    collection_ty,
                    elem,
                },
                elem,
            )
        }
        Expr::Unary(UnaryOp::Not, value) => {
            let (value, ty) = check_expr(ctx, env, value);
            if ty != Ty::Bool {
                ctx.error(
                    span,
                    format!("'!' requires a Bool operand, found '{}'", ctx.name(ty)),
                );
            }
            (TExpr::Not(Box::new(value)), Ty::Bool)
        }
        Expr::ReturnOnError(value) => {
            let CheckedReturnOnError::Expr { value, ty } =
                check_return_on_error(ctx, env, value, span, false)
            else {
                unreachable!("expression-form return_on_error produced a statement")
            };
            (value, ty)
        }
        Expr::Binary(left, op, right) => {
            let (left, left_ty) = check_expr(ctx, env, left);
            let (right, right_ty) = check_expr(ctx, env, right);
            match op {
                BinaryOp::And | BinaryOp::Or => {
                    if left_ty != Ty::Bool {
                        ctx.error(
                            span,
                            format!(
                                "'{}' requires Bool operands, found '{}' and '{}'",
                                if matches!(op, BinaryOp::And) {
                                    "and"
                                } else {
                                    "or"
                                },
                                ctx.name(left_ty),
                                ctx.name(right_ty)
                            ),
                        );
                    } else if right_ty != Ty::Bool {
                        ctx.error(
                            span,
                            format!(
                                "'{}' requires Bool operands, found '{}' and '{}'",
                                if matches!(op, BinaryOp::And) {
                                    "and"
                                } else {
                                    "or"
                                },
                                ctx.name(left_ty),
                                ctx.name(right_ty)
                            ),
                        );
                    }
                    let logical = if matches!(op, BinaryOp::And) {
                        LogicalOp::And
                    } else {
                        LogicalOp::Or
                    };
                    (
                        TExpr::Logical(Box::new(left), logical, Box::new(right)),
                        Ty::Bool,
                    )
                }
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                    let operation = match op {
                        BinaryOp::Add => ArithOp::Add,
                        BinaryOp::Sub => ArithOp::Sub,
                        BinaryOp::Mul => ArithOp::Mul,
                        _ => ArithOp::Div,
                    };
                    // A rejected pair keeps its own operand types rather than
                    // being coerced to a guessed one, so one mixed-type
                    // expression reports one diagnostic.
                    let Some(ty) = operand_numeric(left_ty, right_ty) else {
                        ctx.operands(span, "arithmetic", left_ty, right_ty);
                        return (
                            TExpr::Arith(Box::new(left), operation, Box::new(right), left_ty),
                            left_ty,
                        );
                    };
                    let left = coerce(ctx, left, left_ty, ty, span);
                    let right = coerce(ctx, right, right_ty, ty, span);
                    let value = TExpr::Arith(Box::new(left), operation, Box::new(right), ty);
                    reject_known_nan(ctx, &value, ty, span);
                    (value, ty)
                }
                BinaryOp::Eq | BinaryOp::NotEq => {
                    let operation = if matches!(op, BinaryOp::Eq) {
                        CmpOp::Eq
                    } else {
                        CmpOp::NotEq
                    };
                    // Specification 012 section 10: `nil == nil` has no union
                    // operand to take its type from.
                    if left_ty == Ty::Nil && right_ty == Ty::Nil {
                        ctx.error(span, types::CONTEXTLESS_NIL.to_string());
                        return (
                            TExpr::Cmp(Box::new(left), operation, Box::new(right), Ty::Nil),
                            Ty::Bool,
                        );
                    }
                    // Equality joins the `Int64`/`Float64` promotion pair; a
                    // contextual `nil` joins one Nil-containing union; every
                    // other type compares only against itself.
                    let operand_ty = match common_numeric(left_ty, right_ty) {
                        Some(ty) => ty,
                        None if left_ty == right_ty => left_ty,
                        None => match nil_union(ctx, left_ty, right_ty) {
                            Some(ty) => ty,
                            None => {
                                ctx.mismatch(span, left_ty, right_ty);
                                return (
                                    TExpr::Cmp(Box::new(left), operation, Box::new(right), left_ty),
                                    Ty::Bool,
                                );
                            }
                        },
                    };
                    if !ctx.types.supports_equality(operand_ty) {
                        let name = ctx.name(operand_ty);
                        ctx.error(
                            span,
                            format!(
                                "'{name}' does not support equality because one of the \
                                 types it contains does not"
                            ),
                        );
                    }
                    let left = coerce(ctx, left, left_ty, operand_ty, span);
                    let right = coerce(ctx, right, right_ty, operand_ty, span);
                    (
                        TExpr::Cmp(Box::new(left), operation, Box::new(right), operand_ty),
                        Ty::Bool,
                    )
                }
                _ => {
                    let operation = match op {
                        BinaryOp::Less => CmpOp::Less,
                        BinaryOp::LessEq => CmpOp::LessEq,
                        BinaryOp::Greater => CmpOp::Greater,
                        _ => CmpOp::GreaterEq,
                    };
                    let Some(operand_ty) = operand_numeric(left_ty, right_ty) else {
                        ctx.operands(span, "ordered comparison", left_ty, right_ty);
                        return (
                            TExpr::Cmp(Box::new(left), operation, Box::new(right), left_ty),
                            Ty::Bool,
                        );
                    };
                    let left = coerce(ctx, left, left_ty, operand_ty, span);
                    let right = coerce(ctx, right, right_ty, operand_ty, span);
                    (
                        TExpr::Cmp(Box::new(left), operation, Box::new(right), operand_ty),
                        Ty::Bool,
                    )
                }
            }
        }
        Expr::Call(callee, arguments) => {
            let Some(call) = check_call(ctx, env, span, callee, arguments) else {
                return (TExpr::Nil, Ty::Nil);
            };
            match call {
                CheckedCall::Value(value, ty) => (value, ty),
                CheckedCall::Function {
                    name,
                    args,
                    result: Some(ty),
                } => (TExpr::Call(name, args), ty),
                CheckedCall::Function { name, .. } => {
                    ctx.error(
                        span,
                        format!(
                            "'{name}' declares no result, so its call cannot be used as a value"
                        ),
                    );
                    (TExpr::Nil, Ty::Nil)
                }
                CheckedCall::Method {
                    call,
                    result: Some(ty),
                } => (TExpr::MethodCall(Box::new(call)), ty),
                CheckedCall::Method { call, .. } => {
                    let name = ctx.method_name(call.method);
                    ctx.error(
                        span,
                        format!(
                            "'{name}' declares no result, so its call cannot be used as a value"
                        ),
                    );
                    (TExpr::Nil, Ty::Nil)
                }
                CheckedCall::Statement(_) => {
                    ctx.error(span, "a list mutation does not produce a value".into());
                    (TExpr::Nil, Ty::Nil)
                }
            }
        }
        Expr::GenericCall(callee, type_args, arguments) => {
            let Some(call) =
                check_generic_function_call(ctx, env, span, callee, type_args, arguments)
            else {
                return (TExpr::Nil, Ty::Nil);
            };
            match call {
                CheckedCall::Function {
                    name,
                    args,
                    result: Some(ty),
                } => (TExpr::Call(name, args), ty),
                CheckedCall::Function { name, .. } => {
                    ctx.error(
                        span,
                        format!(
                            "'{name}' declares no result, so its call cannot be used as a value"
                        ),
                    );
                    (TExpr::Nil, Ty::Nil)
                }
                CheckedCall::Value(value, ty) => (value, ty),
                CheckedCall::Method {
                    call,
                    result: Some(ty),
                } => (TExpr::MethodCall(Box::new(call)), ty),
                CheckedCall::Method { .. } | CheckedCall::Statement(_) => {
                    ctx.error(span, "generic call does not produce a value".into());
                    (TExpr::Nil, Ty::Nil)
                }
            }
        }
        Expr::Print(value) => {
            let before = ctx.errors.len();
            let (value, ty) = check_expr(ctx, env, value);
            // Specification 012 section 10 and 12: there is no standalone `Nil`
            // value and no `snacc_print_nil` import, so `print(nil)` is a
            // context-free `nil`. Only report when the operand checked cleanly;
            // `Ty::Nil` is also this checker's error-recovery type.
            if ty == Ty::Nil && ctx.errors.len() == before {
                ctx.error(span, types::CONTEXTLESS_NIL.to_string());
            }
            // Specification 010 section 14: printing a user-defined type is a
            // separate future feature, not a silent no-op. Specification 018
            // section 8 extends the same restriction to a whole inline sum:
            // a program decomposes it with `is` before printing a member.
            match ty {
                Ty::User(_) => {
                    let name = ctx.name(ty);
                    ctx.error(
                        span,
                        format!(
                            "'print' does not support the user-defined type '{name}'; print a \
                             scalar field or unwrap a represented scalar"
                        ),
                    );
                }
                Ty::Sum(_) => {
                    let name = ctx.name(ty);
                    ctx.error(
                        span,
                        format!(
                            "'print' does not support the inline sum type '{name}'; decompose \
                             it with 'is' and print the bound member"
                        ),
                    );
                }
                // Specification 016 section 8.3: direct printing of a box is
                // unsupported initially.
                Ty::Box(_) => {
                    let name = ctx.name(ty);
                    ctx.error(
                        span,
                        format!("'print' does not support the box type '{name}'"),
                    );
                }
                _ => {}
            }
            (TExpr::Print(Box::new(value), ty), ty)
        }
        Expr::Box(operand) => {
            // Specification 016 section 4.2: the operand is an ordinary
            // expression, evaluated exactly once; its checked type becomes
            // the pointee `T`. Every checked expression already has a
            // storable value type (`Ref<T>` and a no-result type are never
            // an expression's type), so no separate "storable pointee"
            // validation is needed here.
            let (value, pointee) = check_expr(ctx, env, operand);
            if is_borrowed_type(ctx, pointee) {
                ctx.error(
                    operand.1,
                    format!(
                        "'{}' is borrowed and cannot be stored in a Box",
                        ctx.name(pointee)
                    ),
                );
            }
            // Specification 016 section 6.1: allocating a box transfers its
            // operand's complete value into the new allocation exactly like
            // an aggregate constructor argument does, so a move-only operand
            // is a consuming use here too -- otherwise the same already-boxed
            // place could be boxed again, producing two owners of one
            // allocation.
            let value = mark_consumed(ctx, env, value, operand.1);
            let ty = Ty::Box(ctx.types.intern_box(pointee));
            (TExpr::Box(Box::new(value), ty), ty)
        }
    }
}

pub(crate) fn check_collection_literal<'src>(
    ctx: &mut Ctx<'src>,
    env: &mut Env<'src>,
    expression: &Spanned<Expr<'src>>,
    expected: Ty,
) -> (TExpr, Ty) {
    let Expr::List(items) = &expression.0 else {
        unreachable!("collection literal helper received a non-list expression")
    };
    let (elem, length, is_array) = match expected {
        Ty::Array(id) => match ctx.types.collection(id) {
            CollectionDef::Array { elem, len } => (*elem, *len, true),
            _ => unreachable!("array type has non-array collection metadata"),
        },
        Ty::List(id) => match ctx.types.collection(id) {
            CollectionDef::List { elem } => (*elem, 0, false),
            _ => unreachable!("list type has non-list collection metadata"),
        },
        _ => {
            ctx.error(
                expression.1,
                "a collection literal requires an expected Array<T, N> or List<T> type".into(),
            );
            for item in items {
                check_expr(ctx, env, item);
            }
            return (TExpr::Nil, Ty::Nil);
        }
    };
    if is_array && items.len() != length as usize {
        ctx.error(
            expression.1,
            format!(
                "array literal has {} elements, but the expected array length is {length}",
                items.len()
            ),
        );
    }
    let mut checked = Vec::with_capacity(items.len());
    for item in items {
        let (value, value_ty) = check_expr(ctx, env, item);
        let value = mark_consumed(ctx, env, value, item.1);
        checked.push(coerce(ctx, value, value_ty, elem, item.1));
    }
    (
        TExpr::CollectionLiteral {
            ty: expected,
            items: checked,
        },
        expected,
    )
}
