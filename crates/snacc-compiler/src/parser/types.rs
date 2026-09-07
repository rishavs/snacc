use super::*;

pub(super) fn builtin_type_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, TypeName, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    select! {
        Token::TyFloat64 => TypeName::Float64,
        Token::TyInt64 => TypeName::Int64,
        Token::TyBool => TypeName::Bool,
        Token::TyNil => TypeName::Nil,
        Token::TyString => TypeName::String,
        Token::TyUnicode => TypeName::Unicode,
        Token::TyByte => TypeName::Byte,
        Token::TyUInt16 => TypeName::UInt16,
        Token::TyUInt32 => TypeName::UInt32,
        Token::TyUInt64 => TypeName::UInt64,
        Token::TyFloat32 => TypeName::Float32,
    }
    .labelled("built-in type name")
}

/// Specification 011 section 5: the permitted declaration sites, named by the
/// diagnostic every other type position produces.
pub(super) const REFERENCE_OUTSIDE_A_PARAMETER: &str = "'Ref<T>' is only valid as the direct type of a function, method, or Rust \
     bridge parameter; a reference is not storable, so it cannot be a result, a \
     local, a field, a represented type, or a union member";

pub(super) const NESTED_REFERENCE: &str =
    "'Ref<T>' cannot contain another reference; a reference parameter refers to a value type";

/// `primary-value-type = builtin-value-type | qualified-name | "(", sum-type,
/// ")"` and `sum-type = primary-value-type, { "|", primary-value-type }`.
pub(super) fn sum_type_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<TypeRef<'src>>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    recursive(|sum_type| {
        let boxed = just(Token::Box)
            .ignore_then(just(Token::Op("<")))
            .ignore_then(sum_type.clone())
            .then_ignore(just(Token::Op(">")))
            .map_with(|inner, e| (TypeRef::Box(Box::new(inner)), e.span()));

        let view = just(Token::TyView)
            .ignore_then(just(Token::Op("<")))
            .ignore_then(sum_type.clone())
            .then_ignore(just(Token::Op(">")))
            .map_with(|inner, e| (TypeRef::View(Box::new(inner)), e.span()));

        let array = just(Token::TyArray)
            .ignore_then(just(Token::Op("<")))
            .ignore_then(sum_type.clone())
            .then_ignore(just(Token::Ctrl(',')))
            .then(select! { Token::Num(NumLiteral::Int(length)) => length })
            .then_ignore(just(Token::Op(">")))
            .validate(|(inner, length), e, emitter| {
                if length < 0 {
                    emitter.emit(Rich::custom(
                        e.span(),
                        "Array length must be non-negative".to_string(),
                    ));
                }
                (
                    TypeRef::Array(Box::new(inner), length.max(0) as u64),
                    e.span(),
                )
            });
        let list = just(Token::TyList)
            .ignore_then(just(Token::Op("<")))
            .ignore_then(sum_type.clone())
            .then_ignore(just(Token::Op(">")))
            .map_with(|inner, e| (TypeRef::List(Box::new(inner)), e.span()));
        let map = just(Token::TyMap)
            .ignore_then(just(Token::Op("<")))
            .ignore_then(sum_type.clone())
            .then_ignore(just(Token::Ctrl(',')))
            .then(sum_type.clone())
            .then_ignore(just(Token::Op(">")))
            .map_with(|(key, value), e| (TypeRef::Map(Box::new(key), Box::new(value)), e.span()));
        let set = just(Token::TySet)
            .ignore_then(just(Token::Op("<")))
            .ignore_then(sum_type.clone())
            .then_ignore(just(Token::Op(">")))
            .map_with(|inner, e| (TypeRef::Set(Box::new(inner)), e.span()));

        let named_path = name_parser()
            .separated_by(just(Token::Ctrl('.')))
            .at_least(1)
            .collect::<Vec<_>>();
        let generic_args = just(Token::Op("<"))
            .ignore_then(
                sum_type
                    .clone()
                    .separated_by(just(Token::Ctrl(',')))
                    .at_least(1)
                    .collect::<Vec<_>>(),
            )
            .then_ignore(just(Token::Op(">")));
        let applied = named_path
            .clone()
            .then(generic_args)
            .map(|(path, args)| TypeRef::Apply { path, args });

        let primary = builtin_type_parser()
            .map(TypeRef::Builtin)
            .or(applied)
            .or(named_path.map(TypeRef::Named))
            .map_with(|ty, e| (ty, e.span()))
            .or(boxed)
            .or(view)
            .or(array)
            .or(list)
            .or(map)
            .or(set)
            .or(sum_type
                .clone()
                .delimited_by(just(Token::Ctrl('(')), just(Token::Ctrl(')'))))
            .labelled("type")
            .boxed();

        primary
            .clone()
            .then(
                just(Token::Ctrl('|'))
                    .ignore_then(primary)
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map_with(|(first, rest), e| {
                if rest.is_empty() {
                    first
                } else {
                    let mut members = vec![first];
                    members.extend(rest);
                    (TypeRef::Sum(members), e.span())
                }
            })
            .boxed()
    })
}

pub(super) fn reference_type_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<TypeRef<'src>>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    let nested = bracketed_reference(sum_type_parser()).validate(|ty, e, emitter| {
        emitter.emit(Rich::custom(e.span(), NESTED_REFERENCE.to_string()));
        ty
    });
    bracketed_reference(nested.or(sum_type_parser())).boxed()
}

fn bracketed_reference<'tokens, 'src: 'tokens, I, P>(
    referent: P,
) -> impl Parser<'tokens, I, Spanned<TypeRef<'src>>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
    P: Parser<'tokens, I, Spanned<TypeRef<'src>>, extra::Err<Rich<'tokens, Token<'src>, Span>>>
        + Clone,
{
    just(Token::Ref)
        .ignore_then(just(Token::Op("<")))
        .ignore_then(referent)
        .then_ignore(just(Token::Op(">")))
}

pub(super) fn type_ref_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<TypeRef<'src>>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    reference_type_parser()
        .validate(|ty, e, emitter| {
            emitter.emit(Rich::custom(
                e.span(),
                REFERENCE_OUTSIDE_A_PARAMETER.to_string(),
            ));
            ty
        })
        .or(sum_type_parser())
        .boxed()
}

pub(super) fn param_type_parser<'tokens, 'src: 'tokens, I>() -> impl Parser<
    'tokens,
    I,
    (ParamMode, Spanned<TypeRef<'src>>),
    extra::Err<Rich<'tokens, Token<'src>, Span>>,
> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    reference_type_parser()
        .map(|ty| (ParamMode::Reference, ty))
        .or(sum_type_parser().map(|ty| (ParamMode::Value, ty)))
        .boxed()
}
