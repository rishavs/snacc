use super::*;

pub(super) fn expr_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<Expr<'src>>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    recursive(|expr| {
        let val = select! {
            Token::Nil => Expr::Value(Value::Nil),
            Token::Bool(x) => Expr::Value(Value::Bool(x)),
            Token::Num(n) => Expr::Value(Value::Num(n)),
            Token::Unicode(value) => Expr::Value(Value::Unicode(value)),
        }
        .labelled("value");

        let string = select! { Token::Str(s) => s }.validate(|value, extra, emitter| {
            match decode_string_content(value) {
                Ok(value) => Expr::Value(Value::Str(value)),
                Err(message) => {
                    emitter.emit(Rich::custom(extra.span(), message));
                    Expr::Error
                }
            }
        });

        let raw_string = select! { Token::RawStr(s) => s }
            .map(|value| Expr::Value(Value::Str(normalize_line_endings(value))));

        let interpolated =
            select! { Token::Interpolated(parts) => parts }.validate(|parts, extra, emitter| {
                let mut checked = Vec::with_capacity(parts.len());
                for part in parts {
                    match part {
                        crate::lexer::InterpolatedPart::Literal(value) => {
                            checked.push(StringPart::Literal(value));
                        }
                        crate::lexer::InterpolatedPart::Expression(tokens) => {
                            match parse_interpolation_expression(&tokens) {
                                Ok(value) => checked.push(StringPart::Expression(value)),
                                Err(message) => {
                                    emitter.emit(Rich::custom(extra.span(), message));
                                }
                            }
                        }
                    }
                }
                Expr::Interpolated(checked)
            });

        let name = name_parser();

        let items = expr
            .clone()
            .separated_by(just(Token::Ctrl(',')))
            .allow_trailing()
            .collect::<Vec<_>>();

        let list = items
            .clone()
            .map(Expr::List)
            .delimited_by(just(Token::Ctrl('[')), just(Token::Ctrl(']')));

        let empty_call = just(Token::Ctrl('(')).then_ignore(just(Token::Ctrl(')')));
        let map_new = just(Token::TyMap)
            .ignore_then(just(Token::Op("<")))
            .ignore_then(sum_type_parser())
            .then_ignore(just(Token::Ctrl(',')))
            .then(sum_type_parser())
            .then_ignore(just(Token::Op(">")))
            .then_ignore(empty_call.clone())
            .map(|(key, value)| Expr::MapNew(key, value));
        let set_new = just(Token::TySet)
            .ignore_then(just(Token::Op("<")))
            .ignore_then(sum_type_parser())
            .then_ignore(just(Token::Op(">")))
            .then_ignore(empty_call)
            .map(Expr::SetNew);

        // `argument = [ identifier, ":" ], expression`. A named argument is
        // recorded without deciding whether the call head can accept one.
        let argument = name
            .clone()
            .then_ignore(just(Token::Ctrl(':')))
            .or_not()
            .then(expr.clone())
            .map(|(name, value)| Arg { name, value });
        let arguments = argument
            .separated_by(just(Token::Ctrl(',')))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::Ctrl('(')), just(Token::Ctrl(')')))
            .map_with(|args, e| (args, e.span()));

        let atom = val
            .or(string)
            .or(raw_string)
            .or(interpolated)
            .or(select! { Token::SelfKw => Expr::SelfRef })
            // A built-in type name is a call head only: `Int64(id)` unwraps one
            // represented layer. Checking rejects it in any other position.
            .or(builtin_type_parser().map(Expr::BuiltinType))
            .or(name.clone().map(|(name, _)| Expr::Local(name)))
            .or(list)
            .or(map_new)
            .or(set_new)
            .or(just(Token::Print)
                .ignore_then(
                    expr.clone()
                        .delimited_by(just(Token::Ctrl('(')), just(Token::Ctrl(')'))),
                )
                .map(|expr| Expr::Print(Box::new(expr))))
            // `"box", "(", expression, ")"` (Specification 016 section 4.2): a
            // reserved allocation expression, not a call -- `box` lexes to its
            // own token, so this can never collide with calling a function or
            // type constructor named `box`.
            .or(just(Token::BoxExpr)
                .ignore_then(
                    expr.clone()
                        .delimited_by(just(Token::Ctrl('(')), just(Token::Ctrl(')'))),
                )
                .map(|expr| Expr::Box(Box::new(expr))))
            .map_with(|expr, e| (expr, e.span()))
            .or(expr
                .clone()
                .delimited_by(just(Token::Ctrl('(')), just(Token::Ctrl(')'))))
            .recover_with(via_parser(nested_delimiters(
                Token::Ctrl('('),
                Token::Ctrl(')'),
                [(Token::Ctrl('['), Token::Ctrl(']'))],
                |span| (Expr::Error, span),
            )))
            .recover_with(via_parser(nested_delimiters(
                Token::Ctrl('['),
                Token::Ctrl(']'),
                [(Token::Ctrl('('), Token::Ctrl(')'))],
                |span| (Expr::Error, span),
            )))
            .boxed();

        // `postfix = atom, { arguments | member-suffix }`. Nothing here decides
        // whether a name is a type, field, constructor, or method.
        let generic_arguments = just(Token::Op("<"))
            .ignore_then(
                type_ref_parser()
                    .separated_by(just(Token::Ctrl(',')))
                    .at_least(1)
                    .collect::<Vec<_>>(),
            )
            .then_ignore(just(Token::Op(">")))
            .map_with(|args, e| (args, e.span()))
            .then(arguments.clone());

        enum Suffix<'src> {
            Call(Spanned<Vec<Arg<'src>>>),
            GenericCall(
                Spanned<Vec<Spanned<TypeRef<'src>>>>,
                Spanned<Vec<Arg<'src>>>,
            ),
            Member(Spanned<&'src str>),
            Index(Spanned<Expr<'src>>),
        }
        let suffix = generic_arguments
            .map(|(type_args, args)| Suffix::GenericCall(type_args, args))
            .or(arguments.map(Suffix::Call))
            .or(just(Token::Ctrl('.')).ignore_then(name).map(Suffix::Member))
            .or(expr
                .clone()
                .delimited_by(just(Token::Ctrl('[')), just(Token::Ctrl(']')))
                .map(Suffix::Index));
        let postfix = atom.foldl_with(suffix.repeated(), |base, suffix, e| {
            let expr = match suffix {
                Suffix::Call(args) => Expr::Call(Box::new(base), args),
                Suffix::GenericCall(type_args, args) => {
                    Expr::GenericCall(Box::new(base), type_args, args)
                }
                Suffix::Member(name) => Expr::Member(Box::new(base), name),
                Suffix::Index(index) => Expr::Index(Box::new(base), Box::new(index)),
            };
            (expr, e.span())
        });

        // Unary negation recurses into itself, not the complete expression:
        // this preserves right associativity and keeps `!a or b` grouped as
        // `(!a) or b` rather than `!(a or b)`.
        let unary = recursive(|unary| {
            just(Token::Op("!"))
                .ignore_then(unary.clone())
                .map_with(|value, e| (Expr::Unary(UnaryOp::Not, Box::new(value)), e.span()))
                .or(just(Token::ReturnOnError)
                    .ignore_then(postfix.clone())
                    .map_with(|value, e| (Expr::ReturnOnError(Box::new(value)), e.span())))
                .or(postfix.clone())
                .boxed()
        });

        let op = just(Token::Op("*"))
            .to(BinaryOp::Mul)
            .or(just(Token::Op("/")).to(BinaryOp::Div));
        let product = unary
            .clone()
            .foldl_with(op.then(unary.clone()).repeated(), |a, (op, b), e| {
                (Expr::Binary(Box::new(a), op, Box::new(b)), e.span())
            });

        let op = just(Token::Op("+"))
            .to(BinaryOp::Add)
            .or(just(Token::Op("-")).to(BinaryOp::Sub));
        let sum = product
            .clone()
            .foldl_with(op.then(product).repeated(), |a, (op, b), e| {
                (Expr::Binary(Box::new(a), op, Box::new(b)), e.span())
            });

        let op = just(Token::Op("=="))
            .to(BinaryOp::Eq)
            .or(just(Token::Op("!=")).to(BinaryOp::NotEq))
            .or(just(Token::Op("<")).to(BinaryOp::Less))
            .or(just(Token::Op("<=")).to(BinaryOp::LessEq))
            .or(just(Token::Op(">")).to(BinaryOp::Greater))
            .or(just(Token::Op(">=")).to(BinaryOp::GreaterEq));
        // A comparison accepts at most one operator. Besides enforcing the
        // language rule, this leaves `f<T>(x)` unambiguous once generic calls
        // are added: a second `<`/`>` cannot be consumed as a legal chain.
        let compare =
            sum.clone()
                .then(op.then(sum.clone()).or_not())
                .map_with(|(left, comparison), e| match comparison {
                    Some((op, right)) => {
                        (Expr::Binary(Box::new(left), op, Box::new(right)), e.span())
                    }
                    None => left,
                });

        let logical_and = compare.clone().foldl_with(
            just(Token::And)
                .to(BinaryOp::And)
                .then(compare.clone())
                .repeated(),
            |a, (op, b), e| (Expr::Binary(Box::new(a), op, Box::new(b)), e.span()),
        );
        let logical_or = logical_and.clone().foldl_with(
            just(Token::Or)
                .to(BinaryOp::Or)
                .then(logical_and.clone())
                .repeated(),
            |a, (op, b), e| (Expr::Binary(Box::new(a), op, Box::new(b)), e.span()),
        );

        logical_or.labelled("expression").as_context()
    })
    .boxed()
}

/// Parses one already-tokenized interpolation expression outside the
/// recursive parser-construction closure. Keeping this helper concrete avoids
/// recursively instantiating the full expression parser type.
fn parse_interpolation_expression<'src>(
    tokens: &[Spanned<Token<'src>>],
) -> Result<Spanned<Expr<'src>>, String> {
    let input_span: Span = (0usize..0usize).into();
    let (value, errors) = expr_parser()
        .parse(tokens.map(input_span, |(token, span)| (token, span)))
        .into_output_errors();
    if let Some(error) = errors.first() {
        return Err(error.to_string());
    }
    value.ok_or_else(|| "interpolation expression produced no syntax".into())
}

/// `condition = type-test | expression`. A type test is valid only as a
/// complete `if`/`elseif` condition (Specification 010 section 12.2), which
/// this shape enforces structurally.
pub(super) fn condition_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Condition<'src>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    // Every built-in type name is a reserved word rather than an identifier,
    // so a member path accepts each one as its own segment spelling.
    // Specification 018 section 3 extends the tested member to any direct
    // sum member, including a built-in scalar (`is Byte(byte)`), not only
    // `Nil` as before.
    let segment = select! {
        Token::Ident(name) => name,
        Token::TyNil => "Nil",
        Token::TyFloat64 => "Float64",
        Token::TyInt64 => "Int64",
        Token::TyBool => "Bool",
        Token::TyUInt16 => "UInt16",
        Token::TyUInt32 => "UInt32",
        Token::TyUInt64 => "UInt64",
        Token::TyFloat32 => "Float32",
        Token::TyString => "String",
        Token::TyUnicode => "Unicode",
        Token::TyByte => "Byte",
    }
    .map_with(|name, e| (name, e.span()));

    let type_test = place_parser()
        .then_ignore(just(Token::Is))
        .then(
            segment
                .separated_by(just(Token::Ctrl('.')))
                .at_least(1)
                .collect::<Vec<_>>()
                .map_with(|member, e| (member, e.span())),
        )
        .then(
            name_parser()
                .delimited_by(just(Token::Ctrl('(')), just(Token::Ctrl(')')))
                .or_not(),
        )
        .map_with(|((place, (member, member_span)), binding), e| {
            Condition::TypeTest(TypeTest {
                place,
                member,
                member_span,
                binding,
                span: e.span(),
            })
        })
        .labelled("type test");

    type_test.or(expr_parser().map(Condition::Expr)).boxed()
}
