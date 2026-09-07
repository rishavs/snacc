use crate::ast::{
    Arg, BinaryOp, Block, BlockElement, Condition, Expr, ExternFunc, FieldDecl, Func, IfForm,
    MethodDecl, NumLiteral, Param, ParamMode, PlacePath, PlaceRootName, Program, Span, Spanned,
    StaticDecl, StringPart, TypeBody, TypeDecl, TypeName, TypeRef, TypeTest, UnaryOp,
    UnionMemberDecl, Value,
};
use crate::lexer::Token;
use crate::lexer::{decode_string_content, normalize_line_endings};
use chumsky::{input::ValueInput, prelude::*};
use std::collections::HashMap;
mod expr;
mod stmt;
mod types;

use expr::{condition_parser, expr_parser};
use stmt::{block_element_parser, block_parser};
use types::{builtin_type_parser, param_type_parser, sum_type_parser, type_ref_parser};

fn name_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Spanned<&'src str>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    select! { Token::Ident(ident) => ident }
        .map_with(|name, e| (name, e.span()))
        .labelled("identifier")
}

/// Specification 011 section 5: the permitted declaration sites, named by the
/// diagnostic every other type position produces.
fn place_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, PlacePath<'src>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    select! {
        Token::Ident(name) => PlaceRootName::Name(name),
        Token::SelfKw => PlaceRootName::SelfRef,
    }
    .map_with(|root, e| (root, e.span()))
    .then(
        just(Token::Ctrl('.'))
            .ignore_then(name_parser())
            .repeated()
            .collect::<Vec<_>>(),
    )
    .map_with(|((root, root_span), fields), e| PlacePath {
        root,
        root_span,
        fields,
        span: e.span(),
    })
    .labelled("place")
}

/// Value-producing expressions only. `let`, assignment, `while`, `break`, and
/// `if` are block elements, so none of them can appear in an operand,
/// argument, initializer, or condition position.
enum Item<'src> {
    Func(Spanned<&'src str>, Func<'src>),
    Extern(Spanned<&'src str>, ExternFunc<'src>),
    Type(TypeDecl<'src>),
    Method(MethodDecl<'src>),
    Static(StaticDecl<'src>),
    Element(Spanned<BlockElement<'src>>),
}

pub fn program_parser<'tokens, 'src: 'tokens, I>()
-> impl Parser<'tokens, I, Program<'src>, extra::Err<Rich<'tokens, Token<'src>, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token<'src>, Span = Span>,
{
    let ident = select! { Token::Ident(ident) => ident };
    let name = name_parser();
    let type_ref = type_ref_parser();

    let param = ident
        .map_with(|name, e| (name, e.span()))
        .then_ignore(just(Token::Ctrl(':')))
        .then(param_type_parser())
        .map(|((name, name_span), (mode, ty))| Param {
            name,
            mode,
            ty,
            span: name_span,
        });

    let args = param
        .separated_by(just(Token::Ctrl(',')))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::Ctrl('(')), just(Token::Ctrl(')')))
        .labelled("function args")
        .boxed();

    // `: type` is optional: omitting it declares no result.
    let result = just(Token::Ctrl(':'))
        .ignore_then(type_ref.clone())
        .or_not()
        .boxed();

    let field = name
        .clone()
        .then_ignore(just(Token::Ctrl(':')))
        .then(type_ref.clone())
        .then_ignore(just(Token::Ctrl(',')).or_not())
        .map(|((name, name_span), ty)| FieldDecl {
            name,
            name_span,
            ty,
        })
        .labelled("field declaration");

    let struct_body = just(Token::Struct)
        .ignore_then(field.repeated().collect::<Vec<_>>())
        .then_ignore(just(Token::End))
        .boxed();

    // A bare alternative is exactly an empty inline struct member.
    let union_member = just(Token::Ctrl('|'))
        .ignore_then(
            select! {
                Token::TyNil => ("Nil", true),
                Token::Ident(name) => (name, false),
            }
            .map_with(|member, e| (member, e.span())),
        )
        .then(
            just(Token::Is)
                .ignore_then(struct_body.clone())
                .or_not()
                .map(Option::unwrap_or_default),
        )
        .map(|(((name, nil), name_span), fields)| UnionMemberDecl {
            name,
            name_span,
            nil,
            fields,
        })
        .labelled("union member");

    let union_body = just(Token::Union)
        .ignore_then(union_member.repeated().at_least(1).collect::<Vec<_>>())
        .then_ignore(just(Token::End))
        .boxed();

    let type_decl = just(Token::Type)
        .ignore_then(name.clone().labelled("type name"))
        .then(
            name.clone()
                .separated_by(just(Token::Ctrl(',')))
                .at_least(1)
                .collect::<Vec<_>>()
                .delimited_by(just(Token::Op("<")), just(Token::Op(">")))
                .or_not()
                .map(|params| params.unwrap_or_default()),
        )
        .then_ignore(just(Token::Is))
        .then(
            struct_body
                .map(TypeBody::Struct)
                .or(union_body.map(TypeBody::Union))
                .or(type_ref.clone().map(TypeBody::Represented)),
        )
        .map_with(|(((name, name_span), generic_params), body), e| {
            Item::Type(TypeDecl {
                name,
                name_span,
                generic_params,
                body,
                span: e.span(),
            })
        })
        .labelled("type declaration");

    let method_decl = just(Token::Method)
        .ignore_then(
            name.clone()
                .separated_by(just(Token::Ctrl('.')))
                .at_least(1)
                .collect::<Vec<_>>()
                .labelled("method name"),
        )
        .then(args.clone())
        .then(result.clone())
        .then_ignore(just(Token::Do))
        .then(block_parser().then_ignore(just(Token::End)))
        .map_with(|(((path, args), ret), body), e| {
            Item::Method(MethodDecl {
                path,
                args,
                ret,
                span: e.span(),
                body,
            })
        })
        .labelled("method declaration");

    let builtin_static_target = builtin_type_parser()
        .map_with(|receiver, e| (TypeRef::Builtin(receiver), e.span()))
        .then_ignore(just(Token::Ctrl('.')))
        .then(name.clone());
    let named_static_target = name
        .clone()
        .separated_by(just(Token::Ctrl('.')))
        .at_least(2)
        .at_most(3)
        .collect::<Vec<_>>()
        .map_with(|mut path, e| {
            let name = path.pop().expect("at least two path components");
            ((TypeRef::Named(path), e.span()), name)
        });
    let static_decl = just(Token::Static)
        .ignore_then(
            builtin_static_target
                .or(named_static_target)
                .labelled("associated type and function name"),
        )
        .then(args.clone())
        .then(result.clone())
        .then_ignore(just(Token::Do))
        .then(block_parser().then_ignore(just(Token::End)))
        .map_with(|(((target, args), ret), body), e| {
            let (receiver, name) = target;
            Item::Static(StaticDecl {
                receiver,
                name,
                args,
                ret,
                span: e.span(),
                body,
            })
        })
        .labelled("static associated-function declaration");

    let func = just(Token::Fun)
        .ignore_then(name.clone().labelled("function name"))
        .then(
            name.clone()
                .separated_by(just(Token::Ctrl(',')))
                .at_least(1)
                .collect::<Vec<_>>()
                .delimited_by(just(Token::Op("<")), just(Token::Op(">")))
                .or_not()
                .map(|params| params.unwrap_or_default()),
        )
        .then(args.clone())
        .then(result.clone())
        .then_ignore(just(Token::Do))
        .then(block_parser().then_ignore(just(Token::End)))
        .map_with(|((((name, generic_params), args), ret), body), e| {
            Item::Func(
                name,
                Func {
                    generic_params,
                    args,
                    ret,
                    span: e.span(),
                    body,
                },
            )
        })
        .labelled("function");

    let extern_func = just(Token::Extern)
        .ignore_then(just(Token::Rust))
        .ignore_then(select! { Token::Str(symbol) => symbol }.labelled("link symbol"))
        .then_ignore(just(Token::Fun))
        .then(name.labelled("function name"))
        .then(args)
        .then(result)
        .map_with(|(((symbol, name), args), ret), e| {
            Item::Extern(
                name,
                ExternFunc {
                    symbol,
                    args,
                    ret,
                    span: e.span(),
                },
            )
        })
        .labelled("external Rust function");

    // None of `fun`, `extern`, `type`, or `method` can begin a block element, so
    // declarations and executable elements interleave freely at the top level.
    let item = extern_func
        .or(func)
        .or(type_decl)
        .or(method_decl)
        .or(static_decl)
        .or(block_element_parser().map(Item::Element));

    item.repeated()
        .collect::<Vec<_>>()
        .map_with(|items, e| (items, e.span()))
        .validate(|(items, span), _, emitter| {
            let mut funcs = HashMap::new();
            let mut externs = HashMap::new();
            let mut link_names = HashMap::new();
            let mut types = Vec::new();
            let mut methods = Vec::new();
            let mut statics = Vec::new();
            let mut elements = Vec::new();
            for item in items {
                match item {
                    Item::Func((name, name_span), function) => {
                        if funcs.contains_key(name) || externs.contains_key(name) {
                            emitter.emit(Rich::custom(
                                name_span,
                                format!("Function '{name}' already exists"),
                            ));
                        } else {
                            funcs.insert(name, function);
                        }
                    }
                    Item::Extern((name, name_span), function) => {
                        if funcs.contains_key(name) || externs.contains_key(name) {
                            emitter.emit(Rich::custom(
                                name_span,
                                format!("Function '{name}' already exists"),
                            ));
                            continue;
                        }
                        if let Some(previous) = link_names.insert(function.symbol, name_span) {
                            emitter.emit(Rich::custom(
                                function.span,
                                format!(
                                    "External link symbol '{}' is already declared at {}..{}",
                                    function.symbol, previous.start, previous.end
                                ),
                            ));
                        }
                        externs.insert(name, function);
                    }
                    Item::Type(declaration) => types.push(declaration),
                    Item::Method(declaration) => methods.push(declaration),
                    Item::Static(declaration) => statics.push(declaration),
                    Item::Element(element) => elements.push(element),
                }
            }
            Program {
                funcs,
                externs,
                types,
                methods,
                statics,
                body: Block { elements, span },
            }
        })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
