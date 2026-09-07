//! Runtime symbol name construction (Specification 029 section 7.1).
//!
//! One table maps every runtime import to its symbol, parameter shape, and
//! result. The declaration path in [`super::imports`] builds the LLVM
//! signature from the row, so emitted declarations stay byte-identical while
//! the 46 hand-written `*_import` methods are gone. Per-family row helpers
//! and short shape aliases keep each table row on one line (the table carries
//! `rustfmt::skip` so the data stays tabular); rows needing a fixed symbol
//! use [`row`] directly.

use super::*;

pub(crate) fn concat_part_tag(ty: Ty) -> Result<u64, String> {
    match ty {
        Ty::Int64 => Ok(1),
        Ty::Byte => Ok(2),
        Ty::UInt16 => Ok(3),
        Ty::UInt32 => Ok(4),
        Ty::UInt64 => Ok(5),
        Ty::Float32 => Ok(6),
        Ty::Float64 => Ok(7),
        Ty::Bool => Ok(8),
        Ty::Unicode => Ok(9),
        Ty::ViewUnicode => Ok(10),
        _ => Err(internal("unsupported string concatenation part")),
    }
}

/// Which runtime family an import belongs to. `Map` is the typed `K -> Int64`
/// map; `MapRaw` the byte-storing one selected by `map_uses_raw_value`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    Map,
    MapRaw,
    Set,
    List,
    ListRaw,
    ListFixed,
    String,
    View,
    Print,
    Fail,
}

/// How a row's key position lowers. `String` and `View` (`ViewByte`) keys
/// share a symbol suffix with each other but lower the key argument
/// differently per operation, so they are distinct classes; every other valid
/// key lowers as its own LLVM type. `Any` marks operations without a key.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyClass {
    String,
    View,
    Scalar,
    Any,
}

/// One parameter of a runtime signature. `Key` is the row's primary type as
/// its own LLVM type (`String`-class rows spell `Ptr` out instead);
/// `Assoc` is the associated value/element type; `Scalar` is `scalar_ty` of
/// the primary type (print arguments); `View` is the view descriptor.
#[derive(Clone, Copy)]
pub(crate) enum AbiTy {
    Ptr,
    Usize,
    I64,
    Key,
    Assoc,
    Scalar,
    View,
}

/// A runtime signature's result using the same type variables as [`AbiTy`].
#[derive(Clone, Copy)]
pub(crate) enum AbiResult {
    Void,
    I8,
    I64,
    Key,
    Assoc,
}

/// How a row's symbol is found. Most rows follow their family's naming
/// pattern with the primary type's suffix; `Literal` rows name one fixed
/// symbol; `Split` preserves the historical view-equality declaration, whose
/// cache lookup and declaration names differ.
#[derive(Clone, Copy)]
pub(crate) enum SymbolSpec {
    Pattern {
        prefix: &'static str,
        middle: &'static str,
        suffix_first: bool,
    },
    Literal(&'static str),
    Split {
        lookup: &'static str,
        declare: &'static str,
    },
}

pub(crate) struct ImportRow {
    pub(crate) family: Family,
    pub(crate) op: &'static str,
    pub(crate) class: KeyClass,
    pub(crate) symbol: SymbolSpec,
    pub(crate) params: &'static [AbiTy],
    pub(crate) result: AbiResult,
}

// Short shape aliases so table rows read as data. `S`/`V`/`K` are key
// classes; `P`/`U`/`I` are pointer/usize/i64 parameters; `KY`/`AS` are the
// key/associated types; `VD`/`I8`/`RK` are void/i8/key results.
const S: KeyClass = KeyClass::String;
const V: KeyClass = KeyClass::View;
const K: KeyClass = KeyClass::Scalar;
const P: AbiTy = AbiTy::Ptr;
const U: AbiTy = AbiTy::Usize;
const I: AbiTy = AbiTy::I64;
const KY: AbiTy = AbiTy::Key;
const A: AbiTy = AbiTy::Assoc;
const SC: AbiTy = AbiTy::Scalar;
const VW: AbiTy = AbiTy::View;
const VD: AbiResult = AbiResult::Void;
const I8: AbiResult = AbiResult::I8;
const I64R: AbiResult = AbiResult::I64;
const AS: AbiResult = AbiResult::Assoc;
const RK: AbiResult = AbiResult::Key;
const A2: KeyClass = KeyClass::Any;

#[rustfmt::skip]
/// The single runtime import table: one row per (family, operation,
/// key class). A `Pattern` row's symbol is `snacc_{prefix}_{suffix}_{middle}_
/// {op}` (`suffix_first`) or `snacc_{prefix}_{op}_{suffix}`, with an empty
/// `middle` omitted.
pub(crate) static IMPORTS: &[ImportRow] = &[
    // Typed maps (`K -> Int64`).
    mrow("contains", S, &[P, P], I8),
    mrow("contains", V, &[P, P], I8),
    mrow("contains", K, &[P, KY], I8),
    mrow("insert", S, &[P, P, A], I8),
    mrow("insert", V, &[P, KY, A], I8),
    mrow("insert", K, &[P, KY, A], I8),
    mrow("delete", S, &[P, P], I8),
    mrow("delete", V, &[P, P], I8),
    mrow("delete", K, &[P, KY], I8),
    mrow("index", S, &[P, P], AS),
    mrow("index", V, &[P, P], AS),
    mrow("index", K, &[P, KY], AS),
    mrow("take", S, &[P, P], AS),
    mrow("take", V, &[P, P], AS),
    mrow("take", K, &[P, KY], AS),
    row(Family::Map, "key_at", S, lit("snacc_map_string_i64_key_at_out"), &[P, P, I], VD),
    mrow("key_at", V, &[P, I], RK),
    mrow("key_at", K, &[P, I], RK),
    mrow("value_at", S, &[P, I], AS),
    mrow("value_at", V, &[P, I], AS),
    mrow("value_at", K, &[P, I], AS),
    mrow("clear", S, &[P], VD),
    mrow("clear", V, &[P], VD),
    mrow("clear", K, &[P], VD),
    mrow("reserve", S, &[P, I], VD),
    mrow("reserve", V, &[P, I], VD),
    mrow("reserve", K, &[P, I], VD),
    mrow("drop", S, &[P], VD),
    mrow("drop", V, &[P], VD),
    mrow("drop", K, &[P], VD),
    // Raw (byte-storing) maps.
    rrow("insert", S, &[P, P, P, U, P], I8),
    rrow("insert", V, &[P, KY, P, U, P], I8),
    rrow("insert", K, &[P, KY, P, U, P], I8),
    rrow("delete", S, &[P, P, P, U], I8),
    rrow("delete", V, &[P, P, P, U], I8),
    rrow("delete", K, &[P, KY, P, U], I8),
    rrow("contains", S, &[P, P], I8),
    rrow("contains", V, &[P, P], I8),
    rrow("contains", K, &[P, KY], I8),
    rrow("index", S, &[P, P, P, U], VD),
    rrow("index", V, &[P, P, P, U], VD),
    rrow("index", K, &[P, KY, P, U], VD),
    rrow("take", S, &[P, P, P, U], VD),
    rrow("take", V, &[P, P, P, U], VD),
    rrow("take", K, &[P, KY, P, U], VD),
    rrow("value_at", S, &[P, I, P, U], VD),
    rrow("value_at", V, &[P, I, P, U], VD),
    rrow("value_at", K, &[P, I, P, U], VD),
    rrow("key_at", S, &[P, P, I], VD),
    rrow("key_at", V, &[P, I], RK),
    rrow("key_at", K, &[P, I], RK),
    rrow("clear", S, &[P], VD),
    rrow("clear", V, &[P], VD),
    rrow("clear", K, &[P], VD),
    rrow("reserve", S, &[P, I], VD),
    rrow("reserve", V, &[P, I], VD),
    rrow("reserve", K, &[P, I], VD),
    rrow("drop", S, &[P], VD),
    rrow("drop", V, &[P], VD),
    rrow("drop", K, &[P], VD),
    // Sets.
    srow("contains", S, &[P, P], I8),
    srow("contains", V, &[P, P], I8),
    srow("contains", K, &[P, KY], I8),
    srow("insert", S, &[P, P], I8),
    srow("insert", V, &[P, KY], I8),
    srow("insert", K, &[P, KY], I8),
    srow("delete", S, &[P, P], I8),
    srow("delete", V, &[P, P], I8),
    srow("delete", K, &[P, KY], I8),
    row(Family::Set, "at", S, lit("snacc_set_string_at_out"), &[P, P, I], VD),
    srow("at", V, &[P, I], RK),
    srow("at", K, &[P, I], RK),
    srow("clear", S, &[P], VD),
    srow("clear", V, &[P], VD),
    srow("clear", K, &[P], VD),
    srow("reserve", S, &[P, I], VD),
    srow("reserve", V, &[P, I], VD),
    srow("reserve", K, &[P, I], VD),
    srow("drop", S, &[P], VD),
    srow("drop", V, &[P], VD),
    srow("drop", K, &[P], VD),
    // Scalar lists (only scalar elements validate; anything else misses).
    lrow("push", &[P, KY], VD),
    lrow("pop", &[P], RK),
    lrow("insert", &[P, I, KY], VD),
    lrow("remove", &[P, I], RK),
    // Raw lists.
    frow(Family::ListRaw, "push", "snacc_list_push_raw", &[P, P, U, U], VD),
    frow(Family::ListRaw, "pop", "snacc_list_pop_raw", &[P, P, U], VD),
    frow(Family::ListRaw, "insert", "snacc_list_insert_raw", &[P, I, P, U, U], VD),
    frow(Family::ListRaw, "remove", "snacc_list_remove_raw", &[P, I, P, U], VD),
    frow(Family::ListRaw, "clear", "snacc_list_clear_raw", &[P], VD),
    // Fixed lists.
    frow(Family::ListFixed, "clear", "snacc_list_clear", &[P], VD),
    frow(Family::ListFixed, "reserve", "snacc_list_reserve", &[P, I, U, U], VD),
    // Strings.
    frow(Family::String, "equal", "snacc_string_equal_ptr", &[P, P], I8),
    frow(Family::String, "new", "snacc_string_new_out", &[P, P, U], VD),
    frow(Family::String, "concat_parts", "snacc_string_concat_parts_out", &[P, P, U], VD),
    frow(Family::String, "clone", "snacc_string_clone_out", &[P, P], VD),
    frow(Family::String, "drop", "snacc_string_drop_ptr", &[P], VD),
    frow(Family::String, "bytes", "snacc_string_bytes_out", &[P, P], VD),
    frow(Family::String, "unicode", "snacc_string_unicode_out", &[P, P], VD),
    frow(Family::String, "from_view", "snacc_string_from_view_out", &[P, P], VD),
    frow(Family::String, "from_utf8", "snacc_string_from_utf8_out", &[P, P], VD),
    // Views.
    frow(Family::View, "length_byte", "snacc_view_byte_length", &[P], I64R),
    frow(Family::View, "length_unicode", "snacc_view_unicode_length", &[P], I64R),
    frow(Family::View, "at_byte", "snacc_view_byte_at_ptr", &[P, I], I64R),
    frow(Family::View, "at_unicode", "snacc_view_unicode_at_ptr", &[P, I], I64R),
    frow(Family::View, "slice_byte", "snacc_view_byte_slice_out", &[P, P, I, I], VD),
    frow(Family::View, "slice_unicode", "snacc_view_unicode_slice_out", &[P, P, I, I], VD),
    row(Family::View, "equal", A2, split("snacc_view_equal", "snacc_view_equal_ptr"), &[P, P], I8),
    // Prints (one row per printable type; `Scalar` is `scalar_ty` of it).
    frow(Family::Print, "f64", "snacc_print_f64", &[SC], VD),
    frow(Family::Print, "f32", "snacc_print_f32", &[SC], VD),
    frow(Family::Print, "i64", "snacc_print_i64", &[SC], VD),
    frow(Family::Print, "u8", "snacc_print_u8", &[SC], VD),
    frow(Family::Print, "u16", "snacc_print_u16", &[SC], VD),
    frow(Family::Print, "u32", "snacc_print_u32", &[SC], VD),
    frow(Family::Print, "u64", "snacc_print_u64", &[SC], VD),
    frow(Family::Print, "bool", "snacc_print_bool", &[SC], VD),
    frow(Family::Print, "unicode", "snacc_print_unicode", &[SC], VD),
    frow(Family::Print, "string", "snacc_print_string_ptr", &[P], VD),
    frow(Family::Print, "unicode_view", "snacc_print_unicode_view", &[VW], VD),
    // Failure paths.
    frow(Family::Fail, "bounds_fail", "snacc_collection_bounds_fail", &[], VD),
];

const fn mrow(
    op: &'static str,
    class: KeyClass,
    params: &'static [AbiTy],
    result: AbiResult,
) -> ImportRow {
    row(
        Family::Map,
        op,
        class,
        pat("map", "i64", true),
        params,
        result,
    )
}

const fn rrow(
    op: &'static str,
    class: KeyClass,
    params: &'static [AbiTy],
    result: AbiResult,
) -> ImportRow {
    row(
        Family::MapRaw,
        op,
        class,
        pat("map", "raw", true),
        params,
        result,
    )
}

const fn srow(
    op: &'static str,
    class: KeyClass,
    params: &'static [AbiTy],
    result: AbiResult,
) -> ImportRow {
    row(Family::Set, op, class, pat("set", "", true), params, result)
}

const fn lrow(op: &'static str, params: &'static [AbiTy], result: AbiResult) -> ImportRow {
    row(
        Family::List,
        op,
        KeyClass::Scalar,
        pat("list", "", false),
        params,
        result,
    )
}

const fn frow(
    family: Family,
    op: &'static str,
    symbol: &'static str,
    params: &'static [AbiTy],
    result: AbiResult,
) -> ImportRow {
    row(
        family,
        op,
        KeyClass::Any,
        SymbolSpec::Literal(symbol),
        params,
        result,
    )
}

const fn pat(prefix: &'static str, middle: &'static str, suffix_first: bool) -> SymbolSpec {
    SymbolSpec::Pattern {
        prefix,
        middle,
        suffix_first,
    }
}

const fn lit(symbol: &'static str) -> SymbolSpec {
    SymbolSpec::Literal(symbol)
}

const fn split(lookup: &'static str, declare: &'static str) -> SymbolSpec {
    SymbolSpec::Split { lookup, declare }
}

const fn row(
    family: Family,
    op: &'static str,
    class: KeyClass,
    symbol: SymbolSpec,
    params: &'static [AbiTy],
    result: AbiResult,
) -> ImportRow {
    ImportRow {
        family,
        op,
        class,
        symbol,
        params,
        result,
    }
}

/// The name suffix a primary type contributes to a patterned symbol.
/// `String` and `ViewByte` share the `string` instantiations.
pub(crate) fn name_suffix(ty: Ty) -> &'static str {
    match ty {
        Ty::Byte => "u8",
        Ty::UInt16 => "u16",
        Ty::UInt32 => "u32",
        Ty::UInt64 => "u64",
        Ty::Int64 => "i64",
        Ty::Bool => "bool",
        Ty::Unicode => "unicode",
        Ty::Float32 => "f32",
        Ty::Float64 => "f64",
        Ty::String | Ty::ViewByte => "string",
        _ => unreachable!("a suffix was requested for a non-key type"),
    }
}

/// The print-table operation for a type, preserving the free function's
/// rejection diagnostics for unprintable types.
pub(crate) fn print_op(ty: Ty) -> Result<&'static str, String> {
    match ty {
        Ty::Float64 => Ok("f64"),
        Ty::Float32 => Ok("f32"),
        Ty::Int64 => Ok("i64"),
        Ty::Byte => Ok("u8"),
        Ty::UInt16 => Ok("u16"),
        Ty::UInt32 => Ok("u32"),
        Ty::UInt64 => Ok("u64"),
        Ty::Bool => Ok("bool"),
        Ty::String => Ok("string"),
        Ty::Unicode => Ok("unicode"),
        Ty::ViewUnicode => Ok("unicode_view"),
        // Specification 010 section 14 rejects printing a user-defined type and
        // Specification 012 section 10 leaves no standalone `Nil` value at all,
        // so neither reaches lowering.
        Ty::User(_) => Err(internal("a user-defined type reached 'print' lowering")),
        Ty::Nil => Err(internal("a standalone 'Nil' reached 'print' lowering")),
        Ty::Sum(_) => Err(internal("an inline sum type reached 'print' lowering")),
        // Specification 016 section 8.3 rejects direct printing of a box in
        // the checker, so this never reaches lowering.
        Ty::Box(_) => Err(internal("a box type reached 'print' lowering")),
        Ty::ViewByte => Err(internal("a byte view reached 'print' lowering")),
        Ty::Array(_) | Ty::List(_) | Ty::View(_) | Ty::Map(_) | Ty::Set(_) => {
            Err(internal("a collection reached 'print' lowering"))
        }
    }
}

#[cfg(test)]
#[path = "symbols_tests.rs"]
mod tests;
