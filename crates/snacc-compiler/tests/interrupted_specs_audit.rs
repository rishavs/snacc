use snacc_compiler::{check, emit_llvm_ir};

fn error_text(source: &str) -> String {
    match check(source) {
        Ok(_) => panic!("source unexpectedly passed: {source}"),
        Err(diagnostics) => format!("{diagnostics:?}"),
    }
}

#[test]
fn call_boundary_view_lending_does_not_move_its_owner() {
    check(
        "fun byte_count(values: View<Byte>): Int64 do values.length() end\n\
         fun int_count(values: View<Int64>): Int64 do values.length() end\n\
         let text: String = \"hello\"\n\
         let values: List<Int64> = [1, 2, 3]\n\
         print(byte_count(text))\n\
         print(text)\n\
         print(int_count(values))\n\
         print(values.length())",
    )
    .expect("a call-boundary view must borrow rather than move its owner");
}

#[test]
fn call_boundary_view_cannot_overlap_a_move_of_the_same_owner() {
    let diagnostics = error_text(
        "fun consume(view: View<Byte>, text: String) do end\n\
         let text: String = \"hello\"\n\
         consume(text, text)",
    );
    assert!(diagnostics.contains("overlaps the moved argument"));
}

#[test]
fn collection_views_and_iteration_prevent_structural_mutation() {
    let view = error_text(
        "let mut values: List<Int64> = [1, 2]\n\
         let borrowed: View<Int64> = values.view()\n\
         values.push(3)\n\
         print(borrowed.length())",
    );
    assert!(view.contains("still borrows it"));

    let iteration = error_text(
        "let mut values: List<Int64> = [1, 2]\n\
         for value in values do\n\
             values.push(value)\n\
         end",
    );
    assert!(iteration.contains("still borrows it"));

    check(
        "let mut values: List<Int64> = [1, 2]\n\
         let borrowed: View<Int64> = values.view()\n\
         print(borrowed.length())\n\
         values.push(3)",
    )
    .expect("a collection borrow should end after its last reachable use");

    let clear = error_text(
        "let mut values: Set<Int64> = Set<Int64>()\n\
         values.clear(1)",
    );
    assert!(clear.contains("Set.clear expects no arguments"));
}

#[test]
fn view_provenance_survives_nested_assignment_and_control_flow() {
    let diagnostics = error_text(
        "let mut first: List<Int64> = [1]\n\
         let mut second: List<Int64> = [2]\n\
         let mut borrowed: View<Int64> = first.view()\n\
         if true then\n\
             borrowed = second.view()\n\
         end\n\
         second.push(3)\n\
         print(borrowed.length())",
    );
    assert!(diagnostics.contains("still borrows it"));

    let loop_move = error_text(
        "fun consume(text: String) do end\n\
         let text: String = \"once\"\n\
         let values: Array<Int64, 1> = [1]\n\
         for value in values do\n\
             consume(text)\n\
         end",
    );
    assert!(loop_move.contains("later iteration"));

    let temporary = error_text("let bytes: View<Byte> = \"short lived\".bytes()");
    assert!(temporary.contains("stored view requires a named owning source"));
}

#[test]
fn map_insert_consumes_a_move_only_value() {
    let diagnostics = error_text(
        "let mut values: Map<Int64, String> = Map<Int64, String>()\n\
         let text: String = \"owned\"\n\
         values.insert(1, text)\n\
         print(text)",
    );
    assert!(diagnostics.contains("already moved"));
}

#[test]
fn deferred_calls_reject_unavailable_exit_values() {
    let moved_later = error_text(
        "fun consume(text: String) do end\n\
         fun caller() do\n\
             let text: String = \"owned\"\n\
             defer consume(text)\n\
             consume(text)\n\
         end",
    );
    assert!(moved_later.contains("deferred call cannot use 'text'"));

    let moved_twice = error_text(
        "fun consume(text: String) do end\n\
         fun caller() do\n\
             let text: String = \"owned\"\n\
             defer consume(text)\n\
             defer consume(text)\n\
         end",
    );
    assert!(moved_twice.contains("deferred call cannot use 'text'"));

    let nested_move = error_text(
        "fun pass(text: String): String do text end\n\
         fun consume(text: String) do end\n\
         fun caller() do\n\
             let text: String = \"owned\"\n\
             defer consume(pass(text))\n\
             defer consume(pass(text))\n\
         end",
    );
    assert!(nested_move.contains("deferred call cannot use 'text'"));
}

#[test]
fn implicit_fallible_results_carry_error_sensitive_cleanup_facts() {
    let ir = emit_llvm_ir(
        "fun cleanup() do print(9) end\n\
         fun fail(): Int64 | Error do\n\
             defer_on_error cleanup()\n\
             Error(category: \"audit\", header: \"failure\", message: \"expected\")\n\
         end\n\
         print(0)",
    )
    .expect("implicit fallible return should lower");
    assert!(ir.contains("defer_error"));

    check(
        "fun cleanup(text: String) do end\n\
         fun choose(success: Bool): String | Error do\n\
             let text: String = \"owned\"\n\
             defer_on_error cleanup(text)\n\
             if success then\n\
                 return text\n\
             end\n\
             Error(category: \"audit\", header: \"failure\", message: \"expected\")\n\
         end\n\
         print(0)",
    )
    .expect("an error-only defer need not retain its arguments on a proven success exit");
}

#[test]
fn every_builtin_inline_sum_member_can_be_type_tested() {
    check(
        "let text: String | Nil = \"text\"\n\
         if text is String(value) then print(value) end\n\
         let scalar: Unicode | Nil = 'x'\n\
         if scalar is Unicode(scalar_value) then print(scalar_value) end",
    )
    .expect("String and Unicode are built-in sum members, not user type names");
}

#[test]
fn interpolation_uses_the_ordinary_numeric_and_raw_literal_rules() {
    check(
        "let exponent: String = \"value={{1e-2}}\"\n\
         let binary: String = \"value={{0b1010u16}}\"",
    )
    .expect("literals inside interpolation should use the ordinary literal contract");
    check(r##"let raw: String = "value={{r#"raw\text"#}}""##)
        .expect("a raw string should remain one expression token inside interpolation");
    check(r#"let nested: String = "outer={{"inner={{1}}"}}""#)
        .expect("an interpreted string inside interpolation may itself interpolate");

    for source in [
        "let bad: String = \"{{1E2}}\"",
        "let bad: String = \"{{1__2}}\"",
        "let bad: String = \"{{0b102}}\"",
    ] {
        assert!(
            check(source).is_err(),
            "invalid interpolation literal unexpectedly passed: {source}"
        );
    }
}

#[test]
fn unused_generic_declarations_are_checked_before_instantiation() {
    for source in [
        "fun invalid<T>(): Bool do 1 + true end\nprint(0)",
        "fun invalid<T>() do missing() end\nprint(0)",
    ] {
        assert!(
            check(source).is_err(),
            "an invalid unused generic declaration unexpectedly passed: {source}"
        );
    }
}
