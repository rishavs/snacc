use super::*;
use crate::lexer;

fn parse(source: &str) -> Program<'_> {
    let (tokens, lex_errors) = lexer::lexer().parse(source).into_output_errors();
    assert!(lex_errors.is_empty(), "lex errors: {lex_errors:?}");
    let tokens = tokens.unwrap();
    let (program, parse_errors) = program_parser()
        .parse(
            tokens
                .as_slice()
                .map((source.len()..source.len()).into(), |(token, span)| {
                    (token, span)
                }),
        )
        .into_output_errors();
    assert!(parse_errors.is_empty(), "parse errors: {parse_errors:?}");
    program.expect("a program without parse errors has a syntax tree")
}

fn assert_parses(source: &str) {
    parse(source);
}

fn assert_rejects(source: &str) {
    let (tokens, lex_errors) = lexer::lexer().parse(source).into_output_errors();
    if !lex_errors.is_empty() {
        return;
    }
    let tokens = tokens.unwrap();
    let (_, parse_errors) = program_parser()
        .parse(
            tokens
                .as_slice()
                .map((source.len()..source.len()).into(), |(token, span)| {
                    (token, span)
                }),
        )
        .into_output_errors();
    assert!(
        !parse_errors.is_empty(),
        "expected a parse error for: {source}"
    );
}

fn builtin(ty: &Spanned<TypeRef<'_>>) -> TypeName {
    match &ty.0 {
        TypeRef::Builtin(name) => *name,
        other => panic!("expected a built-in type, got {other:?}"),
    }
}

#[test]
fn parses_lua_style_blocks_and_conditions() {
    assert_parses(
        "fun isfoo(a: Int64): Bool do\n    a == 0\nend\n\nif x > 0 then\n    \"yes\"\nelseif x == 0 then \"maybe\"\nelse\n    \"no\"\nend",
    );
}

#[test]
fn newlines_are_not_significant() {
    assert_parses(
        "fun isfoo(a: Int64): Bool do a == 0 end if x > 0 then \"yes\" elseif x == 0 then \"maybe\" else \"no\" end",
    );
}

#[test]
fn parses_while_as_a_statement() {
    let program = parse("while false do print(1) end print(2)");
    assert_eq!(program.body.elements.len(), 2);
    assert!(matches!(
        program.body.elements[0].0,
        BlockElement::While { .. }
    ));
    assert!(matches!(program.body.elements[1].0, BlockElement::Expr(_)));
}

#[test]
fn rejects_while_in_an_expression_position() {
    assert_rejects("print(while false do 7 end)");
    assert_rejects("let x: Int64 = while false do 7 end");
    assert_rejects("1 + while false do 7 end");
}

#[test]
fn rejects_if_in_an_expression_position() {
    assert_rejects("print(if true then 1 else 2 end)");
    assert_rejects("let x: Int64 = if true then 1 else 2 end");
}

#[test]
fn parses_break_inside_a_while_body() {
    let program = parse("while true do break end");
    let BlockElement::While { body, .. } = &program.body.elements[0].0 else {
        panic!("expected a while statement");
    };
    assert_eq!(body.elements.len(), 1);
    assert!(matches!(body.elements[0].0, BlockElement::Break(_)));
}

#[test]
fn parses_a_bare_return_immediately_before_every_block_boundary() {
    for source in [
        "fun f() do return end",
        "fun f() do if true then return else return end end",
        "fun f() do if true then return elseif false then return end end",
    ] {
        assert_parses(source);
    }
    let program = parse("fun f() do return end");
    let BlockElement::Return(value, _) = &program.funcs["f"].body.elements[0].0 else {
        panic!("expected a return statement");
    };
    assert!(value.is_none());
}

#[test]
fn parses_a_valued_return_everywhere_else() {
    let program = parse("fun f(): Int64 do return 1 + 2 end");
    let BlockElement::Return(value, _) = &program.funcs["f"].body.elements[0].0 else {
        panic!("expected a return statement");
    };
    assert!(value.is_some());
}

#[test]
fn a_bare_return_is_never_followed_by_another_element_in_the_same_block() {
    let program = parse("fun f() do return print(1) end");
    assert_eq!(program.funcs["f"].body.elements.len(), 1);
    let BlockElement::Return(value, _) = &program.funcs["f"].body.elements[0].0 else {
        panic!("expected a return statement");
    };
    assert!(matches!(value.as_ref().unwrap().0, Expr::Print(_)));
}

#[test]
fn rejects_a_return_immediately_followed_by_a_non_expression_keyword() {
    assert_rejects("fun f() do return let x: Int64 = 1 end");
}

#[test]
fn return_is_reserved_and_not_an_identifier() {
    assert_rejects("let return: Int64 = 1");
    assert_rejects("fun return(): Int64 do 1 end");
}

#[test]
fn parses_a_function_without_a_result_type() {
    let program = parse("fun announce(value: Int64) do print(value) end");
    assert!(program.funcs["announce"].ret.is_none());
}

#[test]
fn parses_a_function_with_a_result_type() {
    let program = parse("fun double(value: Int64): Int64 do value * 2 end");
    assert_eq!(
        builtin(program.funcs["double"].ret.as_ref().unwrap()),
        TypeName::Int64
    );
}

#[test]
fn parses_a_bridge_without_a_result_type() {
    let program = parse("extern rust \"snacc_user_log\" fun log(value: Int64)");
    assert!(program.externs["log"].ret.is_none());
}

#[test]
fn parses_raw_strings_without_escape_processing() {
    let program = parse("let path: String = r#\"C:\\\\snacc\\n\"#");
    let BlockElement::Let { value, .. } = &program.body.elements[0].0 else {
        panic!("expected a raw string declaration");
    };
    assert!(matches!(
        &value.0,
        Expr::Value(Value::Str(text)) if text == "C:\\\\snacc\\n"
    ));
}

#[test]
fn parses_typed_rust_bridge_declaration() {
    let program = parse(
        "extern rust \"snacc_user_double\" fun rust_double(value: Int64): Int64\nprint(rust_double(2))",
    );
    assert_eq!(
        builtin(program.externs["rust_double"].ret.as_ref().unwrap()),
        TypeName::Int64
    );
}

#[test]
fn parses_an_if_without_an_else() {
    let program = parse("if true then print(1) end");
    assert_eq!(program.body.elements.len(), 1);
    let BlockElement::If(form) = &program.body.elements[0].0 else {
        panic!("expected an if form");
    };
    assert_eq!(form.arms.len(), 1);
    assert!(form.else_branch.is_none());
}

#[test]
fn parses_an_elseif_chain_as_one_block_element() {
    let program = parse("if x > 0 then print(1) elseif x == 0 then print(2) else print(3) end");
    let BlockElement::If(form) = &program.body.elements[0].0 else {
        panic!("expected an if form");
    };
    assert_eq!(form.arms.len(), 2);
    assert!(form.else_branch.is_some());
}

#[test]
fn parses_declarations_and_assignments() {
    let program = parse("let mut total: Int64 = 1 total = total + 1 print(total)");
    assert_eq!(program.body.elements.len(), 3);
    assert!(matches!(
        program.body.elements[0].0,
        BlockElement::Let { mutable: true, .. }
    ));
    assert!(matches!(
        program.body.elements[1].0,
        BlockElement::Assign { .. }
    ));
}

#[test]
fn a_leading_name_only_begins_an_assignment_on_a_single_equals() {
    let program = parse("x == 1");
    assert!(matches!(program.body.elements[0].0, BlockElement::Expr(_)));
    let program = parse("x = 1");
    assert!(matches!(
        program.body.elements[0].0,
        BlockElement::Assign { .. }
    ));
}

#[test]
fn one_line_and_multi_line_block_elements_parse_identically() {
    let one_line = parse("let x: Int64 = 10 print(x)");
    let multi_line = parse("let x: Int64 = 10\nprint(x)");
    assert_eq!(one_line.body.elements.len(), 2);
    assert_eq!(
        format!("{:?}", one_line.body.elements[0].0),
        format!("{:?}", multi_line.body.elements[0].0)
    );
}

#[test]
fn parses_every_new_type_name_in_every_type_position() {
    for (name, expected) in [
        ("Byte", TypeName::Byte),
        ("UInt16", TypeName::UInt16),
        ("UInt32", TypeName::UInt32),
        ("UInt64", TypeName::UInt64),
        ("Float32", TypeName::Float32),
    ] {
        let source = format!(
            "extern rust \"snacc_user_edge\" fun edge(value: {name}): {name}\n\
             fun identity(value: {name}): {name} do value end"
        );
        let program = parse(&source);
        assert_eq!(builtin(&program.funcs["identity"].args[0].ty), expected);
        assert_eq!(
            builtin(program.funcs["identity"].ret.as_ref().unwrap()),
            expected
        );
        assert_eq!(builtin(&program.externs["edge"].args[0].ty), expected);
        assert_eq!(
            builtin(program.externs["edge"].ret.as_ref().unwrap()),
            expected
        );
    }
    let program = parse("let byte: Byte = 1u8 let ratio: Float32 = 0.5f32");
    let BlockElement::Let { ty, .. } = &program.body.elements[0].0 else {
        panic!("expected a declaration");
    };
    assert_eq!(builtin(ty), TypeName::Byte);
}

#[test]
fn rejects_the_removed_uint8_source_type_name() {
    assert_rejects("let byte: UInt8 = 1u8");
    assert_rejects("fun take(value: UInt8) do print(value) end");
}

#[test]
fn rejects_semicolons() {
    assert_rejects("while false do print(1) end; print(2)");
    assert_rejects("let x: Int64 = 1; print(x)");
}

#[test]
fn parses_a_represented_type_declaration() {
    let program = parse("type UserId is Int64");
    assert_eq!(program.types.len(), 1);
    assert_eq!(program.types[0].name, "UserId");
    assert!(matches!(program.types[0].body, TypeBody::Represented(_)));
}

#[test]
fn parses_a_struct_with_and_without_a_trailing_comma() {
    for source in [
        "type Point is struct x: Float64, y: Float64, end",
        "type Point is struct x: Float64, y: Float64 end",
        "type Point is struct\n    x: Float64\n    y: Float64\nend",
    ] {
        let program = parse(source);
        let TypeBody::Struct(fields) = &program.types[0].body else {
            panic!("expected a struct body for {source}");
        };
        assert_eq!(fields.len(), 2, "{source}");
        assert_eq!(fields[0].name, "x");
        assert_eq!(fields[1].name, "y");
    }
}

#[test]
fn parses_an_empty_struct() {
    let program = parse("type Marker is struct end");
    let TypeBody::Struct(fields) = &program.types[0].body else {
        panic!("expected a struct body");
    };
    assert!(fields.is_empty());
}

#[test]
fn parses_bare_inline_and_nil_union_members() {
    let program = parse(
        "type Shape is union\n\
         | Circle is struct radius: Int64, end\n\
         | Point\n\
         | Nil\n\
         end",
    );
    let TypeBody::Union(members) = &program.types[0].body else {
        panic!("expected a union body");
    };
    assert_eq!(members.len(), 3);
    assert_eq!(members[0].fields.len(), 1);
    assert!(members[1].fields.is_empty() && !members[1].nil);
    assert!(members[2].nil && members[2].name == "Nil");
}

#[test]
fn parses_top_level_and_member_method_receivers() {
    let program = parse(
        "method Point.length(): Float64 do 1.0 end\n\
         method Shape.Circle.area(): Int64 do 1 end",
    );
    assert_eq!(program.methods.len(), 2);
    let (receiver, name) = program.methods[0].split().expect("two components");
    assert_eq!(receiver.len(), 1);
    assert_eq!(name.0, "length");
    let (receiver, name) = program.methods[1].split().expect("three components");
    assert_eq!(
        receiver.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        vec!["Shape", "Circle"]
    );
    assert_eq!(name.0, "area");
}

#[test]
fn parses_named_constructor_arguments_and_nested_postfix_chains() {
    let program = parse("print(Point(x: 3.0, y: 4.0).x)\nprint(a.b.c(1).d)");
    assert_eq!(program.body.elements.len(), 2);
}

#[test]
fn parses_field_assignment_through_a_field_path() {
    let program = parse("entity.position.x = 1.0");
    let BlockElement::Assign { place, .. } = &program.body.elements[0].0 else {
        panic!("expected an assignment");
    };
    assert_eq!(place.fields.len(), 2);
    assert!(matches!(place.root, PlaceRootName::Name("entity")));
}

#[test]
fn parses_whole_self_assignment_inside_a_method() {
    let program = parse("method Point.reset() do self = Point(x: 0.0, y: 0.0) end");
    let BlockElement::Assign { place, .. } = &program.methods[0].body.elements[0].0 else {
        panic!("expected an assignment");
    };
    assert!(matches!(place.root, PlaceRootName::SelfRef));
    assert!(place.fields.is_empty());
}

#[test]
fn parses_type_tests_with_and_without_a_binding() {
    let program = parse(
        "if shape is Shape.Circle(circle) then print(1) elseif shape is Nil then print(2) end",
    );
    let BlockElement::If(form) = &program.body.elements[0].0 else {
        panic!("expected an if form");
    };
    let Condition::TypeTest(first) = &form.arms[0].0 else {
        panic!("expected a type test");
    };
    assert_eq!(
        first.member.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        vec!["Shape", "Circle"]
    );
    assert_eq!(first.binding.map(|(name, _)| name), Some("circle"));
    let Condition::TypeTest(second) = &form.arms[1].0 else {
        panic!("expected a type test");
    };
    assert_eq!(second.member[0].0, "Nil");
    assert!(second.binding.is_none());
}

#[test]
fn a_comparison_still_parses_as_an_ordinary_condition() {
    let program = parse("if x > 0 then print(1) end");
    let BlockElement::If(form) = &program.body.elements[0].0 else {
        panic!("expected an if form");
    };
    assert!(matches!(form.arms[0].0, Condition::Expr(_)));
}

#[test]
fn parses_a_qualified_type_in_every_type_position() {
    assert_parses(
        "type Shape is union | Circle is struct radius: Int64, end end\n\
         type Holder is struct shape: Shape.Circle, end\n\
         fun take(value: Shape.Circle): Shape.Circle do value end\n\
         let held: Shape.Circle = Shape.Circle(radius: 1)",
    );
}

#[test]
fn parses_a_builtin_type_name_as_an_unwrapping_call_head() {
    let program = parse("type UserId is Int64\nlet id: UserId = UserId(1)\nprint(Int64(id))");
    assert_eq!(program.body.elements.len(), 2);
}

#[test]
fn rejects_malformed_declaration_delimiters() {
    for source in [
        "type Point is struct x: Float64,",
        "type Shape is union | end",
        "type Shape is union end",
        "method Point.length(): Float64 do 1.0",
        "type is Int64",
        "method () do end",
    ] {
        assert_rejects(source);
    }
}

#[test]
fn mut_is_reserved_outside_a_declaration() {
    assert_rejects("fun f(mut value: Int64): Int64 do value end");
    assert_rejects("let mut: Int64 = 1");
}

fn parse_errors(source: &str) -> Vec<String> {
    let (tokens, lex_errors) = lexer::lexer().parse(source).into_output_errors();
    assert!(lex_errors.is_empty(), "lex errors: {lex_errors:?}");
    let tokens = tokens.unwrap();
    let (_, errors) = program_parser()
        .parse(
            tokens
                .as_slice()
                .map((source.len()..source.len()).into(), |(token, span)| {
                    (token, span)
                }),
        )
        .into_output_errors();
    errors.iter().map(ToString::to_string).collect()
}

fn assert_parse_error_contains(source: &str, needle: &str) {
    let errors = parse_errors(source);
    assert!(
        errors.iter().any(|error| error.contains(needle)),
        "expected a parse error containing {needle:?} for {source}, got: {errors:?}"
    );
}

#[test]
fn ref_is_a_reserved_word_and_not_an_identifier() {
    assert_rejects("let Ref: Int64 = 1");
    assert_rejects("fun Ref(value: Int64): Int64 do value end");
}

#[test]
fn parses_a_reference_parameter_in_every_permitted_declaration() {
    for source in [
        "fun add_into(x: Int64, y: Int64, result: Ref<Int64>) do result = x + y end",
        "type Point is struct x: Float64, end\n\
         method Point.give(other: Ref<Float64>) do other = self.x end",
        "extern rust \"snacc_user_bump\" fun bump(value: Ref<Int64>)",
    ] {
        assert_parses(source);
    }
}

#[test]
fn a_reference_parameter_records_its_mode_and_referent_type() {
    let program = parse("fun f(a: Int64, b: Ref<Int64>) do b = a end");
    let args = &program.funcs["f"].args;
    assert_eq!(args[0].mode, ParamMode::Value);
    assert_eq!(args[1].mode, ParamMode::Reference);
    assert_eq!(builtin(&args[1].ty), TypeName::Int64);
}

#[test]
fn a_user_defined_referent_keeps_its_written_path() {
    let program = parse(
        "type Shape is union | Circle is struct radius: Int64, end | Nil end\n\
         fun grow(shape: Ref<Shape.Circle>) do shape.radius = 1 end",
    );
    let TypeRef::Named(segments) = &program.funcs["grow"].args[0].ty.0 else {
        panic!("expected a qualified referent path");
    };
    assert_eq!(
        segments.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        vec!["Shape", "Circle"]
    );
}

#[test]
fn rejects_a_reference_in_every_other_type_position() {
    for source in [
        "fun f(value: Int64): Ref<Int64> do value end",
        "method Point.f(): Ref<Int64> do 1 end",
        "extern rust \"snacc_user_f\" fun f(value: Int64): Ref<Int64>",
        "let saved: Ref<Int64> = 1",
        "type Holder is struct value: Ref<Int64>, end",
        "type Alias is Ref<Int64>",
        "type Shape is union | Circle is struct radius: Ref<Int64>, end | Nil end",
    ] {
        assert_parse_error_contains(source, "only valid as the direct type of a function");
    }
}

#[test]
fn rejects_a_nested_reference() {
    assert_parse_error_contains(
        "fun f(value: Ref<Ref<Int64>>) do print(1) end",
        "cannot contain another reference",
    );
}

#[test]
fn rejects_an_authored_self_annotation() {
    assert_rejects("method Point.f(self: Ref<Point>) do print(1) end");
}

#[test]
fn ordered_comparisons_still_parse_as_expressions() {
    let program = parse("print(a < b) print(a > b) print(a <= b) print(a >= b)");
    assert_eq!(program.body.elements.len(), 4);
}

#[test]
fn boolean_precedence_and_unary_negation_are_grouped_by_the_contract() {
    let program = parse("let value: Bool = !!true or false and true");
    let BlockElement::Let { value, .. } = &program.body.elements[0].0 else {
        panic!("expected a declaration")
    };
    let Expr::Binary(left, BinaryOp::Or, right) = &value.0 else {
        panic!("expected the outer operator to be 'or'")
    };
    assert!(matches!(&left.0, Expr::Unary(UnaryOp::Not, _)));
    assert!(matches!(&right.0, Expr::Binary(_, BinaryOp::And, _)));
}

#[test]
fn comparison_chaining_is_rejected_at_one_expression_level() {
    for source in ["1 < 2 > 0", "1 == 1 == true", "1 < 2 <= 3"] {
        assert_rejects(source);
    }
}

#[test]
fn boolean_operators_are_reserved_keywords() {
    assert_rejects("let and: Bool = true");
    assert_rejects("let or: Bool = false");
}

fn sum_member_names(ty: &Spanned<TypeRef<'_>>) -> Vec<String> {
    match &ty.0 {
        TypeRef::Sum(members) => members.iter().map(|(m, _)| m.to_string()).collect(),
        other => panic!("expected an inline sum type, got {other:?}"),
    }
}

fn let_ty<'src>(program: &Program<'src>, index: usize) -> Spanned<TypeRef<'src>> {
    let BlockElement::Let { ty, .. } = &program.body.elements[index].0 else {
        panic!("expected a variable declaration");
    };
    (ty.0.clone(), ty.1)
}

#[test]
fn parses_an_inline_sum_type_in_every_value_type_position() {
    for source in [
        "type Point is struct x: Float64, end\nfun read(): Byte | Nil do nil end",
        "type Point is struct x: Float64, end\nfun take(value: Byte | Nil) do print(1) end",
        "type Point is struct x: Float64, end\nlet result: Byte | Nil = nil",
        "type Point is struct x: Float64, end\ntype Holder is struct value: Byte | Nil, end",
        "type Point is struct x: Float64, end\n\
         extern rust \"snacc_user_maybe\" fun maybe(): Byte | Nil",
        "type Point is struct x: Float64, end\n\
         method Point.length(): Byte | Nil do nil end",
    ] {
        assert_parses(source);
    }
}

#[test]
fn a_sum_type_records_every_member_in_source_order() {
    let program = parse("let result: Byte | Bool | Nil = nil");
    let ty = let_ty(&program, 0);
    assert_eq!(sum_member_names(&ty), vec!["Byte", "Bool", "Nil"]);
}

#[test]
fn a_single_member_never_wraps_in_a_sum_node() {
    let program = parse("let value: (Byte) = 1u8");
    let ty = let_ty(&program, 0);
    assert_eq!(builtin(&ty), TypeName::Byte);
}

#[test]
fn parses_parenthesized_grouping_with_a_nested_sum_member() {
    let program = parse("let value: (Byte | Bool) | Nil = nil");
    let ty = let_ty(&program, 0);
    let TypeRef::Sum(members) = &ty.0 else {
        panic!("expected an inline sum type");
    };
    assert_eq!(members.len(), 2);
    assert_eq!(sum_member_names(&members[0]), vec!["Byte", "Bool"]);
    assert_eq!(members[1].0.to_string(), "Nil");
}

#[test]
fn whitespace_around_the_sum_operator_has_no_significance() {
    let tight = parse("let value: Byte|Bool|Nil = nil");
    let spaced = parse("let value: Byte | Bool | Nil = nil");
    assert_eq!(
        sum_member_names(&let_ty(&tight, 0)),
        sum_member_names(&let_ty(&spaced, 0))
    );
}

#[test]
fn rejects_malformed_sum_separators() {
    for source in [
        "let value: | Byte = 1u8",
        "let value: Byte | = 1u8",
        "let value: Byte || Bool = nil",
        "let value: () = 1u8",
    ] {
        assert_rejects(source);
    }
}

#[test]
fn rejects_a_reference_as_a_sum_member() {
    for source in [
        "let value: Ref<Byte> | Nil = nil",
        "fun f(value: Byte | Ref<Nil>) do print(1) end",
    ] {
        assert_rejects(source);
    }
}

#[test]
fn a_reference_referent_may_be_a_sum_type() {
    assert_parses("fun replace(value: Ref<Byte | Nil>) do value = nil end");
}

#[test]
fn the_sum_operator_never_parses_as_an_expression_operator() {
    assert_rejects("print(1 | 2)");
    assert_rejects("let x: Int64 = 1 | 2");
}

#[test]
fn parses_a_type_test_naming_a_builtin_direct_member() {
    let program = parse(
        "fun show(value: Byte | Nil) do \
         if value is Byte(byte) then print(1) elseif value is Nil then print(2) end end",
    );
    let BlockElement::If(form) = &program.funcs["show"].body.elements[0].0 else {
        panic!("expected an if form");
    };
    let Condition::TypeTest(first) = &form.arms[0].0 else {
        panic!("expected a type test");
    };
    assert_eq!(first.member[0].0, "Byte");
    assert_eq!(first.binding.map(|(name, _)| name), Some("byte"));
    let Condition::TypeTest(second) = &form.arms[1].0 else {
        panic!("expected a type test");
    };
    assert_eq!(second.member[0].0, "Nil");
    assert!(second.binding.is_none());
}

#[test]
fn box_is_a_reserved_word_and_not_an_identifier() {
    assert_rejects("let Box: Int64 = 1");
    assert_rejects("fun Box(value: Int64): Int64 do value end");
    assert_rejects("type Box is Int64");
}

#[test]
fn the_box_expression_keyword_is_reserved_and_not_an_identifier() {
    assert_rejects("let box: Int64 = 1");
    assert_rejects("fun box(value: Int64): Int64 do value end");
}

#[test]
fn parses_a_box_type_in_every_ordinary_value_type_position() {
    for source in [
        "type Point is struct x: Int64, end\nlet held: Box<Point> = box(Point(x: 1))",
        "type Point is struct x: Int64, end\n\
         type Holder is struct value: Box<Point>, end",
        "type Point is struct x: Int64, end\n\
         fun take(value: Box<Point>): Box<Point> do value end",
        "type Point is struct x: Int64, end\n\
         method Point.wrap(): Box<Point> do box(self) end",
        "type Point is struct x: Int64, end\n\
         extern rust \"snacc_user_box_edge\" fun edge(value: Box<Point>): Box<Point>",
        "type Point is struct x: Int64, end\n\
         type Shape is union | Circle is struct value: Box<Point>, end | Nil end",
    ] {
        assert_parses(source);
    }
}

#[test]
fn parses_nested_and_composed_box_types() {
    for source in [
        "type Point is struct x: Int64, end\n\
         let nested: Box<Box<Point>> = box(box(Point(x: 1)))",
        "type Point is struct x: Int64, end\n\
         let member: Box<Point> | Bool = true",
        "type Point is struct x: Int64, end\n\
         fun grow(value: Ref<Box<Point>>) do print(1) end",
    ] {
        assert_parses(source);
    }
}

#[test]
fn rejects_a_reference_as_a_box_pointee() {
    assert_rejects("let value: Box<Ref<Int64>> = box(1)");
}

#[test]
fn parses_a_box_allocation_expression_in_every_expression_position() {
    for source in [
        "type Point is struct x: Int64, end\nlet held: Box<Point> = box(Point(x: 1))",
        "type Point is struct x: Int64, end\nprint(box(1) == box(1))",
        "type Point is struct x: Int64, end\n\
         fun make(): Box<Point> do box(Point(x: 1)) end",
    ] {
        assert_parses(source);
    }
}

#[test]
fn rejects_the_capitalized_box_type_name_used_as_a_call_head() {
    assert_rejects("print(Box(1))");
}
