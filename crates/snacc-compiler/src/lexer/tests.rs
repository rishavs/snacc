use super::*;

fn lex(source: &str) -> Vec<Token<'_>> {
    let (tokens, errors) = lexer().parse(source).into_output_errors();
    assert!(errors.is_empty(), "lex errors: {errors:?}");
    tokens
        .unwrap()
        .into_iter()
        .map(|(token, _span)| token)
        .collect()
}

/// Lexes a source that is expected to fail, returning one message and span
/// per error.
fn lex_failing(source: &str) -> Vec<(String, Span)> {
    let (_, errors) = lexer().parse(source).into_output_errors();
    errors
        .iter()
        .map(|error| (error.to_string(), *error.span()))
        .collect()
}

fn num(source: &str) -> NumLiteral {
    match lex(source).as_slice() {
        [Token::Num(literal)] => *literal,
        other => panic!("expected one numeric token for {source}, got: {other:?}"),
    }
}

// Specification 009 section 4.2-4.3: literal forms, ranges, and munching.

#[test]
fn lexes_zero_and_the_maximum_for_every_unsigned_width() {
    assert_eq!(num("0u8"), NumLiteral::U8(0));
    assert_eq!(num("255u8"), NumLiteral::U8(u8::MAX));
    assert_eq!(num("0u16"), NumLiteral::U16(0));
    assert_eq!(num("65535u16"), NumLiteral::U16(u16::MAX));
    assert_eq!(num("0u32"), NumLiteral::U32(0));
    assert_eq!(num("4294967295u32"), NumLiteral::U32(u32::MAX));
    assert_eq!(num("0u64"), NumLiteral::U64(0));
    assert_eq!(num("18446744073709551615u64"), NumLiteral::U64(u64::MAX));
}

#[test]
fn rejects_one_above_the_maximum_for_every_unsigned_width() {
    for (source, ty) in [
        ("256u8", "Byte"),
        ("65536u16", "UInt16"),
        ("4294967296u32", "UInt32"),
        ("18446744073709551616u64", "UInt64"),
    ] {
        let errors = lex_failing(source);
        assert!(
            errors
                .iter()
                .any(|(message, _)| message.contains(source) && message.contains(ty)),
            "expected an out-of-range error naming {source} and {ty}, got: {errors:?}"
        );
    }
}

#[test]
fn lexes_float32_with_and_without_a_fractional_part() {
    assert_eq!(num("0f32"), NumLiteral::F32(0.0));
    assert_eq!(num("1f32"), NumLiteral::F32(1.0));
    assert_eq!(num("1.5f32"), NumLiteral::F32(1.5));
}

#[test]
fn a_float32_literal_is_rounded_to_the_nearest_binary32_value() {
    assert_eq!(
        num("0.1f32"),
        NumLiteral::F32("0.1".parse::<f32>().expect("0.1 parses as f32")),
        "0.1 has no exact binary32 value and rounds to nearest"
    );
    assert_eq!(
        num("16777217f32"),
        NumLiteral::F32(16_777_216.0),
        "2^24 + 1 has no binary32 value and rounds to 2^24"
    );
}

#[test]
fn rejects_a_float32_literal_that_rounds_to_infinity() {
    let source = "999999999999999999999999999999999999999999f32";
    let errors = lex_failing(source);
    assert!(
        errors
            .iter()
            .any(|(message, _)| message.contains("Float32") && message.contains("out of range")),
        "expected a Float32 range error, got: {errors:?}"
    );
}

#[test]
fn a_malformed_suffix_is_one_invalid_token_not_a_number_and_an_identifier() {
    // Specification 009 section 4.2: maximal munch. Each of these is one
    // bad token, so each yields exactly one error whose span covers the
    // whole text -- not a valid number plus a separate identifier.
    for source in ["1u9", "1u8x", "1f64", "1.0u8", "1u8_", "12.5f64"] {
        let errors = lex_failing(source);
        assert_eq!(
            errors.len(),
            1,
            "expected exactly one error for {source}, got: {errors:?}"
        );
        let (message, span) = &errors[0];
        assert!(
            message.contains(source),
            "error for {source} should name the complete token, got: {message}"
        );
        assert_eq!(
            (span.start, span.end),
            (0, source.len()),
            "the error for {source} should span the whole token"
        );
    }
}

// Specification 020: radix literals, scientific notation, separators, and
// their required diagnostics. Native conformance tests cover the same
// forms through the complete compiler pipeline.

#[test]
fn lexes_binary_octal_and_hexadecimal_integers() {
    assert_eq!(num("0b101010"), NumLiteral::Int(42));
    assert_eq!(num("0o755"), NumLiteral::Int(493));
    assert_eq!(num("0x2A"), NumLiteral::Int(42));
    assert_eq!(
        num("0xff"),
        NumLiteral::Int(255),
        "hex digits may be lowercase"
    );
    assert_eq!(num("0xFFu8"), NumLiteral::U8(255));
    assert_eq!(num("0b10100101u8"), NumLiteral::U8(0b1010_0101));
    assert_eq!(
        num("0xFFFF_FFFF_FFFF_FFFFu64"),
        NumLiteral::U64(u64::MAX),
        "the suffix selects the exact unsigned type in every radix"
    );
}

#[test]
fn separators_are_discarded_without_changing_the_value() {
    assert_eq!(num("1_000"), NumLiteral::Int(1000));
    assert_eq!(num("0xFF_FFu16"), NumLiteral::U16(0xFFFF));
    assert_eq!(num("1_2.3_4_5e1_0"), NumLiteral::F64(12.345e10));
}

#[test]
fn lexes_scientific_notation_at_both_float_widths() {
    assert_eq!(num("1e6"), NumLiteral::F64(1e6));
    assert_eq!(num("1.25e-3"), NumLiteral::F64(1.25e-3));
    assert_eq!(num("1e+9"), NumLiteral::F64(1e9));
    assert_eq!(num("6.022e23f32"), NumLiteral::F32(6.022e23f32));
    assert_eq!(num("1e0f32"), NumLiteral::F32(1.0));
}

#[test]
fn a_hexadecimal_letter_digit_does_not_start_a_spurious_exponent() {
    // `0xE` is the hex digit E (14), not an exponent marker, so a
    // following operator lexes on its own -- Specification 020 section 5.
    assert_eq!(
        lex("0xE+5"),
        vec![
            Token::Num(NumLiteral::Int(14)),
            Token::Op("+"),
            Token::Num(NumLiteral::Int(5)),
        ]
    );
}

#[test]
fn rejects_every_required_section_11_diagnostic_kind() {
    for source in [
        "0b",     // missing digit after a radix prefix
        "0b102",  // digit invalid for the selected radix
        "0o89",   // digit invalid for the selected radix
        "0xGG",   // digit invalid for the selected radix
        "0B10",   // uppercase prefix
        "0XFF",   // uppercase prefix
        "1E6",    // uppercase exponent marker
        "1e",     // missing exponent digit
        "1e+",    // missing exponent digit
        "1e-f32", // missing exponent digit
        "0b1e10", // exponent on a non-decimal integer
        "0x1.8",  // fractional point on a non-decimal literal
        "1.f32",  // missing digit after a decimal point
        "1u8f32", // incompatible suffixes
        "1e3u32", // unsigned suffix on a floating-point literal
        "1__000", // doubled separator
        "1_",     // trailing separator
        "0x_FF",  // separator adjacent to a prefix
        "1_.0",   // separator adjacent to a decimal point
        "1e_3",   // separator adjacent to an exponent marker
        "1e+_3",  // separator adjacent to an exponent sign
        "1_f32",  // separator adjacent to a suffix
        "256u8",  // magnitude out of range for its selected type
    ] {
        let errors = lex_failing(source);
        assert_eq!(
            errors.len(),
            1,
            "expected exactly one diagnostic for {source}, got: {errors:?}"
        );
    }
}

#[test]
fn a_decimal_literal_rounding_to_infinity_is_rejected_at_both_widths() {
    let huge = "1".repeat(400);
    for source in [format!("{huge}.0"), format!("{huge}f32")] {
        let errors = lex_failing(&source);
        assert!(
            errors.iter().any(
                |(message, _)| message.contains("out of range") && message.contains("infinity")
            ),
            "expected an infinity range error for a huge literal, got: {errors:?}"
        );
    }
}

/// Specification 020 section 8: `null` is no longer the `Nil` literal; it
/// lexes as an ordinary identifier, exactly like any other undeclared name.
#[test]
fn null_is_an_ordinary_identifier_not_the_nil_literal() {
    assert_eq!(lex("null"), vec![Token::Ident("null")]);
    assert_eq!(
        lex("let null: Int64 = 10"),
        vec![
            Token::Let,
            Token::Ident("null"),
            Token::Ctrl(':'),
            Token::TyInt64,
            Token::Op("="),
            Token::Num(NumLiteral::Int(10)),
        ]
    );
}

#[test]
fn suffix_spellings_stay_ordinary_identifiers_away_from_digits() {
    assert_eq!(lex("u8"), vec![Token::Ident("u8")]);
    assert_eq!(lex("f32"), vec![Token::Ident("f32")]);
    assert_eq!(
        lex("1 u8"),
        vec![Token::Num(NumLiteral::Int(1)), Token::Ident("u8")]
    );
}

#[test]
fn the_new_type_names_are_reserved_words() {
    assert_eq!(
        lex("UInt8 UInt16 UInt32 UInt64 Float32"),
        vec![
            Token::RemovedUInt8,
            Token::TyUInt16,
            Token::TyUInt32,
            Token::TyUInt64,
            Token::TyFloat32,
        ]
    );
}

#[test]
fn unsuffixed_literals_keep_their_existing_types() {
    assert_eq!(num("7"), NumLiteral::Int(7));
    assert_eq!(num("7.5"), NumLiteral::F64(7.5));
    assert_eq!(
        num("9223372036854775807"),
        NumLiteral::Int(i64::MAX),
        "Int64 literals still arrive exactly, never through f64"
    );
}

#[test]
fn raw_strings_preserve_backslashes_quotes_and_hash_delimiters() {
    assert_eq!(
        lex(r##"r"C:\snacc\examples" r#"She said "hello"."#"##),
        vec![
            Token::RawStr(r"C:\snacc\examples"),
            Token::RawStr(r##"She said "hello"."##),
        ]
    );
}

#[test]
fn raw_strings_may_span_lines() {
    assert_eq!(
        lex("r#\"first\nsecond\\n\"#"),
        vec![Token::RawStr("first\nsecond\\n")]
    );
}

#[test]
fn interpolation_expression_tokens_keep_their_source_spans() {
    let source = r#"let message: String = "hello {{name}}""#;
    let (tokens, errors) = lexer().parse(source).into_output_errors();
    assert!(errors.is_empty(), "lex errors: {errors:?}");
    let tokens = tokens.expect("lexer should produce tokens");
    let (_, span) = tokens
        .iter()
        .find(|(token, _)| matches!(token, Token::Interpolated(_)))
        .expect("interpolated literal should be present");
    let Token::Interpolated(parts) = &tokens
        .iter()
        .find(|(token, _)| matches!(token, Token::Interpolated(_)))
        .expect("interpolated literal should be present")
        .0
    else {
        unreachable!("the matching token is interpolated");
    };
    let InterpolatedPart::Expression(parts) = parts.last().expect("expression part") else {
        panic!("expected an interpolation expression");
    };
    let (_, name_span) = parts.first().expect("name token");
    let name_start = source.find("name").expect("name in source");
    assert_eq!(
        (name_span.start, name_span.end),
        (name_start, name_start + 4)
    );
    assert!(span.start < name_span.start && name_span.end <= span.end);
}

#[test]
fn break_lexes_to_its_own_token() {
    assert_eq!(lex("break"), vec![Token::Break]);
}

/// Specification 026 section 4: `return` is reserved regardless of
/// context, mirroring `break`'s existing reservation.
#[test]
fn return_lexes_to_its_own_token_and_is_never_an_identifier() {
    assert_eq!(
        lex("fun f() do return end"),
        vec![
            Token::Fun,
            Token::Ident("f"),
            Token::Ctrl('('),
            Token::Ctrl(')'),
            Token::Do,
            Token::Return,
            Token::End,
        ]
    );
}

#[test]
fn break_is_unavailable_as_an_identifier_regardless_of_context() {
    // `break` must never surface as Token::Ident("break"), matching how
    // `while`/`if`/etc. are reserved regardless of surrounding context.
    assert_eq!(
        lex("while break do break end"),
        vec![
            Token::While,
            Token::Break,
            Token::Do,
            Token::Break,
            Token::End,
        ]
    );
}

/// Specification 010 section 4: every new keyword is reserved, and `.`/`|`
/// lex as their own control characters.
#[test]
fn the_nominal_type_keywords_are_reserved_words() {
    assert_eq!(
        lex("type is struct union method static self mut"),
        vec![
            Token::Type,
            Token::Is,
            Token::Struct,
            Token::Union,
            Token::Method,
            Token::Static,
            Token::SelfKw,
            Token::Mut,
        ]
    );
}

/// Specification 011 section 4: `Ref` is reserved, and the type brackets are
/// the ordinary comparison operator tokens -- never munched into `>>`.
#[test]
fn ref_is_reserved_and_type_brackets_lex_one_at_a_time() {
    assert_eq!(
        lex("Ref<Ref<Int64>>"),
        vec![
            Token::Ref,
            Token::Op("<"),
            Token::Ref,
            Token::Op("<"),
            Token::TyInt64,
            Token::Op(">"),
            Token::Op(">"),
        ]
    );
}

/// Specification 016 section 4.1: `Box` is reserved and the type brackets
/// lex one at a time, exactly like `Ref<T>`.
#[test]
fn box_is_reserved_and_type_brackets_lex_one_at_a_time() {
    assert_eq!(
        lex("Box<Box<Int64>>"),
        vec![
            Token::Box,
            Token::Op("<"),
            Token::Box,
            Token::Op("<"),
            Token::TyInt64,
            Token::Op(">"),
            Token::Op(">"),
        ]
    );
}

/// Specification 016 section 4.2: `box` (the allocation expression) is a
/// distinct, separately reserved word from `Box` (the type); the lexer
/// tells them apart purely by case, the same way it already tells `Nil`
/// (the type) apart from `nil` (the literal).
#[test]
fn box_expression_keyword_is_reserved_and_distinct_from_the_box_type() {
    assert_eq!(lex("Box box"), vec![Token::Box, Token::BoxExpr]);
    assert_eq!(
        lex("box(1)"),
        vec![
            Token::BoxExpr,
            Token::Ctrl('('),
            Token::Num(NumLiteral::Int(1)),
            Token::Ctrl(')'),
        ]
    );
}

#[test]
fn two_character_comparisons_still_lex_as_one_token() {
    assert_eq!(
        lex("a == b != c <= d >= e"),
        vec![
            Token::Ident("a"),
            Token::Op("=="),
            Token::Ident("b"),
            Token::Op("!="),
            Token::Ident("c"),
            Token::Op("<="),
            Token::Ident("d"),
            Token::Op(">="),
            Token::Ident("e"),
        ]
    );
}

#[test]
fn member_selection_and_union_bars_lex_as_control_characters() {
    assert_eq!(
        lex("a.b | c"),
        vec![
            Token::Ident("a"),
            Token::Ctrl('.'),
            Token::Ident("b"),
            Token::Ctrl('|'),
            Token::Ident("c"),
        ]
    );
}

#[test]
fn a_decimal_literal_still_munches_its_own_point() {
    assert_eq!(lex("1.5"), vec![Token::Num(NumLiteral::F64(1.5))]);
}

#[test]
fn semicolon_is_a_lex_error() {
    let (_, errors) = lexer().parse("let x: Int64 = 1;").into_output_errors();
    assert!(
        !errors.is_empty(),
        "expected a lex error for a bare semicolon"
    );
    // The error must clearly name the offending character (not just fail
    // silently) so it's diagnosable as "no semicolon syntax" at this span.
    assert!(
        errors.iter().any(|e| e.to_string().contains("';'")),
        "expected the error to name the semicolon, got: {errors:?}"
    );
}

#[test]
fn semicolon_is_a_lex_error_via_parse_entrypoint() {
    let diagnostics = crate::parse("let x: Int64 = 1;")
        .err()
        .expect("snacc has no semicolon syntax; `crate::parse` should report a diagnostic");
    let diagnostic = diagnostics
        .first()
        .expect("expected at least one diagnostic");
    assert_eq!(diagnostic.phase, crate::DiagnosticPhase::Lex);
}
