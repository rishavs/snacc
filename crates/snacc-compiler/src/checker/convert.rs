//! Coercion, common numeric types, and sum/union injection (Specification 009
//! section 4.4, Specification 010 section 13, Specification 018 section 5).

use super::*;

/// The two types the one surviving implicit numeric conversion joins.
/// Specification 009 section 4.4 adds no further promotion.
pub(crate) fn numeric(ty: Ty) -> bool {
    matches!(ty, Ty::Float64 | Ty::Int64)
}

/// Numeric types that operate only on an exact type match (Specification 009
/// sections 4.5-4.6): they never promote, not even to each other.
pub(crate) fn exact_match_numeric(ty: Ty) -> bool {
    matches!(
        ty,
        Ty::Byte | Ty::UInt16 | Ty::UInt32 | Ty::UInt64 | Ty::Float32
    )
}

pub(crate) fn common_numeric(left: Ty, right: Ty) -> Option<Ty> {
    if !numeric(left) || !numeric(right) {
        return None;
    }
    Some(if left == Ty::Float64 || right == Ty::Float64 {
        Ty::Float64
    } else {
        Ty::Int64
    })
}

/// The type an arithmetic or ordered-comparison pair shares, or `None` when the
/// operands cannot be combined at all.
pub(crate) fn operand_numeric(left: Ty, right: Ty) -> Option<Ty> {
    common_numeric(left, right)
        .or_else(|| (left == right && exact_match_numeric(left)).then_some(left))
}

/// Evaluates the closed subset of checked expressions whose floating result is
/// known without executing user code. This is deliberately not a general
/// constant folder: it only supports literals, the existing integer-to-float
/// widening conversion, transparent printing, and floating arithmetic.
pub(crate) fn known_float_value(value: &TExpr) -> Option<f64> {
    match value {
        TExpr::Num(NumLiteral::F64(value)) => Some(*value),
        TExpr::Num(NumLiteral::F32(value)) => Some(f64::from(*value)),
        TExpr::Cast(value, Ty::Float64) => match value.as_ref() {
            TExpr::Num(NumLiteral::Int(value)) => Some(*value as f64),
            _ => known_float_value(value),
        },
        TExpr::Print(value, ty) if matches!(ty, Ty::Float32 | Ty::Float64) => {
            known_float_value(value)
        }
        TExpr::Arith(left, op, right, Ty::Float64) => {
            let left = known_float_value(left)?;
            let right = known_float_value(right)?;
            Some(match op {
                ArithOp::Add => left + right,
                ArithOp::Sub => left - right,
                ArithOp::Mul => left * right,
                ArithOp::Div => left / right,
            })
        }
        TExpr::Arith(left, op, right, Ty::Float32) => {
            let left = known_float_value(left)? as f32;
            let right = known_float_value(right)? as f32;
            Some(f64::from(match op {
                ArithOp::Add => left + right,
                ArithOp::Sub => left - right,
                ArithOp::Mul => left * right,
                ArithOp::Div => left / right,
            }))
        }
        _ => None,
    }
}

pub(crate) fn reject_known_nan(ctx: &mut Ctx<'_>, value: &TExpr, ty: Ty, span: Span) {
    if matches!(ty, Ty::Float32 | Ty::Float64) && known_float_value(value).is_some_and(f64::is_nan)
    {
        ctx.error(
            span,
            "floating-point operation produces NaN, which is not a Snacc value".into(),
        );
    }
}

/// The one existing implicit scalar conversion (Specification 009 section
/// 4.4), reused unchanged as an inline sum's tier-2 injection rule
/// (Specification 018 section 5, "existing implicit conversions"). Named-union
/// member injection is deliberately excluded here: section 5 states a named
/// union's own member injects into an inline sum only once an exact expected
/// union type has already produced a union value, never directly.
pub(crate) fn implicit_conversion_target(from: Ty, to: Ty) -> bool {
    from == Ty::Int64 && to == Ty::Float64
}

/// Assignability. Beyond `Int64` to `Float64`, Specification 010 section 13 adds
/// exactly one implicit conversion: direct union-member injection, including
/// the contextual `nil` spelling of a union's `Nil` member. Specification 018
/// section 5 adds inline-sum injection: an exact direct member match wins;
/// otherwise exactly one existing implicit conversion must accept the value.
pub(crate) fn coerce(ctx: &mut Ctx<'_>, value: TExpr, from: Ty, to: Ty, span: Span) -> TExpr {
    if from == to {
        return value;
    }
    if from == Ty::String && matches!(to, Ty::ViewByte | Ty::ViewUnicode) {
        if let TExpr::Place(place, _) = value {
            return TExpr::ViewFromString(Box::new(TExpr::Place(place, UseMode::Copy)), to);
        }
        ctx.error(
            span,
            "a view can only be lent from a named String place; a temporary String would not live long enough".into(),
        );
        return TExpr::Nil;
    }
    if from == Ty::Int64 && to == Ty::Float64 {
        return TExpr::Cast(Box::new(value), Ty::Float64);
    }
    if let Ty::User(union) = to {
        if let Ty::User(member) = from
            && ctx.types.containing_union(member) == Some(union)
        {
            return TExpr::Inject {
                member,
                into_union: union,
                value: Box::new(value),
            };
        }
        // Specification 012 section 10: `nil` names the `Nil` member of one
        // expected union and never has the standalone type `Nil`.
        if from == Ty::Nil
            && let Some(member) = ctx.types.member(union, "Nil")
        {
            return TExpr::Inject {
                member,
                into_union: union,
                value: Box::new(TExpr::Construct {
                    type_id: member,
                    fields: Vec::new(),
                }),
            };
        }
    }
    if let Ty::Sum(sum) = to {
        let members = ctx.types.sum_members(sum).to_vec();
        // Tier 1: an exact direct member match, including a literal `Nil`
        // member selected by the contextual `nil` literal, which duplicate
        // rejection already guarantees is unique when present.
        if members.contains(&from) {
            return TExpr::InjectSum {
                sum,
                member: from,
                value: Box::new(value),
            };
        }
        // Tier 2: exactly one existing implicit conversion must accept the
        // value; more than one is an ambiguity, and a sum can hold at most
        // one member of any given type, so today's single scalar conversion
        // rule can never itself produce more than one candidate.
        let candidates: Vec<Ty> = members
            .iter()
            .copied()
            .filter(|member| implicit_conversion_target(from, *member))
            .collect();
        match candidates.as_slice() {
            [one] => {
                let converted = coerce(ctx, value, from, *one, span);
                return TExpr::InjectSum {
                    sum,
                    member: *one,
                    value: Box::new(converted),
                };
            }
            [] => {}
            _ => {
                let found = ctx.name(from);
                let target = ctx.name(to);
                ctx.error(
                    span,
                    format!(
                        "'{found}' could convert into more than one member of '{target}'; \
                         add an explicit conversion to pick one"
                    ),
                );
                return value;
            }
        }
    }
    ctx.mismatch(span, to, from);
    value
}

/// Bridge-only view compatibility. `View<Byte>` and `View<Unicode>` retain
/// their string-specific internal types, while a collection view of the same
/// element is a generalized descriptor. The generated bridge consumes both
/// through the identical pointer/length ABI, so this compatibility is valid
/// only for `extern rust` calls and must not leak into string operations.
pub(crate) fn coerce_bridge_view(
    ctx: &mut Ctx<'_>,
    value: TExpr,
    from: Ty,
    to: Ty,
    span: Span,
) -> TExpr {
    if from == to {
        return value;
    }
    let from_elem = match from {
        Ty::View(id) => match ctx.types.collection(id) {
            CollectionDef::View { elem } => Some(*elem),
            _ => None,
        },
        Ty::ViewByte => Some(Ty::Byte),
        Ty::ViewUnicode => Some(Ty::Unicode),
        _ => None,
    };
    let to_elem = match to {
        Ty::View(id) => match ctx.types.collection(id) {
            CollectionDef::View { elem } => Some(*elem),
            _ => None,
        },
        Ty::ViewByte => Some(Ty::Byte),
        Ty::ViewUnicode => Some(Ty::Unicode),
        _ => None,
    };
    if from_elem.is_some() && from_elem == to_elem {
        return value;
    }
    coerce(ctx, value, from, to, span)
}

/// Applies the call-boundary-only owning-sequence-to-view conversion and the
/// String-to-text-view conversion without consuming the owner. The returned
/// place is retained for same-call borrow/move overlap validation.
pub(crate) fn lend_call_view(
    ctx: &Ctx<'_>,
    value: &TExpr,
    from: Ty,
    to: Ty,
) -> Option<(TExpr, Place)> {
    let TExpr::Place(place, _) = value else {
        return None;
    };
    if from == Ty::String && matches!(to, Ty::ViewByte | Ty::ViewUnicode) {
        return Some((
            TExpr::ViewFromString(Box::new(TExpr::Place(place.clone(), UseMode::Copy)), to),
            place.clone(),
        ));
    }
    let source_elem = match from {
        Ty::Array(id) | Ty::List(id) => match ctx.types.collection(id) {
            CollectionDef::Array { elem, .. } | CollectionDef::List { elem } => Some(*elem),
            _ => None,
        },
        _ => None,
    };
    let target_elem = match to {
        Ty::View(id) => match ctx.types.collection(id) {
            CollectionDef::View { elem } => Some(*elem),
            _ => None,
        },
        Ty::ViewByte => Some(Ty::Byte),
        Ty::ViewUnicode => Some(Ty::Unicode),
        _ => None,
    };
    (source_elem.is_some() && source_elem == target_elem).then(|| {
        (
            TExpr::CollectionView(Box::new(TExpr::Place(place.clone(), UseMode::Copy)), to),
            place.clone(),
        )
    })
}

/// A propagation expression has already selected the successful payload. It
/// may therefore be re-injected into the enclosing callable's larger result
/// sum; ordinary sum assignment remains exact and does not use this rule.
pub(crate) fn coerce_return_success(
    ctx: &mut Ctx<'_>,
    value: TExpr,
    from: Ty,
    to: Ty,
    span: Span,
) -> TExpr {
    if let (Ty::Sum(source), Ty::Sum(target)) = (from, to) {
        let source_members = ctx.types.sum_members(source);
        let target_members = ctx.types.sum_members(target);
        if source_members
            .iter()
            .all(|member| target_members.contains(member))
        {
            return TExpr::LiftSum {
                value: Box::new(value),
                from: source,
                to: target,
            };
        }
    }
    coerce(ctx, value, from, to, span)
}

/// The aggregate type -- a named union or an inline sum -- a `value == nil`
/// comparison takes, when exactly one operand is `nil` and the other directly
/// contains `Nil`.
pub(crate) fn nil_union(ctx: &Ctx<'_>, left: Ty, right: Ty) -> Option<Ty> {
    let pair = |aggregate: Ty, other: Ty| match (aggregate, other) {
        (Ty::User(id), Ty::Nil) => ctx.types.member(id, "Nil").map(|_| aggregate),
        (Ty::Sum(id), Ty::Nil) => ctx
            .types
            .sum_members(id)
            .contains(&Ty::Nil)
            .then_some(aggregate),
        _ => None,
    };
    pair(left, right).or_else(|| pair(right, left))
}
