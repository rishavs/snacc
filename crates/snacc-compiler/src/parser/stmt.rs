use super::*;

/// One block element. Nested blocks (loop bodies, `if` branches) are parsed by
/// recursing through this same parser, so `block_element_parser` owns the whole
/// statement grammar.
pub(super) fn block_element_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<BlockElement<'src>>, extra::Err<Rich<'tokens, Token<'src>, Span>>>
+ Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    let mut element = Recursive::declare();

    // A block runs until the enclosing terminator (`end`, `elseif`, `else`, or
    // end of input); none of those can begin a block element, so `repeated`
    // stops on its own without an explicit guard.
    let block = element
        .clone()
        .repeated()
        .collect::<Vec<_>>()
        .map_with(|elements, e| Block {
            elements,
            span: e.span(),
        });

    let name = name_parser();
    let expr = expr_parser();

    let let_ = just(Token::Let)
        .ignore_then(just(Token::Mut).or_not().map(|m| m.is_some()))
        .then(name.clone())
        .then_ignore(just(Token::Ctrl(':')))
        .then(type_ref_parser())
        .then_ignore(just(Token::Op("=")))
        .then(expr.clone())
        .map(
            |(((mutable, (name, name_span)), ty), value)| BlockElement::Let {
                mutable,
                name,
                name_span,
                ty,
                value,
            },
        )
        .labelled("variable declaration");

    // The single `=` token only begins an assignment when a place sits at
    // block-element start; `==` is a distinct token, so no lookahead is needed
    // beyond chumsky's ordinary backtracking into the expression alternative.
    let assign = place_parser()
        .then_ignore(just(Token::Op("=")))
        .then(expr.clone())
        .map(|(place, value)| BlockElement::Assign { place, value })
        .labelled("assignment");

    let indexed_assign = expr
        .clone()
        .then_ignore(just(Token::Op("=")))
        .then(expr.clone())
        .map(|(target, value)| BlockElement::IndexedAssign { target, value })
        .labelled("indexed assignment");

    let while_ = just(Token::While)
        .ignore_then(expr.clone())
        .then_ignore(just(Token::Do))
        .then(block.clone())
        .then_ignore(just(Token::End))
        .map_with(|(condition, body), e| BlockElement::While {
            condition,
            body,
            span: e.span(),
        })
        .labelled("while statement");

    let for_ = just(Token::For)
        .ignore_then(name.clone())
        .then(just(Token::Ctrl(',')).ignore_then(name.clone()).or_not())
        .then_ignore(just(Token::In))
        .then(expr.clone())
        .then_ignore(just(Token::Do))
        .then(block.clone())
        .then_ignore(just(Token::End))
        .map_with(|(((value, key), iterable), body), e| BlockElement::For {
            value,
            key,
            iterable,
            body,
            span: e.span(),
        })
        .labelled("for statement");

    let break_ = just(Token::Break).map_with(|_, e| BlockElement::Break(e.span()));

    let defer_ = just(Token::Defer)
        .to(false)
        .or(just(Token::DeferOnError).to(true))
        .then(expr.clone())
        .map_with(|(on_error, call), e| BlockElement::Defer {
            on_error,
            call,
            span: e.span(),
        })
        .labelled("deferred call");

    // `return-statement = "return", [ expression ]` (Specification 026
    // section 4). `return` is bare only when the next token closes the
    // current block (`end`, `elseif`, `else`) or ends the token stream (the
    // top-level program boundary); `.rewind()` peeks that fact without
    // consuming it. Every other token must begin one maximal expression, so
    // `return let x: Int64 = 1` fails to parse rather than silently
    // splitting into a bare return followed by a `let` -- `let` cannot begin
    // an expression, matching section 4 rule 2's "must begin one expression".
    let return_boundary = just(Token::End)
        .ignored()
        .or(just(Token::ElseIf).ignored())
        .or(just(Token::Else).ignored())
        .or(end())
        .rewind();
    let return_ = just(Token::Return)
        .ignore_then(return_boundary.map(|()| None).or(expr.clone().map(Some)))
        .map_with(|value, e| BlockElement::Return(value, e.span()))
        .labelled("return statement");

    let arm = condition_parser()
        .then_ignore(just(Token::Then))
        .then(block.clone());
    let if_ = just(Token::If)
        .ignore_then(arm.clone())
        .then(
            just(Token::ElseIf)
                .ignore_then(arm)
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then(just(Token::Else).ignore_then(block).or_not())
        .then_ignore(just(Token::End))
        .map_with(|((first, mut arms), else_branch), e| {
            arms.insert(0, first);
            BlockElement::If(IfForm {
                arms,
                else_branch,
                span: e.span(),
            })
        })
        .labelled("if form");

    element.define(
        let_.or(while_)
            .or(for_)
            .or(break_)
            .or(defer_)
            .or(return_)
            .or(if_)
            .or(assign)
            .or(indexed_assign)
            .or(expr.map(BlockElement::Expr))
            .map_with(|element, e| (element, e.span()))
            .boxed(),
    );

    element
}

pub(super) fn block_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Block<'src>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    block_element_parser()
        .repeated()
        .collect::<Vec<_>>()
        .map_with(|elements, e| Block {
            elements,
            span: e.span(),
        })
}
