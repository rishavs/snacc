use crate::ast::{NumLiteral, Span, Spanned};
use chumsky::prelude::*;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Token<'src> {
    Bool(bool),
    Num(NumLiteral),
    Str(&'src str),
    RawStr(&'src str),
    Interpolated(Vec<InterpolatedPart<'src>>),
    Unicode(u32),
    Op(&'src str),
    Ctrl(char),
    Ident(&'src str),
    Extern,
    Rust,
    Fun,
    Let,
    Mut,
    Print,
    If,
    Then,
    Do,
    Type,
    Is,
    Struct,
    Union,
    Method,
    Static,
    SelfKw,
    /// `Ref`, reserved by Specification 011 section 4 for the reference
    /// parameter type. It is never an ordinary identifier.
    Ref,
    /// `Box`, reserved by Specification 016 section 4.1 for the boxed
    /// indirection type. Cased distinctly from [`Token::BoxExpr`]'s `box`, the
    /// same way every other built-in type name is capitalized while ordinary
    /// keywords are not.
    Box,
    /// `box`, reserved by Specification 016 section 4.2 for the allocation
    /// expression `box(expression)`. Never an ordinary identifier, and never
    /// confused with [`Token::Box`]: the lexer matches on the exact spelling.
    BoxExpr,
    TyFloat64,
    TyInt64,
    TyBool,
    TyNil,
    TyString,
    TyUnicode,
    TyByte,
    TyView,
    TyArray,
    TyList,
    TyMap,
    TySet,
    /// The former source spelling is deliberately tokenized separately so
    /// the parser can reject it instead of treating it as an ordinary type or
    /// user-defined name. The numeric `u8` suffix remains supported.
    RemovedUInt8,
    TyUInt16,
    TyUInt32,
    TyUInt64,
    TyFloat32,
    Nil,
    While,
    For,
    In,
    ElseIf,
    Else,
    End,
    Break,
    /// `return`, reserved by Specification 026 section 4 for the return
    /// statement. Never an ordinary identifier.
    Return,
    And,
    Or,
    ReturnOnError,
    Defer,
    DeferOnError,
}

/// Structured contents of an interpreted string. Expression parts retain
/// their already-tokenized source, while literal parts carry their decoded
/// UTF-8 text.
#[derive(Clone, Debug, PartialEq)]
pub enum InterpolatedPart<'src> {
    Literal(String),
    Expression(Vec<Spanned<Token<'src>>>),
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Token::Bool(x) => write!(f, "{x}"),
            Token::Num(n) => write!(f, "{n}"),
            Token::Str(s) => write!(f, "{s}"),
            Token::RawStr(s) => write!(f, "r{s}"),
            Token::Interpolated(_) => f.write_str("interpolated string"),
            Token::Unicode(s) => write!(f, "U+{s:04X}"),
            Token::Op(s) => write!(f, "{s}"),
            Token::Ctrl(c) => write!(f, "{c}"),
            Token::Ident(s) => write!(f, "{s}"),
            Token::Extern => write!(f, "extern"),
            Token::Rust => write!(f, "rust"),
            Token::Fun => write!(f, "fun"),
            Token::Let => write!(f, "let"),
            Token::Mut => write!(f, "mut"),
            Token::Print => write!(f, "print"),
            Token::If => write!(f, "if"),
            Token::Then => write!(f, "then"),
            Token::Do => write!(f, "do"),
            Token::Type => write!(f, "type"),
            Token::Is => write!(f, "is"),
            Token::Struct => write!(f, "struct"),
            Token::Union => write!(f, "union"),
            Token::Method => write!(f, "method"),
            Token::Static => write!(f, "static"),
            Token::SelfKw => write!(f, "self"),
            Token::Ref => write!(f, "Ref"),
            Token::Box => write!(f, "Box"),
            Token::BoxExpr => write!(f, "box"),
            Token::TyFloat64 => write!(f, "Float64"),
            Token::TyInt64 => write!(f, "Int64"),
            Token::TyBool => write!(f, "Bool"),
            Token::TyNil => write!(f, "Nil"),
            Token::TyString => write!(f, "String"),
            Token::TyUnicode => write!(f, "Unicode"),
            Token::TyByte => write!(f, "Byte"),
            Token::TyView => write!(f, "View"),
            Token::TyArray => write!(f, "Array"),
            Token::TyList => write!(f, "List"),
            Token::TyMap => write!(f, "Map"),
            Token::TySet => write!(f, "Set"),
            Token::RemovedUInt8 => write!(f, "UInt8"),
            Token::TyUInt16 => write!(f, "UInt16"),
            Token::TyUInt32 => write!(f, "UInt32"),
            Token::TyUInt64 => write!(f, "UInt64"),
            Token::TyFloat32 => write!(f, "Float32"),
            Token::Nil => write!(f, "nil"),
            Token::While => write!(f, "while"),
            Token::For => write!(f, "for"),
            Token::In => write!(f, "in"),
            Token::ElseIf => write!(f, "elseif"),
            Token::Else => write!(f, "else"),
            Token::End => write!(f, "end"),
            Token::Break => write!(f, "break"),
            Token::Return => write!(f, "return"),
            Token::And => write!(f, "and"),
            Token::Or => write!(f, "or"),
            Token::ReturnOnError => write!(f, "return_on_error"),
            Token::Defer => write!(f, "defer"),
            Token::DeferOnError => write!(f, "defer_on_error"),
        }
    }
}

/// Every suffix a numeric literal may carry, named when one is rejected.
const NUMERIC_SUFFIXES: &str = "'u8', 'u16', 'u32', 'u64', and 'f32'";

fn out_of_range(full: &str, ty: &str) -> String {
    format!("numeric literal '{full}' is out of range for {ty}")
}

/// Decodes the escape language shared by interpreted strings and Unicode
/// literals. Keeping this here makes malformed escapes lexer-owned diagnostics
/// while allowing the parser to retain borrowed token text for link symbols.
pub fn decode_string_content(source: &str) -> Result<String, String> {
    let mut output = String::new();
    let mut chars = source.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let Some(escape) = chars.next() else {
            return Err("string literal ends with an incomplete escape".into());
        };
        match escape {
            '0' => output.push('\0'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            '\\' => output.push('\\'),
            '"' => output.push('"'),
            '{' => output.push('{'),
            '}' => output.push('}'),
            'u' => {
                if chars.next() != Some('{') {
                    return Err("Unicode escape must use the form \\u{H...}".into());
                }
                let mut digits = String::new();
                loop {
                    let Some(digit) = chars.next() else {
                        return Err("Unicode escape is missing its closing '}'".into());
                    };
                    if digit == '}' {
                        break;
                    }
                    if !digit.is_ascii_hexdigit() || digits.len() == 6 {
                        return Err("Unicode escape requires one to six hexadecimal digits".into());
                    }
                    digits.push(digit);
                }
                if digits.is_empty() {
                    return Err("Unicode escape requires at least one hexadecimal digit".into());
                }
                let scalar = u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| "Unicode escape is not a valid Unicode scalar".to_string())?;
                output.push(scalar);
            }
            other => return Err(format!("unknown string escape '\\{other}'")),
        }
    }
    Ok(output)
}

pub fn decode_unicode_content(source: &str) -> Result<u32, String> {
    let decoded = decode_string_content(source)?;
    let mut chars = decoded.chars();
    let Some(value) = chars.next() else {
        return Err("Unicode literal cannot be empty".into());
    };
    if chars.next().is_some() {
        return Err("Unicode literal must contain exactly one Unicode scalar".into());
    }
    Ok(value as u32)
}

pub fn normalize_line_endings(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\r', "\n")
}

/// One `integer-literal`'s magnitude (Specification 020 section 4): either
/// plain `decimal-digits`, or a lowercase `0b`/`0o`/`0x` prefix (captured
/// verbatim, including a rejected uppercase spelling) with its digit run.
/// `None` digits means the lexer found no valid digit for that radix
/// immediately after the prefix -- still distinct from an empty prefix, so
/// `classify_number` can tell "missing digit after prefix" apart from
/// "invalid digit for radix" using what (if anything) `trailing` captured.
#[derive(Clone, Copy, Debug)]
enum RawMagnitude<'src> {
    Decimal(&'src str),
    Radix(&'src str, Option<&'src str>),
}

/// `[ "_" ], digit` run, captured raw (separators still embedded). Chumsky's
/// `repeated()` backtracks a whole failed repetition -- an underscore not
/// followed by another digit of the same radix -- so a leading, trailing,
/// doubled, or prefix/point/marker-adjacent separator (Specification 020
/// section 4/7) is never silently absorbed here: it is left for the trailing
/// catch-all in [`lexer`], which `classify_number` reports as misplaced.
fn digit_run<'src>(
    chars: &'static str,
) -> impl Parser<'src, &'src str, &'src str, extra::Err<Rich<'src, char, Span>>> + Clone {
    one_of(chars)
        .then(just('_').or_not().then(one_of(chars)).repeated())
        .to_slice()
}

/// Discards separators from one already digit-validated run (every character
/// chumsky's [`digit_run`] admits into `run` besides `_` is already a valid
/// digit for its radix), rejecting a leading, trailing, or doubled `_`.
fn strip_separators(run: &str, full: &str) -> Result<String, String> {
    if run.starts_with('_') || run.ends_with('_') || run.contains("__") {
        return Err(format!(
            "numeric literal '{full}' has a misplaced '_' separator; '_' may \
             appear only between two digits of the same component"
        ));
    }
    Ok(run.chars().filter(|&c| c != '_').collect())
}

/// Classifies one maximally-munched numeric token (Specification 020 section
/// 4-7) from its already-boundary-found pieces: `magnitude` (radix and
/// digits), an optional `.`-`fraction` (`Some(None)` means the point was seen
/// with no digit after it), an optional exponent (uppercase flag, sign,
/// digits), and the trailing alphanumeric/`_` run left over -- a suffix, or
/// invalid trailing content that keeps the diagnostic on the whole token
/// instead of splitting it into a valid number and a separate identifier.
fn classify_number(
    full: &str,
    magnitude: RawMagnitude<'_>,
    fraction: Option<Option<&str>>,
    exponent: Option<(bool, Option<char>, Option<&str>)>,
    trailing: &str,
) -> Result<NumLiteral, String> {
    let (radix, radix_name, prefix) = match magnitude {
        RawMagnitude::Decimal(_) => (10u32, "decimal", ""),
        RawMagnitude::Radix(prefix, _) => {
            if prefix.as_bytes()[1].is_ascii_uppercase() {
                return Err(format!(
                    "numeric literal '{full}' uses an uppercase radix prefix; \
                     use lowercase '0b', '0o', or '0x'"
                ));
            }
            match prefix.as_bytes()[1] {
                b'b' => (2, "binary", prefix),
                b'o' => (8, "octal", prefix),
                b'x' => (16, "hexadecimal", prefix),
                _ => unreachable!("only 'b'/'o'/'x' prefixes are matched"),
            }
        }
    };

    if let Some((true, ..)) = exponent {
        return Err(format!(
            "numeric literal '{full}' uses an uppercase exponent marker; use lowercase 'e'"
        ));
    }
    if radix != 10 {
        if fraction.is_some() {
            return Err(format!(
                "numeric literal '{full}' has a decimal point, but a {radix_name} \
                 literal does not support a fractional part"
            ));
        }
        if exponent.is_some() {
            return Err(format!(
                "numeric literal '{full}' has an exponent, but a {radix_name} \
                 literal does not support scientific notation"
            ));
        }
    }

    let magnitude_digits = match magnitude {
        RawMagnitude::Decimal(digits) => digits,
        RawMagnitude::Radix(_, Some(digits)) => digits,
        RawMagnitude::Radix(_, None) => {
            return Err(if trailing.is_empty() {
                format!("numeric literal '{full}' is missing digits after its '{prefix}' prefix")
            } else if trailing.starts_with('_') {
                format!(
                    "numeric literal '{full}' has a '_' separator immediately after its '{prefix}' prefix"
                )
            } else {
                format!("numeric literal '{full}' has a digit invalid for its {radix_name} prefix")
            });
        }
    };
    let magnitude_clean = strip_separators(magnitude_digits, full)?;

    let fraction_clean = match fraction {
        None => None,
        Some(None) => {
            return Err(format!(
                "numeric literal '{full}' is missing a digit after its decimal point"
            ));
        }
        Some(Some(digits)) => Some(strip_separators(digits, full)?),
    };
    let (exponent_sign, exponent_clean) = match exponent {
        None => (None, None),
        Some((_, _, None)) => {
            return Err(format!(
                "numeric literal '{full}' is missing a digit in its exponent"
            ));
        }
        Some((_, sign, Some(digits))) => (sign, Some(strip_separators(digits, full)?)),
    };
    let has_float_shape = fraction_clean.is_some() || exponent_clean.is_some();

    // A decimal source value is rebuilt without separators and converted
    // exactly once to its target IEEE width (Specification 020 section 6);
    // `Float32` never passes through `f64` first.
    let decimal_text = || {
        let mut text = magnitude_clean.clone();
        if let Some(frac) = &fraction_clean {
            text.push('.');
            text.push_str(frac);
        }
        if let Some(exp) = &exponent_clean {
            text.push('e');
            if let Some(sign) = exponent_sign {
                text.push(sign);
            }
            text.push_str(exp);
        }
        text
    };

    match trailing {
        "f32" if radix != 10 => Err(format!(
            "numeric literal '{full}' has an 'f32' suffix, which requires a decimal literal"
        )),
        "f32" => {
            let value: f32 = decimal_text()
                .parse()
                .expect("reconstructed decimal text is always valid float syntax");
            if value.is_finite() {
                Ok(NumLiteral::F32(value))
            } else {
                Err(format!(
                    "numeric literal '{full}' is out of range for Float32; it rounds to infinity"
                ))
            }
        }
        "" if has_float_shape => {
            let value: f64 = decimal_text()
                .parse()
                .expect("reconstructed decimal text is always valid float syntax");
            if value.is_finite() {
                Ok(NumLiteral::F64(value))
            } else {
                Err(format!(
                    "numeric literal '{full}' is out of range for Float64; it rounds to infinity"
                ))
            }
        }
        "" => {
            let raw = u64::from_str_radix(&magnitude_clean, radix)
                .map_err(|_| out_of_range(full, "Int64"))?;
            if raw > i64::MAX as u64 {
                return Err(out_of_range(full, "Int64"));
            }
            Ok(NumLiteral::Int(raw as i64))
        }
        "u8" | "u16" | "u32" | "u64" if has_float_shape => Err(format!(
            "numeric literal '{full}' has the unsigned suffix '{trailing}', but its decimal \
             point or exponent makes it a floating-point literal, not an integer literal"
        )),
        "u8" => u64::from_str_radix(&magnitude_clean, radix)
            .ok()
            .and_then(|value| u8::try_from(value).ok())
            .map(NumLiteral::U8)
            .ok_or_else(|| out_of_range(full, "Byte")),
        "u16" => u64::from_str_radix(&magnitude_clean, radix)
            .ok()
            .and_then(|value| u16::try_from(value).ok())
            .map(NumLiteral::U16)
            .ok_or_else(|| out_of_range(full, "UInt16")),
        "u32" => u64::from_str_radix(&magnitude_clean, radix)
            .ok()
            .and_then(|value| u32::try_from(value).ok())
            .map(NumLiteral::U32)
            .ok_or_else(|| out_of_range(full, "UInt32")),
        "u64" => u64::from_str_radix(&magnitude_clean, radix)
            .map(NumLiteral::U64)
            .map_err(|_| out_of_range(full, "UInt64")),
        _ => Err(format!(
            "numeric literal '{full}' has an unknown suffix '{trailing}'; \
             supported suffixes are {NUMERIC_SUFFIXES}"
        )),
    }
}

/// Applies the ordinary numeric classifier to a complete token discovered
/// inside interpolation. The interpolation scanner cannot recursively embed
/// the opaque Chumsky lexer type, so it reproduces only the token boundary;
/// all validation and conversion still converge here.
fn classify_fragment_number(full: &str) -> Result<NumLiteral, String> {
    let bytes = full.as_bytes();
    let mut position = 0usize;
    let (magnitude, radix) = if bytes.len() >= 2
        && bytes[0] == b'0'
        && matches!(bytes[1], b'b' | b'B' | b'o' | b'O' | b'x' | b'X')
    {
        let prefix = &full[..2];
        position = 2;
        let radix = match bytes[1].to_ascii_lowercase() {
            b'b' => 2,
            b'o' => 8,
            b'x' => 16,
            _ => unreachable!("guarded radix prefix"),
        };
        let start = position;
        while position < bytes.len()
            && (bytes[position] == b'_' || (bytes[position] as char).is_digit(radix))
        {
            position += 1;
        }
        (
            RawMagnitude::Radix(prefix, (position > start).then_some(&full[start..position])),
            radix,
        )
    } else {
        while position < bytes.len()
            && (bytes[position].is_ascii_digit() || bytes[position] == b'_')
        {
            position += 1;
        }
        (RawMagnitude::Decimal(&full[..position]), 10)
    };
    let fraction = if position < bytes.len() && bytes[position] == b'.' {
        position += 1;
        let start = position;
        while position < bytes.len()
            && (bytes[position].is_ascii_digit() || bytes[position] == b'_')
        {
            position += 1;
        }
        Some((position > start).then_some(&full[start..position]))
    } else {
        None
    };
    let exponent = if position < bytes.len()
        && matches!(bytes[position], b'e' | b'E')
        && !(radix == 16 && (bytes[position] as char).is_ascii_hexdigit())
    {
        let uppercase = bytes[position] == b'E';
        position += 1;
        let sign = if position < bytes.len() && matches!(bytes[position], b'+' | b'-') {
            let sign = bytes[position] as char;
            position += 1;
            Some(sign)
        } else {
            None
        };
        let start = position;
        while position < bytes.len()
            && (bytes[position].is_ascii_digit() || bytes[position] == b'_')
        {
            position += 1;
        }
        Some((
            uppercase,
            sign,
            (position > start).then_some(&full[start..position]),
        ))
    } else {
        None
    };
    classify_number(full, magnitude, fraction, exponent, &full[position..])
}

pub fn lexer<'src>()
-> impl Parser<'src, &'src str, Vec<Spanned<Token<'src>>>, extra::Err<Rich<'src, char, Span>>> {
    // `integer-magnitude` (Specification 020 section 4): a lowercase radix
    // prefix (captured with its rejected uppercase spelling too, so a single
    // diagnostic can name it) with an optional digit run, tried before plain
    // `decimal-digits` so "0x1" commits to the hex alternative rather than
    // reading "0" and leaving "x1" as trailing garbage.
    let hex_magnitude = just("0x")
        .or(just("0X"))
        .to_slice()
        .then(digit_run("0123456789abcdefABCDEF").or_not())
        .map(|(prefix, digits)| RawMagnitude::Radix(prefix, digits));
    let octal_magnitude = just("0o")
        .or(just("0O"))
        .to_slice()
        .then(digit_run("01234567").or_not())
        .map(|(prefix, digits)| RawMagnitude::Radix(prefix, digits));
    let binary_magnitude = just("0b")
        .or(just("0B"))
        .to_slice()
        .then(digit_run("01").or_not())
        .map(|(prefix, digits)| RawMagnitude::Radix(prefix, digits));
    let decimal_magnitude = digit_run("0123456789").map(RawMagnitude::Decimal);
    let magnitude = hex_magnitude
        .or(octal_magnitude)
        .or(binary_magnitude)
        .or(decimal_magnitude);

    // `".", decimal-digits`, kept even when no digit follows the point so
    // "missing digit after decimal point" stays one whole-token diagnostic
    // instead of leaving `.` for a separate (meaningless, since no built-in
    // scalar has fields or methods) member-access token.
    let fraction = just('.')
        .ignore_then(digit_run("0123456789").or_not())
        .or_not();

    // `exponent = "e", [ "+" | "-" ], decimal-digits` (Specification 020
    // section 4), with the rejected uppercase spelling captured too. The
    // marker is matched context-free, regardless of radix: a hex literal's
    // `e`/`E` digits are already consumed by `hex_magnitude` above, so only a
    // genuine decimal exponent -- or one misplaced on a binary/octal literal
    // -- ever reaches here.
    let exponent = just('e')
        .to(false)
        .or(just('E').to(true))
        .then(one_of("+-").or_not())
        .then(digit_run("0123456789").or_not())
        .map(|((uppercase, sign), digits)| (uppercase, sign, digits))
        .or_not();

    // Maximal munch: whatever alphanumeric/`_` content immediately follows
    // the structured pieces above -- a valid suffix, or invalid leftover
    // content -- stays part of this one token rather than starting a new one.
    let trailing = any()
        .filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_')
        .repeated()
        .to_slice();

    // A rejected literal is reported through `validate` rather than `try_map`
    // so the diagnostic keeps the whole token's span: chumsky merges a
    // `try_map` failure into the surrounding alternative's error and narrows it
    // to that alternative's position. `crate::parse` stops at any lex error, so
    // the stand-in token below never reaches the parser.
    let num = magnitude
        .then(fraction)
        .then(exponent)
        .then(trailing)
        .validate(
            |(((magnitude, fraction), exponent), trailing), extra, emitter| {
                let full = extra.slice();
                match classify_number(full, magnitude, fraction, exponent, trailing) {
                    Ok(literal) => Token::Num(literal),
                    Err(message) => {
                        emitter.emit(Rich::custom(extra.span(), message));
                        Token::Num(NumLiteral::Int(0))
                    }
                }
            },
        );

    // Raw strings need a delimiter whose closing hash count matches the
    // opening count. Chumsky's ordinary combinators cannot express that
    // dependent delimiter, so this small custom parser scans the literal and
    // returns only its unprocessed body.
    let raw = just('r').ignore_then(custom(|inp| {
        let start = inp.save();
        let start_cursor = start.cursor().clone();
        let mut hashes = 0usize;
        while inp.peek() == Some('#') {
            inp.next();
            hashes += 1;
            if hashes > 255 {
                let span = inp.span_since(&start_cursor);
                inp.rewind(start);
                return Err(Rich::custom(
                    span,
                    "raw string delimiter may contain at most 255 '#' characters",
                ));
            }
        }
        if inp.next() != Some('"') {
            let span = inp.span_since(&start_cursor);
            inp.rewind(start);
            return Err(Rich::custom(span, "raw string literal must begin with r\""));
        }
        let body_start = inp.cursor();
        loop {
            let candidate = inp.save();
            let Some(character) = inp.next() else {
                return Err(Rich::custom(
                    inp.span_since(&start_cursor),
                    "unterminated raw string literal",
                ));
            };
            if character != '"' {
                continue;
            }
            let mut closes = true;
            for _ in 0..hashes {
                if inp.next() != Some('#') {
                    closes = false;
                    break;
                }
            }
            if closes {
                let body_end = candidate.cursor().clone();
                let body = inp.slice(&body_start..&body_end);
                return Ok(Token::RawStr(body));
            }
            // The quote was content because the following hash run did not
            // match. Resume scanning after that quote.
            inp.rewind(candidate);
            inp.next();
        }
    }));

    // Interpreted strings are scanned as one lexical unit so interpolation
    // delimiters cannot be confused with ordinary expression tokens. Each
    // embedded expression is immediately tokenized with the same lexer; the
    // parser later feeds those tokens to its normal expression parser.
    let str_ = just('"').ignore_then(custom(|inp| {
        let mut literal_start = inp.cursor().clone();
        let mut parts = Vec::new();
        loop {
            let character_start = inp.cursor().clone();
            let Some(character) = inp.next() else {
                return Err(Rich::custom(
                    inp.span_since(&literal_start),
                    "unterminated interpreted string literal",
                ));
            };
            match character {
                '"' => {
                    if parts.is_empty() {
                        let body: &str = inp.slice(&literal_start..&inp.cursor());
                        let body = &body[..body.len() - 1];
                        return Ok(Token::Str(body));
                    }
                    {
                        let end = inp.cursor();
                        let body: &str = inp.slice(&literal_start..&end);
                        let body = &body[..body.len() - 1];
                        if !body.is_empty() {
                            match decode_string_content(body) {
                                Ok(text) => parts.push(InterpolatedPart::Literal(text)),
                                Err(message) => {
                                    return Err(Rich::custom(
                                        inp.span_since(&literal_start),
                                        message,
                                    ));
                                }
                            }
                        }
                    }
                    return Ok(Token::Interpolated(parts));
                }
                '\n' | '\r' => {
                    return Err(Rich::custom(
                        inp.span_since(&literal_start),
                        "an interpreted string literal cannot contain an unescaped line break",
                    ));
                }
                '\\' => {
                    // Skip the escaped character while scanning delimiters;
                    // `decode_string_content` owns validation and decoding.
                    let Some(escaped) = inp.next() else {
                        return Err(Rich::custom(
                            inp.span_since(&literal_start),
                            "string literal ends with an incomplete escape",
                        ));
                    };
                    if escaped == 'u' {
                        if inp.next() != Some('{') {
                            return Err(Rich::custom(
                                inp.span_since(&literal_start),
                                "Unicode escape must use the form \\u{H...}",
                            ));
                        }
                        loop {
                            let Some(next) = inp.next() else {
                                return Err(Rich::custom(
                                    inp.span_since(&literal_start),
                                    "Unicode escape is missing its closing '}'",
                                ));
                            };
                            if next == '}' {
                                break;
                            }
                        }
                    }
                }
                '{' if inp.peek() == Some('{') => {
                    let segment_end = character_start;
                    let segment: &str = inp.slice(&literal_start..&segment_end);
                    if !segment.is_empty() {
                        match decode_string_content(segment) {
                            Ok(text) => parts.push(InterpolatedPart::Literal(text)),
                            Err(message) => {
                                return Err(Rich::custom(inp.span_since(&literal_start), message));
                            }
                        }
                    }
                    inp.next();
                    let expression_start = inp.cursor();
                    let mut depth = 0usize;
                    'interpolation: loop {
                        let Some(next) = inp.next() else {
                            return Err(Rich::custom(
                                inp.span_since(&literal_start),
                                "interpolation is missing its closing '}}'",
                            ));
                        };
                        match next {
                            'r' => {
                                let marker = inp.save();
                                let mut hashes = 0usize;
                                while inp.peek() == Some('#') {
                                    inp.next();
                                    hashes += 1;
                                    if hashes > 255 {
                                        return Err(Rich::custom(
                                            inp.span_since(&literal_start),
                                            "raw string delimiter may contain at most 255 '#' characters",
                                        ));
                                    }
                                }
                                if inp.next() == Some('"') {
                                    loop {
                                        let Some(inner) = inp.next() else {
                                            return Err(Rich::custom(
                                                inp.span_since(&literal_start),
                                                "unterminated raw string inside interpolation",
                                            ));
                                        };
                                        if inner != '"' {
                                            continue;
                                        }
                                        let mut closes = true;
                                        for _ in 0..hashes {
                                            if inp.next() != Some('#') {
                                                closes = false;
                                                break;
                                            }
                                        }
                                        if closes {
                                            break;
                                        }
                                    }
                                } else {
                                    inp.rewind(marker);
                                }
                            }
                            '"' => loop {
                                let Some(inner) = inp.next() else {
                                    return Err(Rich::custom(
                                        inp.span_since(&literal_start),
                                        "unterminated string inside interpolation",
                                    ));
                                };
                                if inner == '\\' {
                                    inp.next();
                                } else if matches!(inner, '\n' | '\r') {
                                    return Err(Rich::custom(
                                        inp.span_since(&literal_start),
                                        "an interpreted string literal cannot contain an unescaped line break",
                                    ));
                                } else if inner == '"' {
                                    break;
                                }
                            },
                            '\'' => loop {
                                let Some(inner) = inp.next() else {
                                    return Err(Rich::custom(
                                        inp.span_since(&literal_start),
                                        "unterminated Unicode literal inside interpolation",
                                    ));
                                };
                                if inner == '\\' {
                                    inp.next();
                                } else if inner == '\'' {
                                    break;
                                }
                            },
                            '{' => depth += 1,
                            '}' if depth > 0 => depth -= 1,
                            '}' if inp.peek() == Some('}') => {
                                inp.next();
                                break 'interpolation;
                            }
                            _ => {}
                        }
                    }
                    let expression_end = inp.cursor();
                    let expression_span: chumsky::span::SimpleSpan =
                        inp.span_since(&expression_start);
                    let expression: &str = inp.slice(&expression_start..&expression_end);
                    let expression = &expression[..expression.len() - 2];
                    if expression.trim().is_empty() {
                        return Err(Rich::custom(
                            expression_span,
                            "interpolation expression cannot be empty",
                        ));
                    }
                    let tokens = match lex_interpolation_fragment(expression, expression_span.start)
                    {
                        Ok(tokens) if !tokens.is_empty() => tokens,
                        Ok(_) => {
                            return Err(Rich::custom(
                                expression_span,
                                "interpolation expression produced no tokens",
                            ));
                        }
                        Err(message) => {
                            return Err(Rich::custom(expression_span, message));
                        }
                    };
                    parts.push(InterpolatedPart::Expression(tokens));
                    // The next literal starts immediately after the closing
                    // delimiter; retain it as a source slice for decoding.
                    literal_start = inp.cursor().clone();
                }
                '}' if inp.peek() == Some('}') => {
                    return Err(Rich::custom(
                        inp.span_since(&literal_start),
                        "unexpected closing interpolation delimiter '}}'",
                    ));
                }
                _ => {}
            }
        }
    }));

    let unicode = just('\'')
        .ignore_then(
            any()
                .filter(|c: &char| *c != '\'' && *c != '\n' && *c != '\r')
                .repeated()
                .at_least(1)
                .to_slice(),
        )
        .then_ignore(just('\''))
        .validate(
            |value, extra, emitter| match decode_unicode_content(value) {
                Ok(value) => Token::Unicode(value),
                Err(message) => {
                    emitter.emit(Rich::custom(extra.span(), message));
                    Token::Unicode(0)
                }
            },
        );

    // Snacc's only multi-character operators are the four two-character
    // comparisons, so they are spelled out instead of munched greedily.
    // Specification 011 section 4 puts `>` in a type position, where a greedy
    // run would swallow the two closing brackets of `Ref<Ref<T>>` into one
    // token and hide the nested reference from the parser.
    let op = just("==")
        .or(just("!="))
        .or(just("<="))
        .or(just(">="))
        .or(one_of("+*-/!=<>").to_slice())
        .map(Token::Op);

    // `.` selects a member and continues a qualified path; `|` introduces a
    // union alternative. Numeric literals are munched before this alternative
    // runs, so `1.5` is still one token.
    let ctrl = one_of("()[],:.|").map(Token::Ctrl);

    let ident = text::ascii::ident().map(|ident: &str| match ident {
        "fun" => Token::Fun,
        "extern" => Token::Extern,
        "rust" => Token::Rust,
        "let" => Token::Let,
        "mut" => Token::Mut,
        "type" => Token::Type,
        "is" => Token::Is,
        "struct" => Token::Struct,
        "union" => Token::Union,
        "method" => Token::Method,
        "static" => Token::Static,
        "self" => Token::SelfKw,
        "Ref" => Token::Ref,
        "Box" => Token::Box,
        "box" => Token::BoxExpr,
        "print" => Token::Print,
        "if" => Token::If,
        "then" => Token::Then,
        "Float64" => Token::TyFloat64,
        "Int64" => Token::TyInt64,
        "Bool" => Token::TyBool,
        "Nil" => Token::TyNil,
        "String" => Token::TyString,
        "Unicode" => Token::TyUnicode,
        "Byte" => Token::TyByte,
        "View" => Token::TyView,
        "Array" => Token::TyArray,
        "List" => Token::TyList,
        "Map" => Token::TyMap,
        "Set" => Token::TySet,
        "UInt8" => Token::RemovedUInt8,
        "UInt16" => Token::TyUInt16,
        "UInt32" => Token::TyUInt32,
        "UInt64" => Token::TyUInt64,
        "Float32" => Token::TyFloat32,
        // Specification 020 section 8: `nil` is the sole contextual `Nil`
        // spelling; `null` is an ordinary identifier with no built-in meaning.
        "nil" => Token::Nil,
        "while" => Token::While,
        "for" => Token::For,
        "in" => Token::In,
        "do" => Token::Do,
        "elseif" => Token::ElseIf,
        "else" => Token::Else,
        "end" => Token::End,
        "break" => Token::Break,
        "return" => Token::Return,
        "and" => Token::And,
        "or" => Token::Or,
        "return_on_error" => Token::ReturnOnError,
        "defer" => Token::Defer,
        "defer_on_error" => Token::DeferOnError,
        "true" => Token::Bool(true),
        "false" => Token::Bool(false),
        _ => Token::Ident(ident),
    });

    let token = num.or(raw).or(str_).or(unicode).or(op).or(ctrl).or(ident);

    let comment = just("//")
        .then(any().and_is(just('\n').not()).repeated())
        .padded();

    token
        .map_with(|tok, e| (tok, e.span()))
        .padded_by(comment.repeated())
        .padded()
        .recover_with(skip_then_retry_until(any().ignored(), end()))
        .repeated()
        .collect()
}

/// Tokenizes an interpolation expression through a concrete helper result so
/// the recursive string-token path does not recursively instantiate Chumsky's
/// opaque parser type.
fn lex_interpolation_fragment<'src>(
    source: &'src str,
    source_offset: usize,
) -> Result<Vec<Spanned<Token<'src>>>, String> {
    let mut tokens = Vec::new();
    let mut position = 0usize;
    while position < source.len() {
        let character = source[position..]
            .chars()
            .next()
            .expect("position is a character boundary");
        if character.is_whitespace() {
            position += character.len_utf8();
            continue;
        }
        if source[position..].starts_with("//") {
            while position < source.len()
                && source[position..]
                    .chars()
                    .next()
                    .is_some_and(|next| next != '\n')
            {
                position += source[position..]
                    .chars()
                    .next()
                    .expect("position is a character boundary")
                    .len_utf8();
            }
            continue;
        }
        let start = position;
        let token = if character.is_ascii_digit() {
            while position < source.len()
                && source[position..].chars().next().is_some_and(|next| {
                    next.is_ascii_alphanumeric()
                        || matches!(next, '_' | '.')
                        || (matches!(next, '+' | '-')
                            && position > start
                            && matches!(source.as_bytes()[position - 1], b'e' | b'E'))
                })
            {
                position += source[position..]
                    .chars()
                    .next()
                    .expect("position is a character boundary")
                    .len_utf8();
            }
            let text = &source[start..position];
            Token::Num(classify_fragment_number(text)?)
        } else if character == 'r'
            && source[start + 1..]
                .chars()
                .next()
                .is_some_and(|next| matches!(next, '#' | '"'))
        {
            position += 1;
            let mut hashes = 0usize;
            while position < source.len() && source.as_bytes()[position] == b'#' {
                position += 1;
                hashes += 1;
                if hashes > 255 {
                    return Err(
                        "raw string delimiter may contain at most 255 '#' characters".into(),
                    );
                }
            }
            if position >= source.len() || source.as_bytes()[position] != b'"' {
                return Err("raw string literal must begin with r\"".into());
            }
            position += 1;
            let body_start = position;
            loop {
                let Some(relative) = source[position..].find('"') else {
                    return Err("unterminated raw string inside interpolation".into());
                };
                let quote = position + relative;
                let hashes_start = quote + 1;
                let hashes_end = hashes_start.saturating_add(hashes);
                if hashes_end <= source.len()
                    && source.as_bytes()[hashes_start..hashes_end]
                        .iter()
                        .all(|byte| *byte == b'#')
                {
                    position = hashes_end;
                    break Token::RawStr(&source[body_start..quote]);
                }
                position = quote + 1;
            }
        } else if character == '"' {
            scan_fragment_string(source, &mut position, source_offset)?
        } else if character == '\'' {
            position += 1;
            let body_start = position;
            loop {
                let Some(next) = source[position..].chars().next() else {
                    return Err("unterminated Unicode literal inside interpolation".into());
                };
                position += next.len_utf8();
                if next == '\\' {
                    let Some(escaped) = source[position..].chars().next() else {
                        return Err("Unicode literal ends with an incomplete escape".into());
                    };
                    position += escaped.len_utf8();
                } else if next == '\'' {
                    break;
                }
            }
            Token::Unicode(decode_unicode_content(&source[body_start..position - 1])?)
        } else if let Some((text, token)) = [
            ("==", Token::Op("==")),
            ("!=", Token::Op("!=")),
            ("<=", Token::Op("<=")),
            (">=", Token::Op(">=")),
        ]
        .iter()
        .find(|(text, _)| source[position..].starts_with(text))
        {
            position += text.len();
            token.clone()
        } else if "+*-/!<>=|".contains(character) {
            position += character.len_utf8();
            Token::Op(&source[start..position])
        } else if "()[],:.".contains(character) {
            position += character.len_utf8();
            Token::Ctrl(character)
        } else if character.is_ascii_alphabetic() || character == '_' {
            position += character.len_utf8();
            while position < source.len()
                && source[position..]
                    .chars()
                    .next()
                    .is_some_and(|next| next.is_ascii_alphanumeric() || next == '_')
            {
                position += source[position..]
                    .chars()
                    .next()
                    .expect("position is a character boundary")
                    .len_utf8();
            }
            let text = &source[start..position];
            fragment_keyword(text)
        } else {
            return Err(format!(
                "unexpected character '{character}' in interpolation"
            ));
        };
        tokens.push((
            token,
            (source_offset + start..source_offset + position).into(),
        ));
    }
    Ok(tokens)
}

/// Scans an interpreted string nested in an interpolation expression. It
/// mirrors the outer string token's structured representation so nested
/// interpolation remains ordinary expression syntax instead of becoming
/// literal braces.
fn scan_fragment_string<'src>(
    source: &'src str,
    position: &mut usize,
    source_offset: usize,
) -> Result<Token<'src>, String> {
    *position += 1;
    let body_start = *position;
    let mut literal_start = *position;
    let mut parts = Vec::new();
    loop {
        let Some(character) = source[*position..].chars().next() else {
            return Err("unterminated string inside interpolation".into());
        };
        let character_start = *position;
        *position += character.len_utf8();
        match character {
            '"' => {
                if parts.is_empty() {
                    return Ok(Token::Str(&source[body_start..character_start]));
                }
                let segment = &source[literal_start..character_start];
                if !segment.is_empty() {
                    parts.push(InterpolatedPart::Literal(decode_string_content(segment)?));
                }
                return Ok(Token::Interpolated(parts));
            }
            '\n' | '\r' => {
                return Err(
                    "an interpreted string literal cannot contain an unescaped line break".into(),
                );
            }
            '\\' => {
                let Some(escaped) = source[*position..].chars().next() else {
                    return Err("string literal ends with an incomplete escape".into());
                };
                *position += escaped.len_utf8();
                if escaped == 'u' && source[*position..].starts_with('{') {
                    let Some(close) = source[*position + 1..].find('}') else {
                        return Err("Unicode escape is missing its closing '}'".into());
                    };
                    *position += close + 2;
                }
            }
            '{' if source[*position..].starts_with('{') => {
                let segment = &source[literal_start..character_start];
                if !segment.is_empty() {
                    parts.push(InterpolatedPart::Literal(decode_string_content(segment)?));
                }
                *position += 1;
                let expression_start = *position;
                let expression_end = scan_fragment_interpolation_end(source, position)?;
                let expression = &source[expression_start..expression_end];
                if expression.trim().is_empty() {
                    return Err("interpolation expression cannot be empty".into());
                }
                let tokens =
                    lex_interpolation_fragment(expression, source_offset + expression_start)?;
                if tokens.is_empty() {
                    return Err("interpolation expression produced no tokens".into());
                }
                parts.push(InterpolatedPart::Expression(tokens));
                literal_start = *position;
            }
            '}' if source[*position..].starts_with('}') => {
                return Err("unexpected closing interpolation delimiter '}}'".into());
            }
            _ => {}
        }
    }
}

/// Advances through one nested interpolation expression and returns the byte
/// before its outer `}}`. Braces inside string and Unicode literals do not
/// participate in balancing.
fn scan_fragment_interpolation_end(source: &str, position: &mut usize) -> Result<usize, String> {
    let mut depth = 0usize;
    loop {
        let Some(character) = source[*position..].chars().next() else {
            return Err("interpolation is missing its closing '}}'".into());
        };
        let start = *position;
        *position += character.len_utf8();
        match character {
            'r' if source[*position..].starts_with('#') || source[*position..].starts_with('"') => {
                let mut hashes = 0usize;
                while source[*position..].starts_with('#') {
                    *position += 1;
                    hashes += 1;
                    if hashes > 255 {
                        return Err(
                            "raw string delimiter may contain at most 255 '#' characters".into(),
                        );
                    }
                }
                if !source[*position..].starts_with('"') {
                    continue;
                }
                *position += 1;
                loop {
                    let Some(relative) = source[*position..].find('"') else {
                        return Err("unterminated raw string inside interpolation".into());
                    };
                    let quote = *position + relative;
                    let close = quote + 1 + hashes;
                    if close <= source.len()
                        && source.as_bytes()[quote + 1..close]
                            .iter()
                            .all(|byte| *byte == b'#')
                    {
                        *position = close;
                        break;
                    }
                    *position = quote + 1;
                }
            }
            '"' => {
                scan_fragment_string(source, position, 0)?;
            }
            '\'' => loop {
                let Some(inner) = source[*position..].chars().next() else {
                    return Err("unterminated Unicode literal inside interpolation".into());
                };
                *position += inner.len_utf8();
                if inner == '\\' {
                    let Some(escaped) = source[*position..].chars().next() else {
                        return Err("Unicode literal ends with an incomplete escape".into());
                    };
                    *position += escaped.len_utf8();
                } else if inner == '\'' {
                    break;
                }
            },
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            '}' if source[*position..].starts_with('}') => {
                *position += 1;
                return Ok(start);
            }
            _ => {}
        }
    }
}

fn fragment_keyword<'src>(text: &'src str) -> Token<'src> {
    match text {
        "fun" => Token::Fun,
        "extern" => Token::Extern,
        "rust" => Token::Rust,
        "let" => Token::Let,
        "mut" => Token::Mut,
        "print" => Token::Print,
        "if" => Token::If,
        "then" => Token::Then,
        "do" => Token::Do,
        "type" => Token::Type,
        "is" => Token::Is,
        "struct" => Token::Struct,
        "union" => Token::Union,
        "method" => Token::Method,
        "static" => Token::Static,
        "self" => Token::SelfKw,
        "Ref" => Token::Ref,
        "Box" => Token::Box,
        "box" => Token::BoxExpr,
        "Float64" => Token::TyFloat64,
        "Int64" => Token::TyInt64,
        "Bool" => Token::TyBool,
        "Nil" => Token::TyNil,
        "String" => Token::TyString,
        "Unicode" => Token::TyUnicode,
        "Byte" => Token::TyByte,
        "View" => Token::TyView,
        "Array" => Token::TyArray,
        "List" => Token::TyList,
        "Map" => Token::TyMap,
        "Set" => Token::TySet,
        "UInt16" => Token::TyUInt16,
        "UInt32" => Token::TyUInt32,
        "UInt64" => Token::TyUInt64,
        "Float32" => Token::TyFloat32,
        "nil" => Token::Nil,
        "while" => Token::While,
        "for" => Token::For,
        "in" => Token::In,
        "elseif" => Token::ElseIf,
        "else" => Token::Else,
        "end" => Token::End,
        "break" => Token::Break,
        "return" => Token::Return,
        "and" => Token::And,
        "or" => Token::Or,
        "return_on_error" => Token::ReturnOnError,
        "defer" => Token::Defer,
        "defer_on_error" => Token::DeferOnError,
        "true" => Token::Bool(true),
        "false" => Token::Bool(false),
        _ => Token::Ident(text),
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
