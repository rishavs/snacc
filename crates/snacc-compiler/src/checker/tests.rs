use super::*;
use crate::ast::Block;

fn errors(source: &str) -> Vec<Error> {
    let syntax = crate::parse(source).unwrap_or_else(|d| panic!("{source} should parse: {d:?}"));
    match check(&syntax) {
        Err(Failure::Source(errors)) => errors,
        Err(Failure::Unknown(detail)) => panic!("unexpected compiler bug: {detail}"),
        Ok(_) => panic!("expected a type error for: {source}"),
    }
}

fn assert_checks(source: &str) -> Program {
    let syntax = crate::parse(source).unwrap_or_else(|d| panic!("{source} should parse: {d:?}"));
    check(&syntax).unwrap_or_else(|failure| panic!("{source} should check: {failure:?}"))
}

fn assert_error_contains(source: &str, needle: &str) {
    let errors = errors(source);
    assert!(
        errors.iter().any(|error| error.msg.contains(needle)),
        "expected an error containing {needle:?} for {source}, got: {errors:?}"
    );
}

fn assert_rejected_by_parser(source: &str) {
    assert!(
        crate::parse(source).is_err(),
        "expected a parse error for: {source}"
    );
}

#[test]
fn parser_recovery_nodes_are_compiler_bugs_after_parsing() {
    let span: Span = (0..0).into();
    let mut funcs = HashMap::new();
    funcs.insert(
        "recovered",
        crate::ast::Func {
            generic_params: Vec::new(),
            args: Vec::new(),
            ret: Some((TypeRef::Builtin(TypeName::Nil), span)),
            span,
            body: Block {
                elements: vec![(BlockElement::Expr((Expr::Error, span)), span)],
                span,
            },
        },
    );
    let program = AstProgram {
        funcs,
        externs: HashMap::new(),
        types: Vec::new(),
        methods: Vec::new(),
        statics: Vec::new(),
        body: Block {
            elements: Vec::new(),
            span,
        },
    };

    match check(&program) {
        Err(Failure::Unknown(detail)) => {
            assert_eq!(detail, "a parser recovery node escaped into type checking");
        }
        _ => panic!("recovery node was not classified as a compiler bug"),
    }
}

#[test]
fn checks_typed_rust_bridge_calls() {
    let program = assert_checks(
        "extern rust \"snacc_user_double\" fun rust_double(value: Int64): Int64\nprint(rust_double(2))",
    );
    assert_eq!(program.externs["rust_double"].symbol, "snacc_user_double");
    assert_eq!(program.externs["rust_double"].result, Some(Ty::Int64));
}

#[test]
fn checked_externs_carry_their_declaration_span() {
    let source = "extern rust \"snacc_user_double\" fun rust_double(value: Int64): Int64\nprint(rust_double(2))";
    let program = assert_checks(source);
    let span = &program.externs["rust_double"].span;
    assert_eq!(span.start, 0);
    assert!(span.end > span.start && span.end <= source.find('\n').unwrap());
}

#[test]
fn rejects_bridge_symbols_that_are_not_rust_identifiers() {
    assert_error_contains(
        "extern rust \"snacc_user_bad-name\" fun bad(): Int64\nprint(0)",
        "valid Rust identifiers",
    );
}

#[test]
fn accepts_bridge_symbols_with_digits_and_underscores() {
    assert_checks("extern rust \"snacc_user_v2_ok\" fun ok(): Int64\nprint(0)");
}

#[test]
fn rejects_duplicate_function_parameter_names() {
    let source = "fun f(a: Int64, a: Int64): Int64 do a end";
    let second_a = source.find(", a: Int64)").map(|i| i + 2).unwrap();
    let errors = errors(source);
    let error = errors
        .iter()
        .find(|error| error.msg.contains("Parameter 'a' already exists"))
        .unwrap_or_else(|| panic!("expected a duplicate-parameter diagnostic, got: {errors:?}"));
    assert_eq!(
        error.span.start, second_a,
        "diagnostic should span the second 'a'"
    );
    assert_eq!(error.span.end, second_a + 1);
}

#[test]
fn accepts_functions_with_distinct_parameter_names() {
    assert_checks("fun f(a: Int64, b: Int64): Int64 do a + b end");
}

// RFC 008 conformance 1: declarations with and without results.

#[test]
fn checks_functions_and_bridges_with_and_without_results() {
    let program = assert_checks(
        "extern rust \"snacc_user_log\" fun log(value: Int64)\n\
             extern rust \"snacc_user_double\" fun rust_double(value: Int64): Int64\n\
             fun announce(value: Int64) do print(value) end\n\
             fun double(value: Int64): Int64 do value * 2 end\n\
             announce(1)\n\
             log(2)\n\
             print(double(3))\n\
             print(rust_double(4))",
    );
    assert_eq!(program.funcs["announce"].result, None);
    assert_eq!(program.funcs["double"].result, Some(Ty::Int64));
    assert_eq!(program.externs["log"].result, None);
    assert_eq!(program.externs["rust_double"].result, Some(Ty::Int64));
}

// RFC 008 conformance 2: no-result calls as block elements, never as values.

#[test]
fn accepts_a_no_result_call_as_a_block_element() {
    let program = assert_checks("fun announce(value: Int64) do print(value) end\nannounce(1)");
    assert!(matches!(program.body.statements[0], TStmt::Call(_, _)));
    assert!(program.body.result.is_none());
}

#[test]
fn rejects_a_no_result_call_in_every_expression_position() {
    let declaration = "fun announce(value: Int64) do print(value) end\n";
    for use_site in [
        "print(announce(1))",
        "let value: Int64 = announce(1)",
        "print(1 + announce(1))",
        "fun wrap(): Int64 do announce(1) end",
        "if announce(1) then print(0) end",
    ] {
        assert_error_contains(
            &format!("{declaration}{use_site}"),
            "declares no result, so its call cannot be used as a value",
        );
    }
}

// RFC 008 conformance 3: value-required bodies reject statements.

#[test]
fn rejects_a_value_required_body_ending_in_a_statement() {
    for body in ["let value: Int64 = 1", "while false do print(1) end"] {
        assert_error_contains(
            &format!("fun f(): Int64 do {body} end"),
            "must end in an expression of type 'Int64'",
        );
    }
}

#[test]
fn rejects_a_value_required_body_ending_in_an_assignment() {
    assert_error_contains(
        "fun f(): Int64 do let mut x: Int64 = 1 x = 2 end",
        "must end in an expression of type 'Int64'",
    );
}

#[test]
fn accepts_a_value_required_body_with_a_leading_statement() {
    assert_checks("fun f(value: Int64): Int64 do let result: Int64 = value * value result end");
}

#[test]
fn accepts_a_statement_loop_followed_by_an_explicit_value() {
    assert_checks(
        "fun zero_after_loop(value: Int64): Int64 do while false do print(value) end 0 end",
    );
}

// RFC 008 conformance 6: break targets and placement.

#[test]
fn rejects_break_outside_a_loop() {
    assert_error_contains("break", "only valid inside a 'while' body");
    assert_error_contains(
        "fun f() do if true then break end end",
        "only valid inside a 'while' body",
    );
}

#[test]
fn accepts_break_inside_a_nested_loop_body() {
    assert_checks("while true do while true do break end break end");
}

#[test]
fn a_loop_target_does_not_outlive_its_body() {
    assert_error_contains(
        "while true do print(1) end break",
        "only valid inside a 'while' body",
    );
}

// RFC 008 conformance 7: statement-form vs value-form `if`.

#[test]
fn statement_form_if_accepts_an_omitted_else() {
    let program = assert_checks("if true then print(1) end");
    assert!(matches!(program.body.statements[0], TStmt::If(_)));
}

#[test]
fn value_form_if_requires_an_else() {
    assert_error_contains(
        "fun f(): Int64 do if true then 1 end end",
        "requires an 'else' branch",
    );
}

#[test]
fn value_form_if_checks_every_branch_against_the_required_type() {
    assert_checks("fun f(c: Bool): Int64 do if c then 1 elseif c then 2 else 3 end end");
    assert_error_contains(
        "fun f(c: Bool): Int64 do if c then 1 else true end end",
        "expected 'Int64', found 'Bool'",
    );
}

#[test]
fn value_form_if_rejects_a_branch_that_ends_in_a_statement() {
    assert_error_contains(
        "fun f(c: Bool): Int64 do if c then print(1) else while false do print(2) end end end",
        "must end in an expression of type 'Int64'",
    );
}

#[test]
fn truthiness_accepts_values_but_rejects_standalone_nil() {
    assert_checks("while 1 do print(1) end");
    assert_checks("if 1 then print(1) end");
    assert_error_contains("if nil then print(1) end", "standalone 'nil'");
}

#[test]
fn predeclared_error_and_return_on_error_are_checked() {
    assert_checks(
        "fun fail(): Int64 | Error do return Error(category: \"x\", header: \"h\", message: \"m\") end\nfun caller(): Int64 | Error do let value: Int64 = return_on_error fail() value end",
    );
}

#[test]
fn return_on_error_supports_nil_statement_form() {
    let program = assert_checks(
        "fun flush(): Nil | Error do return nil end\nfun caller(): Int64 | Error do return_on_error flush() return 1 end",
    );
    assert!(matches!(
        program.funcs["caller"].body.statements[0],
        TStmt::ReturnOnError { .. }
    ));
}

#[test]
fn return_on_error_rejects_nil_expression_form_and_discarded_values() {
    assert_error_contains(
        "fun flush(): Nil | Error do return nil end\nfun caller(): Nil | Error do return_on_error flush() end",
        "at least one non-Nil success member",
    );
    assert_error_contains(
        "fun read(): Int64 | Error do return 1 end\nfun caller(): Int64 | Error do print(0) return_on_error read() return 1 end",
        "statement form requires an operand of exactly 'Nil | Error'",
    );
}

#[test]
fn return_on_error_consumes_the_complete_source_sum() {
    assert_error_contains(
        "fun fail(): Int64 | Error do return Error(category: \"x\", header: \"h\", message: \"m\") end\nfun caller(): Int64 | Error do let value: Int64 | Error = fail() let result: Int64 = return_on_error value value end",
        "already moved",
    );
}

#[test]
fn return_on_error_can_propagate_into_a_larger_result_sum() {
    assert_checks(
        "fun narrow(): Int64 | Error do return 1 end\nfun widen(): Int64 | String | Error do let value: Int64 = return_on_error narrow() value end",
    );
    crate::emit_llvm_ir(
            "fun narrow(): Int64 | Error do return 1 end\nfun widen(): Int64 | String | Error do let value: Int64 = return_on_error narrow() value end\nprint(0)",
        )
        .expect("return_on_error should retag an Error into a larger result sum");
}

#[test]
fn defer_requires_a_no_result_call_and_is_accepted_in_a_block() {
    assert_checks("fun cleanup() do end defer cleanup()");
    assert_error_contains(
        "fun cleanup() do end fun value(): Int64 do defer cleanup() end",
        "must end in an expression",
    );
}

#[test]
fn defer_cleanup_lowers_on_fallthrough_and_return() {
    crate::emit_llvm_ir(
            "fun cleanup() do print(9) end\nfun fallthrough(): Int64 do defer cleanup() 1 end\nfun early(): Int64 do defer cleanup() return 2 end\nprint(fallthrough()) print(early())",
        )
        .expect("defer should lower on both normal and explicit-return exits");
}

#[test]
fn defer_on_error_lowers_on_propagation_only() {
    crate::emit_llvm_ir(
            "fun cleanup() do print(9) end\nfun fail(): Int64 | Error do return Error(category: \"x\", header: \"h\", message: \"m\") end\nfun caller(): Int64 | Error do defer_on_error cleanup() return_on_error fail() end\nprint(0)",
        )
        .expect("defer_on_error should lower alongside return_on_error");
}

#[test]
fn deferred_by_value_arguments_move_only_at_scope_exit() {
    crate::emit_llvm_ir(
            "fun consume(text: String) do print(text) end\nfun caller() do let text: String = \"deferred\" defer consume(text) end\ncaller()",
        )
        .expect("a deferred by-value argument should remain available until exit");
}

#[test]
fn string_views_have_closed_element_types_and_builtin_lengths() {
    let program = assert_checks(
        "let text: String = \"café\" let bytes: View<Byte> = text.bytes() let count: Int64 = bytes.length()",
    );
    assert!(program.body.statements.len() >= 2);
    assert_error_contains(
        "let text: String = \"x\" let bad: View<Int64> = text.bytes()",
        "expected 'View<Int64>', found 'View<Byte>'",
    );
}

#[test]
fn collection_sequences_and_empty_map_set_constructors_check() {
    assert_checks(
        "let coordinates: Array<Int64, 3> = [10, 20, 30] let first: Int64 = coordinates[0] let size: Int64 = coordinates.length()",
    );
    assert_checks(
        "let numbers: List<Int64> = [1, 2, 3] let last: Int64 = numbers[2] let size: Int64 = numbers.length()",
    );
    assert_checks(
        "let scores: Map<String, Int64> = Map<String, Int64>() let seen: Set<Int64> = Set<Int64>()",
    );
}

#[test]
fn collection_literal_and_index_lower_to_valid_llvm() {
    crate::emit_llvm_ir(
        "let values: Array<Int64, 3> = [4, 5, 6] let item: Int64 = values[1] print(item)",
    )
    .expect("array literal and indexing should lower");
}

#[test]
fn map_and_set_operations_lower_through_private_runtime_descriptors() {
    crate::emit_llvm_ir(
        "let mut scores: Map<String, Int64> = Map<String, Int64>()
             let name: String = \"Alice\"
             scores.reserve(4)
             let added: Bool = scores.insert(name.clone(), 10)
             let found: Bool = scores.contains(name)
             let score: Int64 = scores[name]
             let removed: Bool = scores.delete(name)
             scores.clear()
             let mut seen: Set<Int64> = Set<Int64>()
             seen.reserve(4)
             let inserted: Bool = seen.insert(3)
             let present: Bool = seen.contains(3)
             let deleted: Bool = seen.delete(3)
             seen.clear()",
    )
    .expect("map and set operations should lower");
}

#[test]
fn integer_map_and_set_iteration_lower_through_private_runtime_descriptors() {
    crate::emit_llvm_ir(
        "let mut scores: Map<Int64, Int64> = Map<Int64, Int64>()
             scores.insert(1, 10)
             scores.insert(2, 20)
             for key, value in scores do print(key) print(value) end
             let mut seen: Set<Int64> = Set<Int64>()
             seen.insert(3)
             for member in seen do print(member) end",
    )
    .expect("integer map and set iteration should lower");
}

#[test]
fn scalar_map_and_set_key_types_lower_through_private_runtime_descriptors() {
    crate::emit_llvm_ir(
        "let mut widths: Map<UInt16, Int64> = Map<UInt16, Int64>()
             widths.insert(1u16, 16)
             let width: Int64 = widths[1u16]
             let mut flags: Set<Bool> = Set<Bool>()
             flags.insert(true)
             let present: Bool = flags.contains(true)",
    )
    .expect("scalar map and set key types should lower");
}

#[test]
fn string_map_and_set_iteration_uses_borrowed_loop_descriptors() {
    crate::emit_llvm_ir(
        "let mut bag: Map<String, Int64> = Map<String, Int64>()
             let name: String = \"Alice\"
             bag.insert(name.clone(), 10)
             for key, score in bag do print(key) print(score) end
             let mut words: Set<String> = Set<String>()
             words.insert(name.clone())
             for word in words do print(word) end",
    )
    .expect("String map and set iteration should lower through borrowed descriptors");
}

#[test]
fn mutable_scalar_lists_support_push_and_clear() {
    let program = assert_checks(
        "let mut values: List<Int64> = [1, 2]
             values.push(3)
             let size: Int64 = values.length()
             values.clear()
             print(size)",
    );
    assert!(matches!(program.body.statements[1], TStmt::ListPush { .. }));
    assert!(matches!(
        program.body.statements[3],
        TStmt::ListClear { .. }
    ));
    crate::emit_llvm_ir(
        "let mut values: List<Int64> = [1, 2]
             values.push(3)
             values.clear()",
    )
    .expect("scalar list mutation should lower");
}

#[test]
fn same_typed_sequences_and_views_support_equality() {
    crate::emit_llvm_ir(
        "let left: Array<Int64, 2> = [1, 2]
             let right: Array<Int64, 2> = [1, 2]
             let list_left: List<Int64> = [1, 2]
             let list_right: List<Int64> = [1, 2]
             let array_equal: Bool = left == right
             let list_equal: Bool = list_left == list_right
             let left_view: View<Int64> = left.view()
             let right_view: View<Int64> = right.view()
             let view_equal: Bool = left_view == right_view
             let text_left: List<String> = [\"a\"]
             let text_right: List<String> = [\"a\"]
             let text_equal: Bool = text_left == text_right
             print(array_equal)
             print(list_equal)
             print(view_equal)
             print(text_equal)",
    )
    .expect("same-typed sequence equality should lower");
    assert_error_contains(
        "let values: Map<Int64, Int64> = Map<Int64, Int64>() let other: Map<Int64, Int64> = Map<Int64, Int64>() print(values == other)",
        "does not support equality",
    );
}

#[test]
fn list_mutation_requires_mutability_and_storable_elements() {
    assert_error_contains(
        "let values: List<Int64> = [1] values.push(2)",
        "requires a mutable list receiver",
    );
    assert_checks("let mut values: List<Bool | Nil> = [true] values.push(true)");
}

#[test]
fn sequence_for_loop_checks_and_lowers() {
    assert_checks(
        "let values: Array<Int64, 3> = [1, 2, 3]
             for value in values do print(value) end",
    );
    crate::emit_llvm_ir(
        "let values: Array<Int64, 3> = [1, 2, 3]
             for value in values do print(value) end",
    )
    .expect("sequence for loop should lower");
}

#[test]
fn unicode_view_for_loop_uses_scalar_iteration() {
    crate::emit_llvm_ir(
        "let text: String = \"hé\"
             let scalars: View<Unicode> = text.unicode()
             for scalar in scalars do print(scalar) end",
    )
    .expect("Unicode view iteration should lower");
}

#[test]
fn string_view_lookup_and_multi_success_propagation_lower_to_valid_llvm() {
    crate::emit_llvm_ir(
        "fun read(): Int64 | String | Error do return 1 end
             fun caller(): Int64 | String | Error do return_on_error read() end
             let text: String = \"hé\"
             let bytes: View<Byte> = text.bytes()
             let byte: Byte | Nil = bytes.at(0)
             let scalars: View<Unicode> = text.unicode()
             let scalar: Unicode | Nil = scalars.scalar_at(1)
             print(text)",
    )
    .expect("view lookup and multi-success propagation should lower");
}

#[test]
fn view_slicing_and_checked_string_construction_lower_to_valid_llvm() {
    crate::emit_llvm_ir(
        "let text: String = \"héllo\"
             let bytes: View<Byte> = text.bytes()
             let part: View<Byte> | Nil = bytes.slice(0, 2)
             let scalars: View<Unicode> = text.unicode()
             let scalar_part: View<Unicode> | Nil = scalars.slice(1, 3)
             let copy: String = String.from_unicode(scalars)
             let checked: String | Nil = String.from_utf8(bytes)
             print(copy)",
    )
    .expect("view slicing and string construction should lower");
}

#[test]
fn string_concat_accepts_the_closed_scalar_part_set() {
    crate::emit_llvm_ir("let text: String = \"count: \".concat(3).concat(true).concat('!')")
        .expect("String.concat scalar parts should lower");
    assert_error_contains(
        "let text: String = \"x\".concat([1])",
        "String.concat does not accept",
    );
}

#[test]
fn interpolated_strings_use_the_normal_expression_checker_and_lowering() {
    let source = "let name: String = \"Ada\"\n\
                      let count: Int64 = 3\n\
                      let message: String = \"Hello, {{name}}. Count: {{count}}\"\n\
                      print(message)";
    assert_checks(source);
    crate::emit_llvm_ir(source).expect("interpolated strings should lower");
}

#[test]
fn interpolated_strings_reject_empty_and_unclosed_expressions() {
    for source in [
        "let message: String = \"bad {{}}\"",
        "let message: String = \"bad {{1\"",
        "let message: String = \"bad }}\"",
    ] {
        assert!(
            crate::parse(source).is_err(),
            "expected rejection: {source}"
        );
    }
}

#[test]
fn string_views_end_at_their_last_local_use_before_reassignment() {
    assert_checks(
        "let mut text: String = \"hello\"\n\
             let bytes: View<Byte> = text.bytes()\n\
             print(bytes.length())\n\
             text = \"goodbye\"",
    );
    assert_error_contains(
        "let mut text: String = \"hello\"\n\
             let bytes: View<Byte> = text.bytes()\n\
             text = \"goodbye\"\n\
             print(bytes.length())",
        "still borrows it",
    );
}

#[test]
fn strings_lend_the_expected_view_type_to_calls_but_views_do_not_escape() {
    assert_checks(
        "fun count(bytes: View<Byte>): Int64 do bytes.length() end\n\
             let text: String = \"hello\"\n\
             let result: Int64 = count(text)",
    );
    assert_error_contains(
        "fun leak(text: String): View<Byte> do text.bytes() end",
        "cannot be returned",
    );
    assert_error_contains(
        "let text: String = \"hello\"\nlet boxed: Box<View<Byte>> = box(text.bytes())",
        "cannot be stored in a Box",
    );
}

// Specification 012 sections 5-6: declarations and root mutability.

#[test]
fn rejects_a_duplicate_local_declaration() {
    assert_error_contains(
        "let x: Int64 = 10\nlet x: Int64 = 20",
        "Variable 'x' already exists",
    );
}

#[test]
fn rejects_a_duplicate_local_declared_in_a_nested_branch() {
    assert_error_contains(
        "fun f(ready: Bool) do let value: Int64 = 1 if ready then let value: Int64 = 2 print(value) end end",
        "Variable 'value' already exists",
    );
}

#[test]
fn rejects_a_local_that_shadows_a_parameter() {
    assert_error_contains(
        "fun f(value: Int64): Int64 do let value: Int64 = 1 value end",
        "Variable 'value' already exists",
    );
}

#[test]
fn accepts_the_same_local_name_in_different_functions() {
    assert_checks(
        "fun f(): Int64 do let value: Int64 = 1 value end\n\
             fun g(): Int64 do let value: Int64 = 2 value end",
    );
}

#[test]
fn an_initializer_cannot_refer_to_the_variable_being_declared() {
    assert_error_contains("let count: Int64 = count + 1", "No such variable 'count'");
}

#[test]
fn rejects_assignment_to_an_immutable_root() {
    assert_error_contains(
        "let count: Int64 = 1\ncount = 2",
        "'count' is not declared 'mut' and cannot be assigned",
    );
}

#[test]
fn accepts_assignment_to_a_mutable_root() {
    let program = assert_checks("let mut count: Int64 = 1\ncount = count + 1\nprint(count)");
    assert!(matches!(
        program.body.statements[0],
        TStmt::Let { mutable: true, .. }
    ));
    assert!(matches!(program.body.statements[1], TStmt::Assign { .. }));
}

#[test]
fn rejects_an_assignment_type_mismatch() {
    assert_error_contains(
        "let mut count: Int64 = 1\ncount = true",
        "expected 'Int64', found 'Bool'",
    );
}

#[test]
fn rejects_assignment_to_an_undeclared_name() {
    assert_error_contains("missing = 1", "No such variable 'missing' in scope");
}

#[test]
fn a_declaration_does_not_escape_its_block() {
    assert_error_contains(
        "if true then let inner: Int64 = 1 print(inner) end\nprint(inner)",
        "No such variable 'inner' in scope",
    );
}

#[test]
fn an_empty_value_required_body_is_rejected() {
    assert_error_contains("fun f(): Int64 do end", "but it is empty");
}

#[test]
fn a_no_result_function_body_may_be_empty() {
    assert_checks("fun nothing() do end");
}

// Specification 009 sections 4.4-4.7: exact-match types.

const NEW_TYPES: [(&str, &str); 5] = [
    ("Byte", "1u8"),
    ("UInt16", "1u16"),
    ("UInt32", "1u32"),
    ("UInt64", "1u64"),
    ("Float32", "1.5f32"),
];

#[test]
fn accepts_every_new_type_in_every_declaration_position() {
    for (name, literal) in NEW_TYPES {
        let program = assert_checks(&format!(
            "extern rust \"snacc_user_edge\" fun edge(value: {name}): {name}\n\
                 fun identity(value: {name}): {name} do value end\n\
                 let bound: {name} = {literal}\n\
                 print(identity(bound))"
        ));
        let expected = program.funcs["identity"].result;
        assert!(expected.is_some());
        assert_eq!(program.funcs["identity"].params[0].ty, expected.unwrap());
        assert_eq!(program.externs["edge"].result, expected);
    }
}

#[test]
fn rejects_every_implicit_conversion_the_new_types_prohibit() {
    for (source, needle) in [
        ("let byte: Byte = 1", "expected 'Byte', found 'Int64'"),
        ("let byte: Byte = 1u16", "expected 'Byte', found 'UInt16'"),
        (
            "let wide: UInt64 = 1u32",
            "expected 'UInt64', found 'UInt32'",
        ),
        (
            "let count: Int64 = 1u64",
            "expected 'Int64', found 'UInt64'",
        ),
        (
            "let ratio: Float32 = 1u8",
            "expected 'Float32', found 'Byte'",
        ),
        (
            "let ratio: Float32 = 1.5",
            "expected 'Float32', found 'Float64'",
        ),
        (
            "let wide: Float64 = 1.5f32",
            "expected 'Float64', found 'Float32'",
        ),
        (
            "let wide: Float64 = 1u8",
            "expected 'Float64', found 'Byte'",
        ),
    ] {
        assert_error_contains(source, needle);
    }
}

#[test]
fn the_int64_to_float64_conversion_still_works() {
    assert_checks("let wide: Float64 = 1\nprint(wide + 1)");
}

#[test]
fn accepts_same_type_arithmetic_and_comparison_for_every_new_type() {
    for (name, literal) in NEW_TYPES {
        for operator in ["+", "-", "*", "/"] {
            assert_checks(&format!(
                "let result: {name} = {literal} {operator} {literal}"
            ));
        }
        for operator in ["<", "<=", ">", ">=", "==", "!="] {
            assert_checks(&format!("let flag: Bool = {literal} {operator} {literal}"));
        }
    }
}

#[test]
fn rejects_mixed_operands_in_every_category() {
    let others = ["1", "1.5", "1u8", "1u16", "1u32", "1u64", "1.5f32", "true"];
    for (_, literal) in NEW_TYPES {
        for other in others {
            if other == literal {
                continue;
            }
            assert_error_contains(
                &format!("print({literal} + {other})"),
                "operands must be two numbers of the same type",
            );
            assert_error_contains(
                &format!("print({literal} < {other})"),
                "operands must be two numbers of the same type",
            );
            assert_error_contains(&format!("print({literal} == {other})"), "expected");
        }
    }
}

#[test]
fn a_mixed_operand_pair_reports_one_diagnostic() {
    assert_eq!(errors("print(1u8 + 1)").len(), 1);
    assert_eq!(errors("print(1u8 < 1u16)").len(), 1);
    assert_eq!(errors("print(1u8 == 1)").len(), 1);
}

#[test]
fn statically_known_nan_is_rejected_for_both_float_widths() {
    assert_error_contains("print(0.0 / 0.0)", "floating-point operation produces NaN");
    assert_error_contains(
        "print(0f32 / 0f32)",
        "floating-point operation produces NaN",
    );
    assert_error_contains(
        "print(1.0 / 0.0 - 1.0 / 0.0)",
        "floating-point operation produces NaN",
    );
}

#[test]
fn infinity_remains_an_admitted_non_nan_float_result() {
    assert_checks("print(1.0 / 0.0)");
    assert_checks("print(1f32 / 0f32)");
}

#[test]
fn arithmetic_and_comparison_keep_the_exact_operand_type() {
    for (name, literal) in NEW_TYPES {
        let program = assert_checks(&format!(
            "fun combine(): Bool do {literal} + {literal} < {literal} end\n\
                 let bound: {name} = {literal}"
        ));
        let TStmt::Let { ty, .. } = &program.body.statements[0] else {
            panic!("expected a let statement");
        };
        let result = program.funcs["combine"]
            .body
            .result
            .as_ref()
            .expect("combine produces a value");
        let TExpr::Cmp(left, _, _, operand_ty) = result else {
            panic!("expected a comparison");
        };
        assert_eq!(operand_ty, ty, "{name} comparison lost its operand type");
        let TExpr::Arith(_, _, _, arith_ty) = left.as_ref() else {
            panic!("expected arithmetic");
        };
        assert_eq!(arith_ty, ty, "{name} arithmetic lost its operand type");
    }
}

#[test]
fn print_accepts_every_new_type_and_returns_it() {
    for (name, literal) in NEW_TYPES {
        assert_checks(&format!("let echoed: {name} = print({literal})"));
    }
}

// ---------------------------------------------------------------------
// Specification 010: nominal types, structs, unions, and methods.
// ---------------------------------------------------------------------

const POINT: &str = "type Point is struct x: Float64, y: Float64, end\n";
const SHAPE: &str = "type Shape is union\n\
         | Circle is struct radius: Int64, end\n\
         | Rectangle is struct length: Int64, width: Int64, end\n\
         end\n";
const DIRECTION: &str = "type Direction is union | East | West end\n";

/// Conformance 1: represented types are nominal and wrap/unwrap one layer.
#[test]
fn a_represented_type_wraps_and_unwraps_exactly_one_layer() {
    assert_checks(
        "type UserId is Int64\n\
             let id: UserId = UserId(42)\n\
             let number: Int64 = Int64(id)\n\
             print(number)",
    );
}

#[test]
fn a_represented_type_is_not_its_representation() {
    assert_error_contains(
        "type UserId is Int64\nlet id: UserId = 42",
        "expected 'UserId', found 'Int64'",
    );
    assert_error_contains(
        "type UserId is Int64\nlet id: UserId = UserId(42)\nlet n: Int64 = id",
        "expected 'Int64', found 'UserId'",
    );
    assert_error_contains(
        "type UserId is Int64\nlet id: UserId = UserId(42)\nprint(id + id)",
        "operands must be two numbers of the same type",
    );
}

#[test]
fn represented_conversion_names_the_required_immediate_type() {
    assert_error_contains(
        "type UserId is Int64\nlet id: UserId = UserId(1.5)",
        "wraps exactly its immediate representation 'Int64', found 'Float64'",
    );
    assert_error_contains(
        "type UserId is Int64\nprint(Int64(7))",
        "unwraps exactly one value of a type represented by 'Int64', found 'Int64'",
    );
    assert_error_contains(
        "type UserId is Int64\nlet id: UserId = UserId(1, 2)",
        "converts exactly one positional value",
    );
    assert_error_contains(
        "type UserId is Int64\nlet id: UserId = UserId(value: 1)",
        "named arguments are only used to construct a struct",
    );
}

/// Conformance 1: unwrapping skips no layer.
#[test]
fn represented_conversion_does_not_skip_a_layer() {
    assert_error_contains(
        "type Inner is Int64\ntype Outer is Inner\n\
             let value: Outer = Outer(Inner(1))\n\
             let flat: Int64 = Int64(value)",
        "unwraps exactly one value of a type represented by 'Int64', found 'Outer'",
    );
    assert_checks(
        "type Inner is Int64\ntype Outer is Inner\n\
             let value: Outer = Outer(Inner(1))\n\
             let one: Inner = Inner(value)\n\
             let flat: Int64 = Int64(one)\n\
             print(flat)",
    );
}

/// Conformance 2: same-representation nominal types stay distinct.
#[test]
fn same_representation_nominal_types_are_not_assignable_or_comparable() {
    assert_error_contains(
        "type UserId is Int64\ntype OrderId is Int64\n\
             let id: UserId = UserId(1)\nlet other: OrderId = id",
        "expected 'OrderId', found 'UserId'",
    );
    assert_error_contains(
        "type UserId is Int64\ntype OrderId is Int64\n\
             let id: UserId = UserId(1)\nlet other: OrderId = OrderId(1)\n\
             print(id == other)",
        "expected 'UserId', found 'OrderId'",
    );
}

/// Conformance 7: represented equality compares represented values.
#[test]
fn represented_values_compare_with_their_own_type_only() {
    assert_checks(
        "type UserId is Int64\n\
             let a: UserId = UserId(1)\nlet b: UserId = UserId(2)\nprint(a == b)",
    );
    assert_error_contains(
        "type UserId is Int64\nlet a: UserId = UserId(1)\nprint(a == 1)",
        "expected 'UserId', found 'Int64'",
    );
}

/// Conformance 3: reordered named fields and a trailing comma, checked and
/// stored so evaluation stays in written order.
#[test]
fn struct_construction_accepts_reordered_named_fields() {
    let program = assert_checks(&format!(
        "{POINT}let point: Point = Point(y: 4.0, x: 3.0,)\nprint(point.x)"
    ));
    let TStmt::Let { value, .. } = &program.body.statements[0] else {
        panic!("expected a declaration");
    };
    let TExpr::Construct { fields, .. } = value else {
        panic!("expected a constructor");
    };
    assert_eq!(
        fields.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        vec![1, 0],
        "constructor entries keep written evaluation order with declaration indices"
    );
}

/// Conformance 4: missing, duplicate, unknown, and positional fields.
#[test]
fn rejects_malformed_struct_construction() {
    for (source, needle) in [
        ("let p: Point = Point(x: 1.0)", "is missing field 'y'"),
        (
            "let p: Point = Point(x: 1.0, x: 2.0, y: 3.0)",
            "Field 'Point.x' is supplied more than once",
        ),
        (
            "let p: Point = Point(x: 1.0, y: 2.0, z: 3.0)",
            "'Point' has no field 'z'",
        ),
        (
            "let p: Point = Point(1.0, 2.0)",
            "positional construction of a non-empty struct is invalid",
        ),
    ] {
        assert_error_contains(&format!("{POINT}{source}"), needle);
    }
}

/// Conformance 5: empty top-level and union-member structs.
#[test]
fn empty_structs_construct_with_parentheses_and_stay_nominal() {
    assert_checks(
        "type Marker is struct end\ntype Other is struct end\n\
             let marker: Marker = Marker()\nlet second: Marker = Marker()\n\
             print(marker == second)",
    );
    assert_error_contains(
        "type Marker is struct end\ntype Other is struct end\n\
             let marker: Marker = Marker()\nlet other: Other = Other()\n\
             print(marker == other)",
        "expected 'Marker', found 'Other'",
    );
    assert_error_contains(
        "type Marker is struct end\nlet marker: Marker = Marker(x: 1)",
        "has no fields, so it is constructed with '()'",
    );
    assert_error_contains(
        "type Marker is struct end\nlet marker: Marker = Marker",
        "a type name alone is never a value",
    );
}

/// Conformance 6: member names live only in their union's namespace.
#[test]
fn union_member_names_resolve_only_through_their_union() {
    assert_checks(&format!(
        "{SHAPE}let shape: Shape = Shape.Circle(radius: 10)\nprint(1)"
    ));
    assert_error_contains(
        &format!("{SHAPE}let c: Circle = 1"),
        "Unknown type 'Circle'",
    );
    assert_error_contains(
        &format!("{SHAPE}print(Circle(radius: 1))"),
        "'Circle' is not callable",
    );
    assert_error_contains(
        &format!("{SHAPE}let s: Shape = Shape.Triangle()"),
        "'Shape' has no member type 'Triangle'",
    );
}

/// Conformance 8: a union takes each direct member and nothing else.
#[test]
fn a_union_accepts_its_direct_members_and_rejects_others() {
    assert_checks(&format!(
        "{SHAPE}{DIRECTION}\
             let a: Shape = Shape.Circle(radius: 1)\n\
             let b: Shape = Shape.Rectangle(length: 1, width: 2)\n\
             let c: Direction = Direction.East()\nprint(1)"
    ));
    assert_error_contains(
        &format!("{SHAPE}{DIRECTION}let bad: Shape = Direction.East()"),
        "expected 'Shape', found 'Direction.East'",
    );
    assert_error_contains(
        &format!("{SHAPE}let bad: Shape = Shape(1)"),
        "construction names one member type, not the union itself",
    );
}

/// Conformance 7: struct and union equality.
#[test]
fn struct_and_union_equality_follow_nominal_identity() {
    assert_checks(&format!(
        "{POINT}let a: Point = Point(x: 1.0, y: 2.0)\n\
             let b: Point = Point(x: 1.0, y: 2.0)\nprint(a == b)"
    ));
    assert_checks(&format!(
        "{SHAPE}let a: Shape = Shape.Circle(radius: 1)\n\
             let b: Shape = Shape.Rectangle(length: 1, width: 2)\nprint(a != b)"
    ));
    // A union never compares directly with one of its member types.
    assert_error_contains(
        &format!(
            "{SHAPE}let a: Shape = Shape.Circle(radius: 1)\n\
                 let b: Shape.Circle = Shape.Circle(radius: 1)\nprint(a == b)"
        ),
        "expected 'Shape', found 'Shape.Circle'",
    );
    assert_error_contains(
        &format!("{POINT}let a: Point = Point(x: 1.0, y: 2.0)\nprint(a < a)"),
        "operands must be two numbers of the same type",
    );
}

#[test]
fn union_tags_follow_source_order_from_zero() {
    let program = assert_checks(&format!("{SHAPE}print(1)"));
    let tags: Vec<(String, u32)> = program
        .types
        .iter()
        .filter_map(|def| match def {
            TypeDef::UnionMember { name, tag, .. } => Some((name.clone(), *tag)),
            _ => None,
        })
        .collect();
    assert_eq!(
        tags,
        vec![("Shape.Circle".into(), 0), ("Shape.Rectangle".into(), 1)]
    );
}

/// Conformance 10: methods read `self` and work on values and temporaries.
#[test]
fn methods_read_self_and_return_values() {
    let program = assert_checks(&format!(
        "{POINT}\
             method Point.sum(): Float64 do self.x + self.y end\n\
             method Point.scaled(factor: Float64): Point do \
                Point(x: self.x * factor, y: self.y * factor) end\n\
             let point: Point = Point(x: 3.0, y: 4.0)\n\
             print(point.sum())\n\
             print(Point(x: 1.0, y: 2.0).sum())\n\
             print(point.scaled(2.0).sum())"
    ));
    assert_eq!(program.methods.len(), 2);
    assert!(program.methods.iter().all(|m| !m.writes_receiver));
}

/// Conformance 13: lookup is exact, namespaced, and non-overloaded.
#[test]
fn method_lookup_is_exact_and_namespaced_by_receiver_type() {
    assert_checks(&format!(
        "{POINT}type Other is struct x: Float64, end\n\
             method Point.width(): Float64 do self.x end\n\
             method Other.width(): Float64 do self.x end\n\
             let p: Point = Point(x: 1.0, y: 2.0)\n\
             let o: Other = Other(x: 1.0)\n\
             print(p.width())\nprint(o.width())"
    ));
    assert_error_contains(
        &format!(
            "{POINT}method Point.a(): Float64 do 1.0 end\nmethod Point.a(): Float64 do 2.0 end"
        ),
        "Method 'Point.a' already exists; methods are not overloaded",
    );
    assert_error_contains(
        &format!("{POINT}let p: Point = Point(x: 1.0, y: 2.0)\nprint(p.missing())"),
        "'Point' has no method 'missing'",
    );
    assert_error_contains(
        &format!(
            "{POINT}method Point.sum(): Float64 do self.x end\n\
                 let p: Point = Point(x: 1.0, y: 2.0)\nprint(p.sum)"
        ),
        "'Point.sum' is a method; a method requires a receiver call",
    );
    assert_error_contains(
        &format!("{POINT}let p: Point = Point(x: 1.0, y: 2.0)\nprint(p.x())"),
        "'Point.x' is a field, not a method, so it cannot be called",
    );
    assert_error_contains(
        "method Missing.thing(): Int64 do 1 end",
        "Unknown type 'Missing'",
    );
}

/// Conformance 12: `self` is method-only.
#[test]
fn self_is_rejected_outside_a_method() {
    assert_error_contains("print(self)", "'self' is only valid inside a method body");
    assert_error_contains(
        "fun f(): Int64 do self end",
        "'self' is only valid inside a method body",
    );
    assert_error_contains("self = 1", "'self' is only valid inside a method body");
    // `self` is a keyword, so it cannot be spelled as a binding at all.
    assert_rejected_by_parser("let self: Int64 = 1");
    assert_rejected_by_parser("fun f(self: Int64) do end");
}

/// Conformance 11 and 15, with Specification 012 sections 7 and 9.
#[test]
fn field_and_receiver_assignment_follow_root_mutability_only() {
    assert_checks(&format!(
        "{POINT}let mut point: Point = Point(x: 3.0, y: 4.0)\n\
             point.x = 5.0\npoint = Point(x: 0.0, y: 0.0)"
    ));
    assert_error_contains(
        &format!("{POINT}let point: Point = Point(x: 3.0, y: 4.0)\npoint.x = 5.0"),
        "'point' is not declared 'mut' and cannot be assigned",
    );
    assert_error_contains(
        &format!("{POINT}let point: Point = Point(x: 3.0, y: 4.0)\npoint = Point(x: 0.0, y: 0.0)"),
        "'point' is not declared 'mut' and cannot be assigned",
    );
    // Ordinary parameters and their fields are immutable roots.
    assert_error_contains(
        &format!("{POINT}fun shift(point: Point) do point.x = 1.0 end"),
        "'point' is not declared 'mut' and cannot be assigned",
    );
    // Root mutability applies through the complete field path, and never
    // consults the struct definition.
    assert_checks(&format!(
        "{POINT}type Entity is struct position: Point, end\n\
             let mut entity: Entity = Entity(position: Point(x: 1.0, y: 2.0))\n\
             entity.position.x = 1.0"
    ));
    assert_error_contains(
        &format!(
            "{POINT}type Entity is struct position: Point, end\n\
                 let entity: Entity = Entity(position: Point(x: 1.0, y: 2.0))\n\
                 entity.position.x = 1.0"
        ),
        "'entity' is not declared 'mut' and cannot be assigned",
    );
    assert_error_contains(
        &format!("{POINT}let mut point: Point = Point(x: 1.0, y: 2.0)\npoint.z = 1.0"),
        "'Point' has no field 'z'",
    );
}

/// Conformance 11: receiver-writing calls need a mutable root; whole-`self`
/// replacement is one of them, and no method-level marker exists.
#[test]
fn receiver_writing_calls_require_a_mutable_receiver_root() {
    let program = assert_checks(&format!(
        "{POINT}\
             method Point.translate(dx: Float64, dy: Float64) do \
                self.x = self.x + dx self.y = self.y + dy end\n\
             method Point.reset() do self = Point(x: 0.0, y: 0.0) end\n\
             method Point.sum(): Float64 do self.x + self.y end\n\
             let mut point: Point = Point(x: 3.0, y: 4.0)\n\
             point.translate(1.0, 2.0)\npoint.reset()\nprint(point.sum())"
    ));
    let effects: Vec<(String, bool)> = program
        .methods
        .iter()
        .map(|m| (m.name.clone(), m.writes_receiver))
        .collect();
    assert_eq!(
        effects,
        vec![
            ("translate".into(), true),
            ("reset".into(), true),
            ("sum".into(), false)
        ]
    );
    for call in ["point.translate(1.0, 2.0)", "point.reset()"] {
        assert_error_contains(
            &format!(
                "{POINT}\
                     method Point.translate(dx: Float64, dy: Float64) do self.x = self.x + dx end\n\
                     method Point.reset() do self = Point(x: 0.0, y: 0.0) end\n\
                     let point: Point = Point(x: 3.0, y: 4.0)\n{call}"
            ),
            "requires a mutable root, but 'point' is not mutable",
        );
    }
    // A read-only method is callable on a temporary; a writing one is not.
    assert_error_contains(
        &format!(
            "{POINT}method Point.reset() do self = Point(x: 0.0, y: 0.0) end\n\
                 Point(x: 1.0, y: 2.0).reset()"
        ),
        "requires a mutable root, but 'a temporary' is not mutable",
    );
}

/// Conformance 27: the effect reaches its least fixed point through direct,
/// transitive, recursive, and mutually recursive calls, and marks nothing
/// for unrelated local writes.
#[test]
fn receiver_write_effects_reach_a_least_fixed_point() {
    let source = format!(
        "{POINT}\
             method Point.bump() do self.x = self.x + 1.0 end\n\
             method Point.outer() do self.middle() end\n\
             method Point.middle() do self.bump() end\n\
             method Point.ping() do self.pong() end\n\
             method Point.pong() do self.ping() end\n\
             method Point.local_only() do let mut n: Float64 = 1.0 n = n + 1.0 end\n\
             method Point.selfish() do self.selfish() end\n\
             let mut point: Point = Point(x: 1.0, y: 2.0)\npoint.outer()"
    );
    let program = assert_checks(&source);
    let effects: Vec<(String, bool)> = program
        .methods
        .iter()
        .map(|m| (m.name.clone(), m.writes_receiver))
        .collect();
    assert_eq!(
        effects,
        vec![
            ("bump".into(), true),
            // Transitive through one intermediate method.
            ("outer".into(), true),
            ("middle".into(), true),
            // A mutually recursive pair that never writes stays unmarked.
            ("ping".into(), false),
            ("pong".into(), false),
            // An unrelated local write is not a receiver write.
            ("local_only".into(), false),
            ("selfish".into(), false),
        ]
    );
    // The transitive fact is what rejects the call, not the direct one.
    assert_error_contains(
        &source.replace("let mut point", "let point"),
        "'Point.outer' may assign through 'self', so its receiver requires a mutable root",
    );
    // Calling on a field of a mutable root is allowed.
    assert_checks(&format!(
        "{POINT}type Entity is struct position: Point, end\n\
             method Point.bump() do self.x = self.x + 1.0 end\n\
             let mut entity: Entity = Entity(position: Point(x: 1.0, y: 2.0))\n\
             entity.position.bump()"
    ));
}

/// A type-test binding is an immutable root (Specification 012 section 7).
#[test]
fn a_type_test_binding_is_an_immutable_root() {
    assert_error_contains(
        &format!(
            "{SHAPE}let shape: Shape = Shape.Circle(radius: 1)\n\
                 if shape is Shape.Circle(circle) then circle.radius = 2 end"
        ),
        "'circle' is not declared 'mut' and cannot be assigned",
    );
}

/// Conformance 16: a no-result method call is a statement only.
#[test]
fn no_result_method_calls_are_rejected_in_value_positions() {
    assert_checks(&format!(
        "{POINT}method Point.noop() do print(1) end\n\
             let mut p: Point = Point(x: 1.0, y: 2.0)\np.noop()"
    ));
    assert_error_contains(
        &format!(
            "{POINT}method Point.noop() do print(1) end\n\
                 let mut p: Point = Point(x: 1.0, y: 2.0)\nlet n: Int64 = p.noop()"
        ),
        "'Point.noop' declares no result, so its call cannot be used as a value",
    );
}

/// Conformance 26: call-head conflicts and section 6.1 resolution.
#[test]
fn a_type_name_may_not_share_a_call_head_with_a_callable() {
    assert_error_contains(
        "type Point is struct x: Int64, end\nfun Point(): Int64 do 1 end",
        "shares a call head with the function or Rust bridge of the same name",
    );
    assert_error_contains(
        "type Point is struct x: Int64, end\n\
             extern rust \"snacc_user_point\" fun Point(): Int64",
        "shares a call head with the function or Rust bridge of the same name",
    );
    assert_error_contains(
        "type Point is struct x: Int64, end\ntype Point is Int64",
        "Type 'Point' already exists",
    );
}

/// Conformance 26: an in-scope binding wins a qualified call head, and a
/// bare `name(...)` never calls a local.
#[test]
fn a_binding_wins_a_qualified_call_head_over_a_type_path() {
    // `Case` is both a union type and a parameter; the parameter wins.
    // (Specification 016 reserves `Box`, so this uses an unreserved name
    // that still exercises the same type-path-versus-binding shadowing.)
    assert_checks(
        "type Case is union | Item is struct n: Int64, end end\n\
             method Case.Item.get(): Int64 do self.n end\n\
             fun read(Case: Case.Item): Int64 do Case.get() end\n\
             print(read(Case.Item(n: 1)))",
    );
    // Without a binding of that name the same path is a constructor.
    assert_checks(
        "type Case is union | Item is struct n: Int64, end end\n\
             let held: Case = Case.Item(n: 1)\nprint(1)",
    );
    assert_error_contains(
        "fun f(value: Int64): Int64 do value(1) end",
        "'value' is a variable; Snacc has no function values, so it cannot be called",
    );
    assert_error_contains("print(missing(1))", "'missing' is not callable");
    // A bare call head skips the local namespace entirely, so a local that
    // shares a callable's name does not hide it.
    assert_checks(
        "fun double(value: Int64): Int64 do value * 2 end\n\
             fun use(double: Int64): Int64 do double(double) end\n\
             print(use(3))",
    );
    // The same holds for a type constructor sharing a local's name.
    assert_checks(
        "type Wrapper is struct n: Int64, end\n\
             fun build(Wrapper: Int64): Int64 do Wrapper(n: Wrapper).n end\n\
             print(build(3))",
    );
}

/// Conformance 17 and 18: `is` produces `Bool`, and its binding has the
/// exact member type only inside the successful branch.
#[test]
fn type_tests_narrow_only_inside_their_own_branch() {
    assert_checks(&format!(
        "{SHAPE}let shape: Shape = Shape.Circle(radius: 10)\n\
             if shape is Shape.Circle(circle) then print(circle.radius) \
             elseif shape is Shape.Rectangle(rectangle) then \
             print(rectangle.length * rectangle.width) end"
    ));
    assert_error_contains(
        &format!(
            "{SHAPE}let shape: Shape = Shape.Circle(radius: 10)\n\
                 if shape is Shape.Circle(circle) then print(1) else print(circle.radius) end"
        ),
        "No such variable 'circle' in scope",
    );
    assert_error_contains(
        &format!(
            "{SHAPE}let shape: Shape = Shape.Circle(radius: 10)\n\
                 if shape is Shape.Circle(circle) then print(1) end\nprint(circle.radius)"
        ),
        "No such variable 'circle' in scope",
    );
    // Specification 012 section 5.2: the binding name is function-wide.
    assert_error_contains(
        &format!(
            "{SHAPE}let circle: Int64 = 1\nlet shape: Shape = Shape.Circle(radius: 10)\n\
                 if shape is Shape.Circle(circle) then print(1) end"
        ),
        "Binding 'circle' already exists",
    );
    assert_error_contains(
        &format!(
            "{SHAPE}let shape: Shape = Shape.Circle(radius: 10)\n\
                 if shape is Shape.Circle(c) then print(1) elseif shape is Shape.Rectangle(c) \
                 then print(2) end"
        ),
        "Binding 'c' already exists",
    );
}

#[test]
fn a_bare_type_test_supplies_a_bool_condition() {
    let program = assert_checks(&format!(
        "{DIRECTION}let d: Direction = Direction.East()\n\
             if d is Direction.East then print(1) elseif d is Direction.West then print(2) end"
    ));
    let TStmt::If(form) = &program.body.statements[1] else {
        panic!("expected an if statement");
    };
    let tags: Vec<u32> = form
        .arms
        .iter()
        .map(|(condition, _)| match condition {
            TCondition::Test(test) => test.tag,
            TCondition::Expr(_) | TCondition::SumTest(_) => panic!("expected a union test"),
        })
        .collect();
    assert_eq!(tags, vec![0, 1]);
    assert!(form.exhaustive);
}

/// Conformance 19: unrelated, always-true, and non-place tests.
#[test]
fn invalid_type_tests_are_rejected() {
    assert_error_contains(
        &format!(
            "{SHAPE}{DIRECTION}let shape: Shape = Shape.Circle(radius: 1)\n\
                 if shape is Direction.East then print(1) end"
        ),
        "'Direction.East' is not a direct member of 'Shape'",
    );
    assert_error_contains(
        &format!(
            "{SHAPE}let shape: Shape = Shape.Circle(radius: 1)\n\
                 if shape is Shape then print(1) end"
        ),
        "already has type 'Shape', so this test is always true",
    );
    assert_error_contains(
        &format!("{POINT}let p: Point = Point(x: 1.0, y: 2.0)\nif p is Point then print(1) end"),
        "the left side of 'is' must have a union type",
    );
    assert_error_contains(
        &format!(
            "{SHAPE}let c: Shape.Circle = Shape.Circle(radius: 1)\n\
                 if c is Shape.Circle then print(1) end"
        ),
        "the left side of 'is' must have a union type",
    );
    // A call result is not a place, so it cannot be the subject of `is`.
    assert_rejected_by_parser(&format!(
        "{SHAPE}fun make(): Shape do Shape.Circle(radius: 1) end\n\
             if make() is Shape.Circle then print(1) end"
    ));
}

/// Conformance 20: exhaustive chains produce values without `else`, for
/// both empty and data-carrying members.
#[test]
fn an_exhaustive_chain_produces_a_value_without_an_else() {
    let program = assert_checks(&format!(
        "{DIRECTION}\
             fun pick(d: Direction): Int64 do \
                if d is Direction.East then 1 elseif d is Direction.West then 2 end end\n\
             print(pick(Direction.East()))"
    ));
    let result = program.funcs["pick"]
        .body
        .result
        .as_ref()
        .expect("pick produces a value");
    let TExpr::If(form) = result else {
        panic!("expected a value-form if");
    };
    assert!(form.exhaustive);
    assert!(form.else_branch.is_none());

    assert_checks(&format!(
        "{SHAPE}\
             fun area(shape: Shape): Int64 do \
                if shape is Shape.Circle(circle) then circle.radius * circle.radius \
                elseif shape is Shape.Rectangle(rectangle) then \
                rectangle.length * rectangle.width end end\n\
             print(area(Shape.Circle(radius: 2)))"
    ));
}

/// Conformance 21: missing members, duplicates, mixed places, and an
/// unreachable `else`.
#[test]
fn non_exhaustive_and_unreachable_chains_are_rejected() {
    let three = "type Light is union | Red | Amber | Green end\n";
    assert_error_contains(
        &format!(
            "{three}fun pick(l: Light): Int64 do \
                 if l is Light.Red then 1 elseif l is Light.Amber then 2 end end"
        ),
        "does not handle Light.Green",
    );
    assert_error_contains(
        &format!(
            "{DIRECTION}fun pick(d: Direction): Int64 do \
                 if d is Direction.East then 1 elseif d is Direction.East then 2 \
                 elseif d is Direction.West then 3 end end"
        ),
        "'Direction.East' is already handled by an earlier branch",
    );
    // Two different places never form one chain, so `else` stays required.
    assert_error_contains(
        &format!(
            "{DIRECTION}fun pick(a: Direction, b: Direction): Int64 do \
                 if a is Direction.East then 1 elseif b is Direction.West then 2 end end"
        ),
        "requires an 'else' branch",
    );
    // An ordinary condition mixed into the chain also requires `else`.
    assert_error_contains(
        &format!(
            "{DIRECTION}fun pick(d: Direction, flag: Bool): Int64 do \
                 if d is Direction.East then 1 elseif flag then 2 end end"
        ),
        "requires an 'else' branch",
    );
    // A covered chain rejects `else` in both statement and value form.
    for source in [
        format!(
            "{DIRECTION}fun pick(d: Direction): Int64 do \
                 if d is Direction.East then 1 elseif d is Direction.West then 2 else 3 end end"
        ),
        format!(
            "{DIRECTION}let d: Direction = Direction.East()\n\
                 if d is Direction.East then print(1) elseif d is Direction.West then print(2) \
                 else print(3) end"
        ),
    ] {
        assert_error_contains(&source, "so the 'else' branch is unreachable");
    }
}

/// Section 12.3: the tested place may be a field path rooted at a local,
/// and exhaustiveness compares the complete syntactic place.
#[test]
fn an_exhaustive_chain_may_test_a_field_path() {
    let types =
        format!("{DIRECTION}type Entity is struct heading: Direction, spare: Direction, end\n");
    assert_checks(&format!(
        "{types}fun rank(entity: Entity): Int64 do \
             if entity.heading is Direction.East then 1 \
             elseif entity.heading is Direction.West then 2 end end"
    ));
    // Two different field paths under one root are two different places.
    assert_error_contains(
        &format!(
            "{types}fun rank(entity: Entity): Int64 do \
                 if entity.heading is Direction.East then 1 \
                 elseif entity.spare is Direction.West then 2 end end"
        ),
        "requires an 'else' branch",
    );
}

/// Section 11.2 and conformance 14: the receiver evaluates once, before the
/// explicit arguments, and arguments keep their written order.
#[test]
fn a_method_call_keeps_its_receiver_and_arguments_in_order() {
    let program = assert_checks(&format!(
        "{POINT}method Point.combine(a: Float64, b: Float64): Float64 do self.x + a + b end\n\
             let p: Point = Point(x: 1.0, y: 2.0)\nprint(p.combine(3.0, 4.0))"
    ));
    let TStmt::Expr(TExpr::Print(value, _)) = &program.body.statements[1] else {
        panic!("expected a print statement");
    };
    let TExpr::MethodCall(call) = value.as_ref() else {
        panic!("expected a method call");
    };
    assert!(
        matches!(&call.receiver, TReceiver::Place(place) if place.root == PlaceRoot::Local("p".into())),
        "a place receiver keeps its addressable storage"
    );
    assert_eq!(call.args.len(), 2);
}

/// Conformance 22: adding a member breaks a formerly exhaustive chain.
#[test]
fn adding_a_union_member_breaks_an_exhaustive_chain() {
    let chain = "fun pick(d: Direction): Int64 do \
             if d is Direction.East then 1 elseif d is Direction.West then 2 end end";
    assert_checks(&format!("{DIRECTION}{chain}"));
    assert_error_contains(
        &format!("type Direction is union | East | West | North end\n{chain}"),
        "does not handle Direction.North",
    );
}

/// Conformance 23: direct and indirect recursive layouts.
#[test]
fn recursive_value_layouts_are_rejected() {
    assert_error_contains(
        "type Node is struct next: Node, end",
        "Type 'Node' has an infinite value layout: Node -> Node",
    );
    assert_error_contains(
        "type A is struct b: B, end\ntype B is struct a: A, end",
        "has an infinite value layout: A -> B -> A",
    );
    assert_error_contains(
        "type A is B\ntype B is A",
        "has an infinite value layout: A -> B -> A",
    );
    assert_error_contains(
        "type Tree is union | Leaf | Branch is struct child: Tree, end end",
        "has an infinite value layout",
    );
}

/// Conformance 24: no user-defined type crosses the Rust bridge.
#[test]
fn user_defined_types_are_rejected_at_every_bridge_site() {
    for declaration in [
        "extern rust \"snacc_user_take\" fun take(value: Point)",
        "extern rust \"snacc_user_make\" fun make(): Point",
    ] {
        assert_error_contains(
            &format!("{POINT}{declaration}"),
            "only the ABI's permitted types may cross a Rust bridge",
        );
    }
    assert_error_contains(
        &format!("{SHAPE}extern rust \"snacc_user_take\" fun take(value: Shape.Circle)"),
        "'Shape.Circle' is a user-defined type",
    );
    assert_error_contains(
        "type UserId is Int64\nextern rust \"snacc_user_take\" fun take(value: UserId)",
        "'UserId' is a user-defined type",
    );
    // Internal Snacc functions accept and return them freely.
    assert_checks(&format!(
        "{POINT}fun identity(point: Point): Point do point end\n\
             print(identity(Point(x: 1.0, y: 2.0)).x)"
    ));
}

/// Conformance 14 (checking half): a union type flows from an expected
/// type through branches and arguments.
#[test]
fn common_union_types_come_from_the_expected_type() {
    assert_checks(&format!(
        "{DIRECTION}\
             fun pick(flag: Bool): Direction do \
                if flag then Direction.East() else Direction.West() end end\n\
             fun take(d: Direction): Int64 do 1 end\n\
             print(take(Direction.East()))\nprint(take(pick(true)))"
    ));
    assert_error_contains(
        &format!(
            "{DIRECTION}{SHAPE}\
                 fun pick(flag: Bool): Direction do \
                    if flag then Direction.East() else Shape.Circle(radius: 1) end end"
        ),
        "expected 'Direction', found 'Shape.Circle'",
    );
    assert_error_contains(
        &format!(
            "{DIRECTION}fun pick(flag: Bool): Int64 do \
                 if flag then Direction.East() else 1 end end"
        ),
        "expected 'Int64', found 'Direction.East'",
    );
}

/// Specification 012 section 10: `Nil` is a union member and `nil` names it
/// from an expected union type.
#[test]
fn nil_is_available_as_a_union_member() {
    assert_checks(
        "type UserId is Int64\n\
             type MaybeUser is union | User is struct id: UserId, end | Nil end\n\
             let missing: MaybeUser = nil\n\
             let present: MaybeUser = MaybeUser.User(id: UserId(10))\n\
             if missing is Nil then print(1) elseif missing is MaybeUser.User(user) then \
             print(Int64(user.id)) end\n\
             print(missing == nil)",
    );
    assert_error_contains(
        "type MaybeUser is union | User is struct id: Int64, end | Nil end\n\
             let missing: MaybeUser = nil\n\
             if missing is Nil(value) then print(1) end",
        "'Nil' carries no value, so it cannot be bound by a type test",
    );
    assert_error_contains(
        "type Empty is union | Nil end",
        "contains only 'Nil'; 'Nil' requires another member type",
    );
}

/// Specification 012 conformance 16: standalone `Nil` is rejected in every
/// type and bridge position.
#[test]
fn standalone_nil_is_rejected_in_every_type_position() {
    for source in [
        "let value: Nil = nil",
        "fun consume(value: Nil) do print(1) end",
        "fun produce(): Nil do nil end",
        "fun update(value: Ref<Nil>) do print(1) end",
        "type Empty is Nil",
        "type Holder is struct value: Nil, end",
        "type Maybe is union | Held is struct value: Nil, end | Nil end",
        "extern rust \"snacc_user_take\" fun take(value: Nil)\nprint(0)",
        "extern rust \"snacc_user_make\" fun make(): Nil\nprint(0)",
        "extern rust \"snacc_user_ref\" fun update(value: Ref<Nil>)\nprint(0)",
        &format!("{POINT}method Point.at(value: Nil) do print(1) end"),
        &format!("{POINT}method Point.at(): Nil do nil end"),
    ] {
        assert_error_contains(source, "'Nil' is not a standalone type");
    }
}

/// Specification 012 conformance 17: `nil` needs one expected
/// Nil-containing union. Specification 020 section 8 removes `null` as an
/// alternate spelling entirely.
#[test]
fn contextual_nil_requires_one_nil_containing_union() {
    assert_checks(
        "type Maybe is union | Some is struct value: Int64, end | Nil end\n\
             let missing: Maybe = nil\n\
             fun absent(): Maybe do nil end\n\
             fun take(value: Maybe): Bool do value == nil end\n\
             print(take(absent()))",
    );
    for source in ["print(nil)", "print(nil == nil)"] {
        assert_error_contains(source, "'nil' has no type of its own");
    }
    assert_error_contains("let value: Int64 = nil", "expected 'Int64', found 'Nil'");
}

/// Specification 020 section 8: `null` is an ordinary identifier with no
/// built-in meaning, so an unresolved use gets the same diagnostic as any
/// other undeclared name -- never a `nil`-specific one.
#[test]
fn null_is_an_ordinary_undeclared_identifier_not_a_nil_spelling() {
    assert_error_contains("print(null)", "No such variable 'null' in scope");
    assert_checks("let null: Int64 = 10\nprint(null)");
}

/// Conformance 4 and 17 diagnostics: duplicate fields and members.
#[test]
fn duplicate_fields_and_union_members_are_rejected() {
    assert_error_contains(
        "type Point is struct x: Float64, x: Float64, end",
        "Field 'Point.x' already exists",
    );
    assert_error_contains(
        "type Shape is union | Circle | Circle end",
        "Union member 'Shape.Circle' already exists",
    );
}

/// Conformance 14 and 18: field access needs a struct, and printing a
/// user-defined type is rejected rather than silently accepted.
#[test]
fn field_access_and_printing_reject_unsupported_receivers() {
    assert_error_contains(
        "let value: Int64 = 1\nprint(value.field)",
        "'Int64' is not a struct, so it has no field 'field'",
    );
    assert_error_contains(
        &format!("{SHAPE}let shape: Shape = Shape.Circle(radius: 1)\nprint(shape.radius)"),
        "'Shape' is not a struct, so it has no field 'radius'",
    );
    assert_error_contains(
        &format!("{POINT}print(Point(x: 1.0, y: 2.0))"),
        "'print' does not support the user-defined type 'Point'",
    );
    assert_error_contains(
        "type UserId is Int64\nprint(UserId(1))",
        "'print' does not support the user-defined type 'UserId'",
    );
    assert_checks("type UserId is Int64\nprint(Int64(UserId(1)))");
}

/// Conformance 25: user-defined types combine with RFC 008 no-result
/// functions and Specification 009 scalar fields.
#[test]
fn user_types_combine_with_no_result_functions_and_fixed_width_scalars() {
    assert_checks(
        "type Pixel is struct red: Byte, green: Byte, blue: Byte, ratio: Float32, end\n\
             method Pixel.brightest(): Byte do self.red end\n\
             fun announce(value: Byte) do print(value) end\n\
             let pixel: Pixel = Pixel(red: 1u8, green: 2u8, blue: 3u8, ratio: 0.5f32)\n\
             announce(pixel.brightest())\n\
             print(pixel.ratio)",
    );
}

/// A method on a union receiver takes the whole union value.
#[test]
fn methods_attach_to_unions_and_to_their_members() {
    assert_checks(&format!(
        "{SHAPE}\
             method Shape.Circle.area(): Int64 do self.radius * self.radius end\n\
             method Shape.describe(): Int64 do \
                if self is Shape.Circle(circle) then circle.area() \
                elseif self is Shape.Rectangle(rectangle) then \
                rectangle.length * rectangle.width end end\n\
             let shape: Shape = Shape.Circle(radius: 2)\nprint(shape.describe())"
    ));
    // A member method is not callable on the union.
    assert_error_contains(
        &format!(
            "{SHAPE}method Shape.Circle.area(): Int64 do self.radius end\n\
                 let shape: Shape = Shape.Circle(radius: 2)\nprint(shape.area())"
        ),
        "'Shape' has no method 'area'",
    );
}

// ---------------------------------------------------------------------
// Specification 011: call-scoped reference parameters.
// ---------------------------------------------------------------------

const ADD_INTO: &str =
    "fun add_into(x: Int64, y: Int64, result: Ref<Int64>) do result = x + y end\n";
const EXCHANGE: &str = "fun exchange(left: Ref<Float64>, right: Ref<Float64>) do \
                            let saved: Float64 = left left = right right = saved end\n";

/// The checked arguments of the first top-level call statement.
fn top_level_args(program: &Program) -> &[TArg] {
    program
        .body
        .statements
        .iter()
        .find_map(|statement| match statement {
            TStmt::Call(_, args) | TStmt::Expr(TExpr::Call(_, args)) => Some(args.as_slice()),
            TStmt::MethodCall(call) => Some(call.args.as_slice()),
            _ => None,
        })
        .expect("the top-level body contains a call")
}

/// Conformance 1-2: the canonical example checks, and its reference
/// argument survives into the checked program as a resolved place.
#[test]
fn a_reference_parameter_carries_a_resolved_place_to_lowering() {
    let program = assert_checks(&format!(
        "{ADD_INTO}let x: Int64 = 20\nlet y: Int64 = 22\n\
             let mut z: Int64 = 0\nadd_into(x, y, z)\nprint(z)"
    ));
    assert_eq!(
        program.funcs["add_into"].params[2].mode,
        ParamMode::Reference
    );
    assert_eq!(program.funcs["add_into"].params[2].ty, Ty::Int64);
    assert_eq!(program.funcs["add_into"].params[0].mode, ParamMode::Value);
    let args = top_level_args(&program);
    assert!(matches!(args[0], TArg::Value(_)));
    assert!(matches!(args[1], TArg::Value(_)));
    let TArg::Reference(place) = &args[2] else {
        panic!("the third argument should be a reference place");
    };
    assert_eq!(place.root, PlaceRoot::Local("z".into()));
    assert_eq!(place.ty, Ty::Int64);
    assert!(place.path.is_empty());
}

/// Conformance 1 and 7.2: inside the callee a reference parameter is an
/// ordinary mutable root, so it reads, writes, and selects fields through
/// the same machinery a `let mut` local uses.
#[test]
fn a_reference_parameter_is_a_mutable_root_of_its_referent_type() {
    assert_checks("fun increment(value: Ref<Int64>) do value = value + 1 end\nprint(0)");
    // A by-value parameter is still immutable, so the mode -- not the
    // parameter position -- is what grants the capability.
    assert_error_contains(
        "fun increment(value: Int64) do value = value + 1 end\nprint(0)",
        "'value' is not declared 'mut' and cannot be assigned",
    );
}

/// Conformance 3: only an initialized mutable place can establish a
/// reference.
#[test]
fn rejects_every_reference_argument_that_is_not_a_mutable_place() {
    assert_error_contains(
        &format!("{ADD_INTO}let total: Int64 = 0\nadd_into(20, 22, total)"),
        "'total' is not declared 'mut', so it cannot be passed to the reference \
             parameter 'result' of 'add_into'",
    );
    for argument in ["0", "make_total()", "1 + 2"] {
        assert_error_contains(
            &format!("{ADD_INTO}fun make_total(): Int64 do 0 end\nadd_into(20, 22, {argument})"),
            "requires an initialized mutable place of type 'Int64', but this argument \
                 is a value with no storage",
        );
    }
}

/// Conformance 4: there is no uninitialized declaration, and an initialized
/// mutable local is the operational output form.
#[test]
fn a_referent_is_always_an_initialized_mutable_local() {
    assert_rejected_by_parser("let result: Int64");
    assert_checks(&format!(
        "{ADD_INTO}let mut result: Int64 = 0\nadd_into(20, 22, result)\nprint(result)"
    ));
}

/// Conformance 5 and 9: a field place rooted at a mutable variable is a
/// valid referent, and the callee reaches the caller's field through it.
#[test]
fn struct_fields_rooted_at_a_mutable_variable_are_valid_referents() {
    let program = assert_checks(&format!(
        "{POINT}fun move_right(point: Ref<Point>, amount: Float64) do \
             point.x = point.x + amount end\n\
             fun bump(value: Ref<Float64>) do value = value + 1.0 end\n\
             let mut point: Point = Point(x: 1.0, y: 2.0)\n\
             bump(point.x)\n\
             move_right(point, 1.0)"
    ));
    let TArg::Reference(place) = &top_level_args(&program)[0] else {
        panic!("expected a reference argument");
    };
    assert_eq!(place.root, PlaceRoot::Local("point".into()));
    assert_eq!(place.path, vec![0]);
    assert_eq!(place.ty, Ty::Float64);
    // An immutable root is rejected even when the field itself is written.
    assert_error_contains(
        &format!(
            "{POINT}fun bump(value: Ref<Float64>) do value = value + 1.0 end\n\
                 let point: Point = Point(x: 1.0, y: 2.0)\nbump(point.x)"
        ),
        "'point' is not declared 'mut'",
    );
}

/// Conformance 6: the referent type is exact. Neither the `Int64`-to-`Float64`
/// widening nor represented-type equivalence establishes a reference.
#[test]
fn a_reference_argument_requires_the_exact_referent_type() {
    assert_error_contains(
        "fun scale(value: Ref<Float64>) do value = value * 2.0 end\n\
             let mut count: Int64 = 1\nscale(count)",
        "reference parameter 'value' of 'scale' requires a place of exactly type \
             'Float64', found 'Int64'",
    );
    // The same value widens happily when the parameter is by value.
    assert_checks(
        "fun scale(value: Float64): Float64 do value * 2.0 end\n\
             let count: Int64 = 1\nprint(scale(count))",
    );
    assert_error_contains(
        "type UserId is Int64\nfun bump(value: Ref<Int64>) do value = value + 1 end\n\
             let mut id: UserId = UserId(1)\nbump(id)",
        "requires a place of exactly type 'Int64', found 'UserId'",
    );
    assert_error_contains(
        "type UserId is Int64\nfun bump(value: Ref<UserId>) do value = UserId(1) end\n\
             let mut raw: Int64 = 1\nbump(raw)",
        "requires a place of exactly type 'UserId', found 'Int64'",
    );
}

/// Conformance 7 and 11: an automatic read produces `T`, after which every
/// ordinary rule for `T` applies -- including widening and copying into a
/// by-value parameter.
#[test]
fn an_automatic_read_behaves_exactly_like_a_loaded_value() {
    assert_checks(
        "fun show(value: Int64) do print(value) end\n\
             fun widen(value: Float64): Float64 do value end\n\
             fun use_all(value: Ref<Int64>): Bool do\n\
                 print(value)\n\
                 show(value)\n\
                 print(widen(value))\n\
                 value > 0\n\
             end\n\
             let mut count: Int64 = 1\nprint(use_all(count))",
    );
}

/// Conformance 8: a whole-value assignment through a reference works for
/// every category of referent.
#[test]
fn assignment_through_a_reference_replaces_a_complete_value() {
    assert_checks(&format!(
        "{POINT}{SHAPE}type UserId is Int64\n\
             fun set_scalar(value: Ref<Int64>) do value = 7 end\n\
             fun set_represented(value: Ref<UserId>) do value = UserId(7) end\n\
             fun set_struct(value: Ref<Point>) do value = Point(x: 0.0, y: 0.0) end\n\
             fun set_union(value: Ref<Shape>) do value = Shape.Circle(radius: 1) end\n\
             let mut scalar: Int64 = 0\n\
             let mut id: UserId = UserId(0)\n\
             let mut point: Point = Point(x: 1.0, y: 1.0)\n\
             let mut shape: Shape = Shape.Circle(radius: 0)\n\
             set_scalar(scalar)\nset_represented(id)\nset_struct(point)\nset_union(shape)"
    ));
    assert_error_contains(
        "fun set(value: Ref<Int64>) do value = true end\nprint(0)",
        "expected 'Int64', found 'Bool'",
    );
}

/// Conformance 10 and 7.3: forwarding is a reborrow with no extra ceremony
/// -- the parameter is already a mutable root, so the ordinary place and
/// mutability rules accept it.
#[test]
fn forwarding_a_reference_parameter_reborrows_it() {
    let program = assert_checks(
        "fun increment(value: Ref<Int64>) do value = value + 1 end\n\
             fun twice(value: Ref<Int64>) do increment(value) increment(value) end\n\
             let mut count: Int64 = 0\ntwice(count)",
    );
    let TStmt::Call(_, args) = &program.funcs["twice"].body.statements[0] else {
        panic!("expected a forwarded call");
    };
    let TArg::Reference(place) = &args[0] else {
        panic!("a forwarded reference parameter stays a reference argument");
    };
    assert_eq!(place.root, PlaceRoot::Local("value".into()));
    // Forwarding a field of a referenced struct reborrows the same way.
    assert_checks(&format!(
        "{POINT}fun bump(value: Ref<Float64>) do value = value + 1.0 end\n\
             fun bump_x(point: Ref<Point>) do bump(point.x) end\nprint(0)"
    ));
    // A by-value parameter still cannot supply a reference.
    assert_error_contains(
        "fun increment(value: Ref<Int64>) do value = value + 1 end\n\
             fun twice(value: Int64) do increment(value) end\nprint(0)",
        "'value' is not declared 'mut'",
    );
}

/// Conformance 11: supplying a reference parameter to a by-value parameter
/// copies the current referent instead of forwarding the reference.
#[test]
fn a_reference_parameter_supplied_by_value_is_copied() {
    let program = assert_checks(
        "fun show(value: Int64) do print(value) end\n\
             fun relay(value: Ref<Int64>) do show(value) end\nprint(0)",
    );
    let TStmt::Call(_, args) = &program.funcs["relay"].body.statements[0] else {
        panic!("expected a value call");
    };
    assert!(matches!(args[0], TArg::Value(_)));
}

/// Conformance 12-13: overlap is decided from resolved roots and field
/// paths. Identical places and prefix relationships overlap; sibling fields
/// do not.
#[test]
fn overlapping_reference_arguments_are_rejected_and_siblings_are_accepted() {
    assert_error_contains(
        &format!("{EXCHANGE}let mut value: Float64 = 1.0\nexchange(value, value)"),
        "reference arguments 'value' and 'value' overlap, so parameters 'left' and \
             'right' cannot both have exclusive access",
    );
    assert_error_contains(
        &format!(
            "{POINT}fun use_both(whole: Ref<Point>, part: Ref<Float64>) do \
                 part = whole.y end\n\
                 let mut point: Point = Point(x: 1.0, y: 2.0)\nuse_both(point, point.x)"
        ),
        "reference arguments 'point' and 'point.x' overlap",
    );
    // Two statically distinct fields of the same mutable struct are
    // disjoint, so both may be referenced at once.
    assert_checks(&format!(
        "{POINT}{EXCHANGE}let mut point: Point = Point(x: 1.0, y: 2.0)\n\
             exchange(point.x, point.y)"
    ));
    // Two different mutable roots are always disjoint.
    assert_checks(&format!(
        "{EXCHANGE}let mut a: Float64 = 1.0\nlet mut b: Float64 = 2.0\nexchange(a, b)"
    ));
}

/// Nested field paths overlap only through a common prefix.
#[test]
fn overlap_compares_complete_field_paths() {
    const NESTED: &str = "type Point is struct x: Float64, y: Float64, end\n\
                              type Entity is struct position: Point, velocity: Point, end\n";
    assert_checks(&format!(
        "{NESTED}{EXCHANGE}let mut entity: Entity = Entity(\
             position: Point(x: 0.0, y: 0.0), velocity: Point(x: 0.0, y: 0.0))\n\
             exchange(entity.position.x, entity.velocity.x)"
    ));
    assert_error_contains(
        &format!(
            "{NESTED}fun use_both(whole: Ref<Point>, part: Ref<Float64>) do \
                 part = whole.y end\n\
                 let mut entity: Entity = Entity(\
                 position: Point(x: 0.0, y: 0.0), velocity: Point(x: 0.0, y: 0.0))\n\
                 use_both(entity.position, entity.position.x)"
        ),
        "reference arguments 'entity.position' and 'entity.position.x' overlap",
    );
}

/// Conformance 14: an addressable receiver participates in overlap checking
/// for the whole call, whether or not the method writes through `self`; a
/// temporary receiver cannot overlap a caller place.
#[test]
fn a_method_receiver_participates_in_overlap_checking() {
    const READ_ONLY: &str = "type Point is struct x: Float64, y: Float64, end\n\
                                 method Point.give(other: Ref<Float64>) do other = self.x end\n";
    const WRITING: &str = "type Point is struct x: Float64, y: Float64, end\n\
                               method Point.take(other: Ref<Float64>) do self.x = other end\n";
    for source in [READ_ONLY, WRITING] {
        let name = if std::ptr::eq(source, READ_ONLY) {
            "give"
        } else {
            "take"
        };
        assert_error_contains(
            &format!("{source}let mut point: Point = Point(x: 1.0, y: 2.0)\npoint.{name}(point.y)"),
            "overlaps the receiver 'point', which the method may access through 'self'",
        );
    }
    // An unrelated mutable place is accepted for either method.
    assert_checks(&format!(
        "{READ_ONLY}let mut total: Float64 = 0.0\n\
             let point: Point = Point(x: 1.0, y: 2.0)\npoint.give(total)"
    ));
    // A temporary receiver has independent storage.
    assert_checks(&format!(
        "{READ_ONLY}let mut total: Float64 = 0.0\nPoint(x: 1.0, y: 2.0).give(total)"
    ));
}

/// A `self`-rooted reference argument may be written by the callee, so it
/// feeds the same receiver-write effect an assignment to `self` would.
#[test]
fn passing_a_self_rooted_place_by_reference_is_a_receiver_write() {
    assert_error_contains(
        &format!(
            "{POINT}fun bump(value: Ref<Float64>) do value = value + 1.0 end\n\
                 method Point.grow() do bump(self.x) end\n\
                 let point: Point = Point(x: 1.0, y: 2.0)\npoint.grow()"
        ),
        "may assign through 'self', so its receiver requires a mutable root",
    );
    assert_checks(&format!(
        "{POINT}fun bump(value: Ref<Float64>) do value = value + 1.0 end\n\
             method Point.grow() do bump(self.x) end\n\
             let mut point: Point = Point(x: 1.0, y: 2.0)\npoint.grow()"
    ));
}

/// Section 7.2: method lookup uses the referent type `T`, and a
/// receiver-writing method is valid because the referent is a mutable root.
#[test]
fn methods_resolve_through_a_reference_parameter() {
    assert_checks(&format!(
        "{POINT}method Point.length(): Float64 do self.x + self.y end\n\
             method Point.reset() do self.x = 0.0 end\n\
             fun report(point: Ref<Point>) do\n\
                 print(point.length())\n\
                 point.reset()\n\
             end\n\
             let mut point: Point = Point(x: 1.0, y: 2.0)\nreport(point)"
    ));
    // A by-value parameter still cannot receive a receiver-writing method.
    assert_error_contains(
        &format!(
            "{POINT}method Point.reset() do self.x = 0.0 end\n\
                 fun report(point: Point) do point.reset() end\nprint(0)"
        ),
        "may assign through 'self', so its receiver requires a mutable root",
    );
}

/// Conformance 15: a value argument is read before the callee starts, so
/// reading a place by value and referencing it in the same call is valid.
#[test]
fn the_same_place_may_be_read_by_value_and_referenced_in_one_call() {
    let program = assert_checks(
        "fun replace(previous: Int64, value: Ref<Int64>) do value = previous + 1 end\n\
             let mut number: Int64 = 4\nreplace(number, number)",
    );
    let args = top_level_args(&program);
    assert!(matches!(args[0], TArg::Value(_)));
    assert!(matches!(args[1], TArg::Reference(_)));
}

/// Conformance 16-17: a reference is not storable and cannot be built,
/// returned, or named anywhere but a parameter.
#[test]
fn a_reference_is_never_storable_or_constructible() {
    for source in [
        "fun f(value: Ref<Int64>): Ref<Int64> do value end",
        "let saved: Ref<Int64> = 1",
        "type Holder is struct value: Ref<Int64>, end",
        "type Alias is Ref<Int64>",
        "type Shape is union | Circle is struct radius: Ref<Int64>, end | Nil end",
        "fun f(value: Ref<Ref<Int64>>) do print(1) end",
        "method Point.f(self: Ref<Point>) do print(1) end",
        // No constructor, and `Ref` is reserved so it is not a call head.
        "let saved: Int64 = Ref(1)",
    ] {
        assert_rejected_by_parser(source);
    }
    // Returning a reference parameter returns a copy of its referent, which
    // is an ordinary value result.
    assert_checks(
        "fun read(value: Ref<Int64>): Int64 do value end\n\
             let mut count: Int64 = 1\nprint(read(count))",
    );
}

/// Reference parameters compose with methods and with Rust bridges.
#[test]
fn reference_parameters_are_permitted_on_methods_and_bridges() {
    assert_checks(&format!(
        "{POINT}method Point.give(other: Ref<Float64>) do other = self.x end\n\
             extern rust \"snacc_user_bump\" fun rust_bump(value: Ref<Int64>)\n\
             let mut total: Float64 = 0.0\nlet mut count: Int64 = 0\n\
             let point: Point = Point(x: 1.0, y: 2.0)\n\
             point.give(total)\nrust_bump(count)"
    ));
    // Specification 011 section 12.1: a bridge referent is an ABI scalar.
    assert_error_contains(
        &format!(
            "{POINT}extern rust \"snacc_user_move\" fun rust_move(point: Ref<Point>)\n\
                 print(0)"
        ),
        "only the ABI's permitted types may cross a Rust bridge",
    );
}

// ---------------------------------------------------------------------
// Specification 018: inline sum types.
// ---------------------------------------------------------------------

/// Conformance 2-3: normalized member order and grouping never affect
/// identity, so a differently written but member-set-identical sum is
/// exactly assignable.
#[test]
fn sum_identity_ignores_member_order() {
    assert_checks("let a: Byte | Nil = nil\nlet b: Nil | Byte = a\nprint(0)");
}

#[test]
fn sum_identity_ignores_parenthesized_grouping() {
    assert_checks("let a: Bool | Nil | Byte = nil\nlet b: (Byte | Bool) | Nil = a\nprint(0)");
}

/// Conformance 3: duplicates and fewer than two members are rejected.
#[test]
fn rejects_fewer_than_two_distinct_sum_members() {
    assert_error_contains(
        "let x: Byte | Byte = 1u8",
        "at least two distinct member types",
    );
}

#[test]
fn rejects_a_repeated_member_after_flattening() {
    assert_error_contains(
        "let x: (Byte | Bool) | Byte = 1u8",
        "is repeated in this sum type",
    );
}

#[test]
fn rejects_a_lone_nil_member() {
    assert_error_contains(
        "let x: Nil | Nil = nil",
        "valid in a sum type only alongside",
    );
}

/// Specification 018 section 4: an unresolved member is reported once,
/// through the same "Unknown type" diagnostic every other position uses.
#[test]
fn an_unresolved_sum_member_is_reported_once() {
    assert_error_contains("let x: Foo | Nil = nil", "Unknown type 'Foo'");
}

/// Specification 020 section 11/12: `Dec64` has no built-in meaning or
/// compatibility alias and is diagnosed like any other unknown type name,
/// unless a user declaration independently defines it.
#[test]
fn stale_dec64_name_has_no_built_in_meaning() {
    assert_error_contains("let x: Dec64 = 1.0", "Unknown type 'Dec64'");
    assert_checks("type Dec64 is Int64\nlet x: Dec64 = Dec64(1)\nprint(0)");
}

/// Conformance 5: an inline sum has no callable type name, so it cannot be
/// a represented type's immediate representation.
#[test]
fn an_inline_sum_cannot_be_a_represented_types_target() {
    assert_error_contains(
        "type MaybeByte is Byte | Nil",
        "cannot be a represented type's immediate representation",
    );
}

/// Specification 018 section 3: a reference is not a value-type member,
/// so it fails to parse as one -- the parser establishes this, and the
/// checker never sees a `Ref<T>` sum member.
#[test]
fn a_reference_cannot_be_a_sum_member() {
    assert_rejected_by_parser("let value: Ref<Byte> | Nil = nil");
}

/// Specification 018 section 8: a self-referential inline sum field has
/// no more indirection than a plain self-referential field, so it is
/// still an infinite value layout.
#[test]
fn a_self_referential_inline_sum_field_is_an_infinite_layout() {
    assert_error_contains(
        "type Node is struct next: Node | Nil, end",
        "has an infinite value layout",
    );
}

/// Conformance 8: a named union is one opaque direct member; its own
/// members do not flatten into the inline sum.
#[test]
fn a_named_union_is_one_opaque_member_of_an_inline_sum() {
    assert_checks(&format!(
        "{SHAPE}fun classify(value: Shape | Nil): Int64 do \
             if value is Shape(shape) then \
                 if shape is Shape.Circle(circle) then circle.radius \
                 elseif shape is Shape.Rectangle(rectangle) then rectangle.length end \
             elseif value is Nil then 0 end end\n\
             print(classify(nil))"
    ));
}

#[test]
fn a_sum_type_test_cannot_name_a_member_inside_a_named_union_member() {
    assert_error_contains(
        &format!(
            "{SHAPE}fun f(value: Shape | Nil): Int64 do \
                 if value is Shape.Circle(circle) then circle.radius \
                 elseif value is Nil then 0 end end",
        ),
        "a type test on an inline sum names exactly one direct member",
    );
}

/// Conformance 4: a direct value and a contextual `nil` both inject.
#[test]
fn direct_values_and_contextual_nil_inject_into_an_expected_sum() {
    let program =
        assert_checks("let present: Byte | Nil = 1u8\nlet absent: Byte | Nil = nil\nprint(0)");
    let TStmt::Let { value, .. } = &program.body.statements[0] else {
        panic!("expected a declaration");
    };
    assert!(matches!(
        value,
        TExpr::InjectSum {
            member: Ty::Byte,
            ..
        }
    ));
    let TStmt::Let { value, .. } = &program.body.statements[1] else {
        panic!("expected a declaration");
    };
    assert!(matches!(
        value,
        TExpr::InjectSum {
            member: Ty::Nil,
            ..
        }
    ));
}

/// Specification 018 section 5: an exact direct member match wins over an
/// available widening conversion.
#[test]
fn an_exact_member_match_wins_over_widening() {
    let program = assert_checks("let x: Float64 | Int64 = 1\nprint(0)");
    let TStmt::Let { value, .. } = &program.body.statements[0] else {
        panic!("expected a declaration");
    };
    assert!(matches!(
        value,
        TExpr::InjectSum {
            member: Ty::Int64,
            ..
        }
    ));
}

/// With no exact `Int64` member, the value still widens through the one
/// existing implicit conversion.
#[test]
fn an_int64_value_widens_into_the_sums_float64_member() {
    let program = assert_checks("let x: Float64 | Nil = 1\nprint(0)");
    let TStmt::Let { value, .. } = &program.body.statements[0] else {
        panic!("expected a declaration");
    };
    let TExpr::InjectSum { member, value, .. } = value else {
        panic!("expected a sum injection");
    };
    assert_eq!(*member, Ty::Float64);
    assert!(matches!(**value, TExpr::Cast(_, Ty::Float64)));
}

#[test]
fn rejects_a_value_with_no_matching_sum_member() {
    assert_error_contains("let x: Bool | Byte = 1.5", "found 'Float64'");
}

/// Section 5: a named union's own member does not directly inject into an
/// inline sum; only an already-`Shape`-typed value does.
#[test]
fn a_named_unions_member_value_does_not_directly_inject_into_an_inline_sum() {
    assert_error_contains(
        &format!("{SHAPE}let combined: Shape | Nil = Shape.Circle(radius: 1)"),
        "found 'Shape.Circle'",
    );
    assert_checks(&format!(
        "{SHAPE}let shape: Shape = Shape.Circle(radius: 1)\n\
             let combined: Shape | Nil = shape\nprint(0)"
    ));
}

/// Conformance 6: sum-to-sum assignment requires identical normalized
/// member sets; there is no subset-to-superset conversion.
#[test]
fn sum_to_sum_assignment_requires_identical_member_sets() {
    assert_checks("let a: Byte | Nil = nil\nlet b: Byte | Nil = a\nprint(0)");
    assert_error_contains(
        "let narrow: Byte | Nil = nil\nlet wide: Bool | Nil | Byte = narrow",
        "expected 'Bool | Nil | Byte', found 'Nil | Byte'",
    );
}

/// Conformance 7: type tests bind the exact tested member and support an
/// exhaustive chain with no `else`.
#[test]
fn is_tests_bind_the_exact_member_and_support_exhaustive_chains() {
    let program = assert_checks(
        "fun describe(value: Byte | Nil): Byte do \
             if value is Byte(byte) then byte elseif value is Nil then 0u8 end end\n\
             print(describe(nil))",
    );
    let result = program.funcs["describe"]
        .body
        .result
        .as_ref()
        .expect("describe produces a value");
    let TExpr::If(form) = result else {
        panic!("expected a value-form if");
    };
    assert!(form.exhaustive);
    assert!(form.else_branch.is_none());
    let TCondition::SumTest(first) = &form.arms[0].0 else {
        panic!("expected a sum type test");
    };
    assert_eq!(first.member, Ty::Byte);
    assert_eq!(first.binding.as_ref().map(|(_, ty)| *ty), Some(Ty::Byte));
}

#[test]
fn a_non_exhaustive_sum_chain_without_an_else_is_rejected() {
    assert_error_contains(
        "fun describe(value: Byte | Nil): Byte do \
             if value is Byte(byte) then byte end end",
        "does not handle Nil",
    );
}

#[test]
fn a_duplicate_sum_branch_is_rejected() {
    assert_error_contains(
        "fun describe(value: Byte | Nil): Byte do \
             if value is Byte(byte) then byte \
             elseif value is Byte(other) then other \
             elseif value is Nil then 0u8 end end",
        "'Byte' is already handled by an earlier branch",
    );
}

#[test]
fn an_exhaustive_sum_chain_rejects_a_redundant_else() {
    assert_error_contains(
        "fun describe(value: Byte | Nil): Byte do \
             if value is Byte(byte) then byte \
             elseif value is Nil then 0u8 \
             else 1u8 end end",
        "already covers every direct member",
    );
}

#[test]
fn a_sum_type_test_cannot_name_a_nonmember() {
    assert_error_contains(
        "fun f(value: Byte | Nil): Int64 do \
             if value is Bool(b) then 1 elseif value is Nil then 0 end end",
        "'Bool' is not a direct member of",
    );
}

#[test]
fn nil_cannot_be_bound_in_a_sum_type_test() {
    assert_error_contains(
        "fun f(value: Byte | Nil): Byte do \
             if value is Byte(byte) then byte elseif value is Nil(x) then 0u8 end end",
        "'Nil' carries no value, so it cannot be bound by a type test",
    );
}

/// Section 7: an explicit expected sum injects different branch values
/// without synthesizing a new type.
#[test]
fn an_expected_sum_permits_different_branch_types_without_synthesis() {
    assert_checks(
        "fun maybe_byte(found: Bool): Byte | Nil do \
             if found then 1u8 else nil end end\n\
             print(0)",
    );
}

/// Conformance 9: equality follows every member; unsupported operations on
/// the whole sum decompose first.
#[test]
fn sums_support_equality_when_every_member_does() {
    assert_checks("let a: Byte | Nil = nil\nlet b: Byte | Nil = 1u8\nprint(a == b)\nprint(a != b)");
}

#[test]
fn a_sum_value_compares_against_contextual_nil() {
    assert_checks("let a: Byte | Nil = 1u8\nprint(a == nil)");
}

#[test]
fn unsupported_operations_on_a_whole_sum_are_rejected() {
    let base = "let a: Byte | Nil = nil\nlet b: Byte | Nil = nil\n";
    assert_error_contains(
        &format!("{base}print(a < b)"),
        "operands must be two numbers of the same type",
    );
    assert_error_contains(
        &format!("{base}print(a + b)"),
        "operands must be two numbers of the same type",
    );
    assert_error_contains(
        &format!("{base}print(a.value)"),
        "is not a struct, so it has no field",
    );
    assert_error_contains(
        &format!("{base}print(a)"),
        "'print' does not support the inline sum type",
    );
}

/// Conformance 11: an inline sum cannot cross a Rust bridge, even when
/// every member individually could.
#[test]
fn an_inline_sum_cannot_cross_a_rust_bridge() {
    assert_error_contains(
        "extern rust \"snacc_user_maybe\" fun maybe(): Byte | Nil\nprint(0)",
        "no inline sum may cross a Rust bridge",
    );
}

/// A struct field may hold an inline sum, and construction injects into it
/// exactly like any other expected-sum position.
#[test]
fn a_struct_field_may_be_an_inline_sum() {
    assert_checks(
        "type CacheEntry is struct value: Byte | Nil, end\n\
             let empty: CacheEntry = CacheEntry(value: nil)\n\
             let full: CacheEntry = CacheEntry(value: 1u8)\nprint(0)",
    );
}

// RFC 016 Task A: `Box<T>` syntax, resolved types, and layout.

/// Specification 016 sections 4.1 and 12 (phase 1): `Box<T>` resolves as
/// an ordinary storable value type in every position a plain value type
/// can occupy -- field, parameter, local, and result -- and round-trips
/// through the full `check()` pipeline into a resolved `Ty::Box`.
#[test]
fn a_box_type_resolves_as_a_field_parameter_local_and_result_type() {
    let program = assert_checks(
        "type Point is struct x: Int64, end\n\
             type Holder is struct value: Box<Point>, end\n\
             fun make(value: Box<Point>): Box<Point> do let local: Box<Point> = value local end\n\
             print(0)",
    );
    let holder = program
        .types
        .iter()
        .find(|def| def.name() == "Holder")
        .expect("Holder exists");
    assert!(
        matches!(holder.fields().unwrap()[0].1, Ty::Box(_)),
        "Holder.value should resolve to a 'Ty::Box'"
    );
    let make = &program.funcs["make"];
    assert!(
        matches!(make.params[0].ty, Ty::Box(_)),
        "make's parameter should resolve to a 'Ty::Box'"
    );
    assert!(
        matches!(make.result, Some(Ty::Box(_))),
        "make's result should resolve to a 'Ty::Box'"
    );
}

/// Specification 016 section 11: the wrong number of `Box` type
/// arguments is diagnosed. `Box<T>` accepts exactly one type argument
/// (section 4.1), so zero or more than one both fail to parse, the same
/// way a malformed `Ref<T>` would.
#[test]
fn rejects_the_wrong_number_of_box_type_arguments() {
    assert_rejected_by_parser("let value: Box = box(1)");
    assert_rejected_by_parser("let value: Box<> = box(1)");
    assert_rejected_by_parser("let value: Box<Int64, Bool> = box(1)");
}

/// Specification 016 section 4.1: `Ref<T>` is not storable, so it cannot
/// be a box's pointee -- like a reference nested inside a sum member
/// (`a_reference_cannot_be_a_sum_member` above), this fails to parse
/// rather than reaching resolution.
#[test]
fn a_reference_cannot_be_a_box_pointee() {
    assert_rejected_by_parser("let value: Box<Ref<Int64>> = box(1)");
}

/// Specification 016 section 4.1: a no-result type is not storable, so
/// it cannot be a box's pointee. There is no `TypeRef` spelling for a
/// no-result type, so the only way to attempt boxing one is
/// `box(call-to-a-no-result-function())` -- rejected by the pre-existing
/// no-result-call-as-value diagnostic (RFC 008 conformance 2) before any
/// box-specific checking runs.
#[test]
fn a_no_result_calls_result_cannot_be_boxed() {
    assert_error_contains(
        "fun log(value: Int64) do print(value) end\n\
             let boxed: Box<Int64> = box(log(1))\nprint(0)",
        "declares no result, so its call cannot be used as a value",
    );
}

/// Specification 016 section 4.1: `Box<Box<T>>` is valid.
#[test]
fn nested_box_is_accepted() {
    assert_checks("let value: Box<Box<Int64>> = box(box(1))\nprint(0)");
}

/// Specification 016 section 5.1's worked example (also section 3's
/// motivation): a recursive union crossing a `Box<T>` edge has a finite
/// layout and checks; the identical shape with the edge unbroken is
/// still an infinite value layout, exactly as before this RFC (compare
/// `recursive_value_layouts_are_rejected` above).
#[test]
fn a_box_edge_breaks_an_otherwise_infinite_recursive_union_layout() {
    assert_checks(
        "type IntLink is union | Empty | Item is struct value: Int64, \
             next: Box<IntLink>, end end\nprint(0)",
    );
    assert_error_contains(
        "type IntLink is union | Empty | Item is struct value: Int64, \
             next: IntLink, end end\nprint(0)",
        "has an infinite value layout",
    );
}

/// Specification 016 section 5.3: a struct with a `Box<T>` field is
/// move-only. `move_only_support`'s own fixed-point computation is
/// already unit-tested directly in `types.rs`; this proves the property
/// is reachable and correct on a realistic program through the same
/// declaration-collection phase `check()` itself starts from (`check`
/// calls `types::collect` as its first step).
#[test]
fn a_box_field_makes_its_struct_move_only_through_the_shared_pipeline() {
    let source = "type Holder is struct value: Box<Int64>, end\nprint(0)";
    assert_checks(source);
    let syntax = crate::parse(source).unwrap_or_else(|d| panic!("{source} should parse: {d:?}"));
    let mut errors = Vec::new();
    let collected = types::collect(&syntax, &mut errors);
    assert!(
        errors.is_empty(),
        "unexpected collection errors: {errors:?}"
    );
    let holder = collected.types.top_level("Holder").expect("Holder exists");
    assert!(
        collected.types.is_move_only(Ty::User(holder)),
        "a struct with a 'Box<T>' field must be move-only"
    );
}

/// Specification 016 section 4.1: `Box` is reserved, so it cannot be a
/// user-declared type, callable, parameter, or local name -- like `self`
/// (`self_is_rejected_outside_a_method` above), this fails to parse
/// rather than reaching the checker at all.
#[test]
fn box_is_reserved_and_cannot_be_a_declared_name() {
    assert_rejected_by_parser("type Box is Int64");
    assert_rejected_by_parser("fun Box(): Int64 do 1 end");
    assert_rejected_by_parser("fun f(Box: Int64): Int64 do 1 end");
    assert_rejected_by_parser("fun f(): Int64 do let Box: Int64 = 1 1 end");
}

/// Specification 016 section 10: `Box<T>` and every type transitively
/// containing one are rejected in `extern rust` parameters and results.
/// A direct `Box<T>` parameter or result gets its own diagnostic naming
/// the box type; a struct containing a `Box<T>` field is already caught
/// by the pre-existing "no user-defined type crosses the bridge" rule
/// (`user_defined_types_are_rejected_at_every_bridge_site` above), which
/// rejects every `Ty::User` unconditionally regardless of its fields --
/// this proves the transitive case is actually enforced, not merely
/// claimed by the diagnostic text.
#[test]
fn box_and_every_type_transitively_containing_one_are_rejected_at_the_bridge() {
    assert_error_contains(
        "extern rust \"snacc_user_take\" fun take(value: Box<Int64>)",
        "is a box type; 'Box<T>' and every type transitively containing one",
    );
    assert_error_contains(
        "extern rust \"snacc_user_make\" fun make(): Box<Int64>",
        "is a box type; 'Box<T>' and every type transitively containing one",
    );
    assert_error_contains(
        "type Holder is struct value: Box<Int64>, end\n\
             extern rust \"snacc_user_take\" fun take(value: Holder)",
        "is a user-defined type",
    );
}

// RFC 016 Task B (first half): consuming-context classification and the
// available/moved control-flow analysis (Specification 016 sections 6.1
// and 6.2). `Box<Int64>` stands in for every move-only type here since
// `Types::is_move_only` already has its own direct unit tests; these
// exercise the dataflow pass, not move-only classification itself.

/// Specification 016 section 6.1's worked example: a move-only value
/// moves out of its root on a consuming use, and a later consuming use of
/// the same root is rejected.
#[test]
fn using_a_moved_box_again_is_rejected() {
    assert_error_contains(
        "let first: Box<Int64> = box(1)\n\
             let second: Box<Int64> = first\n\
             let third: Box<Int64> = first\n\
             print(0)",
        "is already moved",
    );
}

/// Specification 016 section 6.2: a move confined to one non-exhaustive
/// `if` arm, with no later use, is fine -- there is nothing after the
/// merge to reject.
#[test]
fn a_move_inside_one_if_arm_with_no_later_use_is_accepted() {
    assert_checks(
        "let first: Box<Int64> = box(1)\n\
             if true then\n\
             \x20   let second: Box<Int64> = first\n\
             end\n\
             print(0)",
    );
}

/// Specification 016 section 6.2: a root moved on only one arm is not
/// available on the un-taken fall-through predecessor, so -- per "available
/// only when available on every reachable predecessor" -- it is unavailable
/// after the merge and a later use is rejected.
#[test]
fn a_move_inside_one_if_arm_is_unavailable_after_the_merge() {
    assert_error_contains(
        "let first: Box<Int64> = box(1)\n\
             if true then\n\
             \x20   let second: Box<Int64> = first\n\
             end\n\
             let third: Box<Int64> = first\n\
             print(0)",
        "is already moved",
    );
}

/// Specification 016 section 6.2: a root moved on every arm of an
/// exhaustive `if`/`else` is moved on every reachable predecessor, so it
/// is definitely gone after the merge.
#[test]
fn a_move_on_every_if_else_arm_is_unavailable_after_the_merge() {
    assert_error_contains(
        "let first: Box<Int64> = box(1)\n\
             if true then\n\
             \x20   let second: Box<Int64> = first\n\
             else\n\
             \x20   let third: Box<Int64> = first\n\
             end\n\
             let fourth: Box<Int64> = first\n\
             print(0)",
        "is already moved",
    );
}

/// Specification 016 section 6.3's closing sentence: assigning a fresh
/// value to a moved mutable local reinitializes it, so a later use
/// succeeds.
#[test]
fn reassigning_a_moved_mutable_local_restores_availability() {
    assert_checks(
        "let mut first: Box<Int64> = box(1)\n\
             let second: Box<Int64> = first\n\
             first = box(2)\n\
             let third: Box<Int64> = first\n\
             print(0)",
    );
}

/// Specification 016 section 6.2: a move inside a `while` body that is
/// never reinitialized would already be moved on a later iteration, so it
/// is rejected even though the single pass through the body that moves it
/// starts from an available root.
#[test]
fn a_move_inside_a_while_body_that_would_double_move_is_rejected() {
    assert_error_contains(
        "let first: Box<Int64> = box(1)\n\
             while true do\n\
             \x20   let second: Box<Int64> = first\n\
             end\n\
             print(0)",
        "already moved",
    );
}

/// Specification 016 section 6.3's closing sentence, inside a loop: a
/// `while` body that reinitializes the moved root before the body ends is
/// safe to repeat, so no double-move diagnostic fires.
#[test]
fn a_while_body_that_reinitializes_after_moving_is_accepted() {
    assert_checks(
        "let mut first: Box<Int64> = box(1)\n\
             while true do\n\
             \x20   let second: Box<Int64> = first\n\
             \x20   first = box(2)\n\
             end\n\
             print(0)",
    );
}

/// Specification 016 section 5.3: a copyable type is never move-only, so
/// this analysis leaves it alone -- reusing the same local repeatedly is
/// an ordinary copy, exactly as it was before this task existed.
#[test]
fn copyable_values_may_be_used_repeatedly_without_move_tracking() {
    assert_checks(
        "let first: Int64 = 1\n\
             let second: Int64 = first\n\
             let third: Int64 = first\n\
             print(third)",
    );
}

// RFC 016 Task B (second half): subplace-move rejection, overlapping
// source/destination, union-test-binding aliasing through boxes, and
// `Ref<T>` lending from `Box<T>` (Specification 016 sections 6.3, 6.4,
// 7.2, and 7.3).

const NODE: &str = "type Node is struct value: Int64, end\n";
// `box(...)`'s operand is checked with no expected type (Specification
// 016 section 4.2's evaluation rule takes whatever the operand
// synthesizes), so `box(Tree.Branch(...))` directly would allocate a
// `Box<Tree.Branch>`, not the intended `Box<Tree>` -- injecting a member
// into its union already works before a box ever gets involved, so
// `leaf` binds the plain `Tree` value (letting that existing injection
// run) and boxes the already-widened result.
const TREE: &str = "type Tree is union\n\
         | Empty\n\
         | Branch is struct value: Int64, left: Box<Tree>, right: Box<Tree>, end\n\
         end\n\
         fun leaf(): Box<Tree> do let payload: Tree = Tree.Empty() box(payload) end\n";

/// Specification 016 section 6.4: a move-only struct field cannot be
/// moved out of; only the complete root may be consumed.
#[test]
fn moving_a_move_only_struct_field_out_is_rejected() {
    assert_error_contains(
        "type Holder is struct value: Box<Int64>, end\n\
             let holder: Holder = Holder(value: box(1))\n\
             let taken: Box<Int64> = holder.value\n\
             print(0)",
        "cannot be moved out of",
    );
}

/// Specification 016 section 6.4, composed with section 4.3's automatic
/// box dereference (Task B's item 6 verification): a move-only field
/// reached only by crossing a box is still a subplace, not a root.
#[test]
fn moving_a_move_only_field_through_an_automatic_box_dereference_is_rejected() {
    assert_error_contains(
        "type Holder is struct value: Box<Int64>, end\n\
             let boxed: Box<Holder> = box(Holder(value: box(1)))\n\
             let taken: Box<Int64> = boxed.value\n\
             print(0)",
        "cannot be moved out of",
    );
}

/// Specification 016 section 7.3: a union-test binding is a branch-scoped
/// alias to its tested place's active payload, never an independent
/// owning root, so consuming it whole is a subplace move exactly like
/// consuming one of its fields (below) already was.
#[test]
fn moving_a_union_test_binding_whole_is_rejected() {
    assert_error_contains(
        &format!(
            "{TREE}let payload: Tree = Tree.Branch(value: 1, left: leaf(), right: leaf())\n\
                 let tree: Box<Tree> = box(payload)\n\
                 if tree is Tree.Branch(branch) then\n\
                 let taken: Tree.Branch = branch\n\
                 end\n\
                 print(0)"
        ),
        "cannot be moved out of",
    );
}

/// Specification 016 section 7.3: a move-only field of a union-test
/// binding is a subplace of the tested root, so moving it out is
/// rejected the same way a field of an ordinary place is (Specification
/// 016 section 6.4). This is the field-level counterpart to
/// `moving_a_union_test_binding_whole_is_rejected` above.
#[test]
fn moving_a_field_out_of_a_box_wrapped_union_binding_is_rejected() {
    assert_error_contains(
        &format!(
            "{TREE}let payload: Tree = Tree.Branch(value: 1, left: leaf(), right: leaf())\n\
                 let mut tree: Box<Tree> = box(payload)\n\
                 if tree is Tree.Branch(branch) then\n\
                 let taken: Box<Tree> = branch.left\n\
                 end\n\
                 print(0)"
        ),
        "cannot be moved out of",
    );
}

/// Specification 016 section 6.4: rejecting a move out of a subplace
/// does not prevent reading a sibling copyable field, borrowing or
/// mutating the move-only subplace itself, or any of that through an
/// automatic box dereference.
#[test]
fn reading_borrowing_and_mutating_a_move_only_subplace_is_still_accepted() {
    assert_checks(
        "type Holder is struct tag: Int64, value: Box<Int64>, end\n\
             fun touch(value: Ref<Box<Int64>>) do print(1) end\n\
             let mut holder: Holder = Holder(tag: 1, value: box(2))\n\
             print(holder.tag)\n\
             touch(holder.value)\n\
             holder.value = box(3)\n\
             let mut boxed: Box<Holder> = box(Holder(tag: 1, value: box(2)))\n\
             print(boxed.tag)\n\
             touch(boxed.value)\n\
             boxed.value = box(4)\n\
             print(0)",
    );
}

/// Specification 016 section 6.3: `value = value` is a move whose source
/// overlaps its destination -- the source is available (this is its
/// first use), so nothing else would reject it.
#[test]
fn assigning_a_move_only_place_to_itself_is_rejected() {
    assert_error_contains(
        "let mut first: Box<Int64> = box(1)\n\
             first = first\n\
             print(0)",
        "overlap",
    );
}

/// Specification 016 section 6.3: a destination that is a projection of
/// its own source overlaps it too, not just an identical place. (The
/// source here also fails the assignment's ordinary type check, since a
/// field can never share its containing struct's exact type without
/// going through a box and this RFC gives boxing its own consuming node
/// rather than a bare place -- but the overlap diagnostic is independent
/// of that and still fires.)
#[test]
fn assigning_a_container_from_its_own_projection_is_rejected() {
    assert_error_contains(
        "type Wrapper is struct inner: Box<Int64>, other: Box<Int64>, end\n\
             let mut w: Wrapper = Wrapper(inner: box(1), other: box(2))\n\
             w.inner = w\n\
             print(0)",
        "overlap",
    );
}

/// Specification 016 section 6.3: two different roots never overlap, so
/// an ordinary reassignment between them is unaffected by the new checks.
#[test]
fn a_non_overlapping_move_only_reassignment_is_still_accepted() {
    assert_checks(
        "let mut a: Box<Int64> = box(1)\n\
             let b: Box<Int64> = box(2)\n\
             a = b\n\
             print(0)",
    );
}

/// Specification 016 section 7.3's worked example: testing a
/// `Box<Tree>`-typed union member binds a branch-scoped alias whose
/// fields may be read, mutated (the tested root is `mut`), and lent to a
/// `Ref<T>` parameter -- `branch.left`/`branch.right` are `Box<Tree>`
/// fields passed to a plain `Ref<Tree>` parameter, exercising Specification
/// 016 section 7.2's automatic pointee lending through the alias.
#[test]
fn a_box_wrapped_union_binding_permits_read_borrow_and_mutation_through_a_mutable_root() {
    assert_checks(&format!(
        "{TREE}fun touch(node: Ref<Tree>) do print(1) end\n\
             let payload: Tree = Tree.Branch(value: 1, left: leaf(), right: leaf())\n\
             let mut tree: Box<Tree> = box(payload)\n\
             if tree is Tree.Branch(branch) then\n\
             print(branch.value)\n\
             touch(branch.left)\n\
             touch(branch.right)\n\
             branch.value = 2\n\
             end\n\
             print(0)"
    ));
}

/// Specification 016 section 7.3: the binding is mutable only when the
/// tested place's root is. This is the same rule
/// `a_type_test_binding_is_an_immutable_root` already covers for an
/// unboxed union; this proves it still holds through a `Box<T>` subject.
#[test]
fn a_box_wrapped_union_binding_is_immutable_when_its_root_is_immutable() {
    assert_error_contains(
        &format!(
            "{TREE}let payload: Tree = Tree.Branch(value: 1, left: leaf(), right: leaf())\n\
                 let tree: Box<Tree> = box(payload)\n\
                 if tree is Tree.Branch(branch) then\n\
                 branch.value = 2\n\
                 end\n\
                 print(0)"
        ),
        "'branch' is not declared 'mut' and cannot be assigned",
    );
}

/// Specification 016 section 7.2's worked example: a `Box<T>` argument
/// place automatically lends its pointee to a `Ref<T>` parameter, and the
/// call may both read and mutate through the lent reference without
/// consuming the box -- `node` is still usable afterward.
#[test]
fn a_box_argument_automatically_lends_its_pointee_to_a_ref_parameter() {
    let program = assert_checks(&format!(
        "{NODE}fun increment(node: Ref<Node>) do node.value = node.value + 1 end\n\
             let mut node: Box<Node> = box(Node(value: 10))\n\
             increment(node)\n\
             print(node.value)"
    ));
    let args = top_level_args(&program);
    let TArg::Reference(place) = &args[0] else {
        panic!("a boxed argument lent to 'Ref<T>' should stay a reference argument");
    };
    assert_eq!(place.root, PlaceRoot::Local("node".into()));
    assert!(place.path.is_empty());
    // The lent place's type is the pointee, matching the parameter's
    // referent type exactly, not the argument's own `Box<Node>` type.
    assert_eq!(place.ty, program.funcs["increment"].params[0].ty);
}

/// Specification 016 section 7.2: a `Box<T>` argument may instead bind to
/// a declared `Ref<Box<T>>` parameter, borrowing the box itself rather
/// than lending its pointee -- the declared parameter type disambiguates
/// with no new inference, and this is just an exact-type match, so it
/// needs no special-casing beyond what already exists.
#[test]
fn a_box_argument_binds_to_a_declared_ref_of_box_parameter_instead_of_lending_its_pointee() {
    let program = assert_checks(&format!(
        "{NODE}fun replace(node: Ref<Box<Node>>) do node = box(Node(value: 0)) end\n\
             let mut node: Box<Node> = box(Node(value: 10))\n\
             replace(node)\n\
             print(node.value)"
    ));
    let args = top_level_args(&program);
    let TArg::Reference(place) = &args[0] else {
        panic!("a 'Ref<Box<T>>' parameter should still receive a reference argument");
    };
    assert!(matches!(place.ty, Ty::Box(_)));
    assert_eq!(place.ty, program.funcs["replace"].params[0].ty);
}

/// Specification 016 section 7.2: "mutation requires a mutable
/// originating root" applies to a lent pointee exactly as it already
/// does to an ordinary `Ref<T>` argument.
#[test]
fn lending_a_box_argument_still_requires_its_root_to_be_declared_mut() {
    assert_error_contains(
        &format!(
            "{NODE}fun increment(node: Ref<Node>) do node.value = node.value + 1 end\n\
                 let node: Box<Node> = box(Node(value: 10))\n\
                 increment(node)\n\
                 print(0)"
        ),
        "'node' is not declared 'mut', so it cannot be passed to the reference parameter",
    );
}

/// Specification 011 section 6.4, extended through boxes: lending the
/// same box's pointee to two parameters in the same call still overlaps,
/// exactly like passing the same plain place twice already did.
#[test]
fn passing_the_same_boxed_pointee_twice_in_one_call_is_rejected_as_overlapping() {
    assert_error_contains(
        &format!(
            "{NODE}fun swap_values(a: Ref<Node>, b: Ref<Node>) do print(1) end\n\
                 let mut node: Box<Node> = box(Node(value: 10))\n\
                 swap_values(node, node)\n\
                 print(0)"
        ),
        "reference arguments 'node' and 'node' overlap",
    );
}

/// Specification 016 section 7.2's closing sentence: a borrowed
/// allocation cannot also be moved out from under the same call.
#[test]
fn moving_a_boxed_argument_while_borrowing_its_pointee_in_the_same_call_is_rejected() {
    assert_error_contains(
        &format!(
            "{NODE}fun consume_and_peek(taken: Box<Node>, peek: Ref<Node>) do print(1) end\n\
                 let mut node: Box<Node> = box(Node(value: 10))\n\
                 consume_and_peek(node, node)\n\
                 print(0)"
        ),
        "overlaps the moved argument",
    );
}

// Specification 026: return statements. A small representative net, not
// the specification's full conformance matrix.

#[test]
fn bare_return_exits_a_no_result_function_early() {
    let program = assert_checks(
        "fun print_if_positive(value: Int64) do\n\
             if value <= 0 then\n\
             return\n\
             end\n\
             print(value)\n\
             end",
    );
    let TStmt::If(form) = &program.funcs["print_if_positive"].body.statements[0] else {
        panic!("expected an if statement");
    };
    assert!(matches!(
        form.arms[0].1.statements.as_slice(),
        [TStmt::Return { value: None, .. }]
    ));
}

#[test]
fn valued_return_exits_a_result_declaring_function_early() {
    let program = assert_checks(
        "fun absolute(value: Int64): Int64 do\n\
             if value < 0 then\n\
             return 0 - value\n\
             end\n\
             value\n\
             end",
    );
    let TStmt::If(form) = &program.funcs["absolute"].body.statements[0] else {
        panic!("expected an if statement");
    };
    assert!(matches!(
        form.arms[0].1.statements.as_slice(),
        [TStmt::Return { value: Some(_), .. }]
    ));
    // The trailing `value` still supplies the function's own result on
    // the path that never returns early.
    assert!(program.funcs["absolute"].body.result.is_some());
}

/// Section 6's `normalize` example: a returning branch supplies no value
/// of its own and does not participate in the value-form `if`'s common
/// result type, while the other branch still must.
#[test]
fn a_returning_if_branch_is_excluded_from_the_value_form_common_result_type() {
    let program = assert_checks(
        "fun normalize(value: Int64): Int64 do\n\
             if value < 0 then\n\
             return 0\n\
             else\n\
             value\n\
             end\n\
             end",
    );
    // Only the `else` branch reaches the callable end without a value of
    // its own, so this stays a value-form `if` (`TExpr::If`) in the
    // block's `result`, not a plain statement.
    let Some(TExpr::If(form)) = &program.funcs["normalize"].body.result else {
        panic!("expected a value-form if");
    };
    assert!(form.arms[0].1.result.is_none());
    assert!(form.else_branch.as_ref().unwrap().result.is_some());
}

/// Section 6's `bit` example: every reachable branch returning makes the
/// `if` itself a callable return that supplies no value, even though it
/// is the last element of a value-required body.
#[test]
fn an_if_whose_every_branch_returns_becomes_a_statement_with_no_result() {
    let program = assert_checks(
        "fun bit(flag: Bool): Int64 do\n\
             if flag then\n\
             return 1\n\
             else\n\
             return 0\n\
             end\n\
             end",
    );
    let body = &program.funcs["bit"].body;
    assert!(body.result.is_none());
    assert!(matches!(body.statements.as_slice(), [TStmt::If(_)]));
}

/// Specification 026 section 8: a value moved out through a conditional
/// early return is unavailable only on the path that took it -- the
/// fall-through path never moved it, so a later use on that path is
/// still valid. This exercises the `check_if` move-state fix this
/// specification requires: a returning arm's exit state must not be
/// merged into the state after the `if`.
#[test]
fn a_move_confined_to_a_returning_if_branch_leaves_the_fallthrough_path_available() {
    let program = assert_checks(
        "fun grab(held: Bool, value: Box<Int64>): Box<Int64> do\n\
             if held then\n\
             return value\n\
             end\n\
             value\n\
             end",
    );
    assert!(program.funcs["grab"].body.result.is_some());
}

#[test]
fn a_while_body_that_always_returns_still_leaves_the_loop_exit_reachable() {
    assert_checks(
        "fun first(): Int64 do\n\
             while true do\n\
             return 1\n\
             end\n\
             0\n\
             end",
    );
}

#[test]
fn rejects_return_outside_a_function_or_method() {
    assert_error_contains(
        "return",
        "'return' is only valid inside a function or method",
    );
    assert_error_contains(
        "return 1",
        "'return' is only valid inside a function or method",
    );
}

#[test]
fn rejects_a_bare_return_from_a_result_declaring_callable() {
    assert_error_contains(
        "fun f(): Int64 do return end",
        "declares a result of type 'Int64', so 'return' needs a value",
    );
}

#[test]
fn rejects_a_value_return_from_a_no_result_callable() {
    assert_error_contains(
        "fun f() do return 1 end",
        "declares no result, so 'return' cannot return a value",
    );
}

#[test]
fn rejects_a_returned_expression_not_assignable_to_the_declared_result() {
    assert_error_contains(
        "fun f(): Int64 do return true end",
        "expected 'Int64', found 'Bool'",
    );
}

/// Section 6's `incomplete` example: the path that does not return still
/// reaches the callable end without a value.
#[test]
fn rejects_a_result_declaring_path_that_reaches_the_end_without_returning() {
    assert_error_contains(
        "fun incomplete(flag: Bool): Int64 do\n\
             if flag then\n\
             return 1\n\
             end\n\
             end",
        "requires an 'else' branch",
    );
}

#[test]
fn rejects_source_after_an_unconditional_return() {
    assert_error_contains(
        "fun invalid(): Int64 do\n\
             return 1\n\
             print(2)\n\
             end",
        "unreachable",
    );
}

/// The same unreachable-source rule applies after an `if` for which
/// every reachable branch returns, not only after a bare `return`.
#[test]
fn rejects_source_after_an_if_whose_every_branch_returns() {
    assert_error_contains(
        "fun invalid(flag: Bool): Int64 do\n\
             if flag then\n\
             return 1\n\
             else\n\
             return 0\n\
             end\n\
             print(2)\n\
             end",
        "unreachable",
    );
}

#[test]
fn rejects_an_invalid_move_through_return() {
    assert_error_contains(
        "type Holder is struct value: Box<Int64>, end\n\
             fun take(holder: Holder): Box<Int64> do\n\
             return holder.value\n\
             end",
        "cannot be moved out of",
    );
}
