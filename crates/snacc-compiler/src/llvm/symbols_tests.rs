//! Tests for the runtime import table (Specification 029 Phase 4).

use super::*;

/// Every row's symbol resolves without validation errors for a
/// representative primary type, so no lowering path can miss the table.
#[test]
fn every_table_row_resolves() {
    let scalars = [
        Ty::Byte,
        Ty::UInt16,
        Ty::UInt32,
        Ty::UInt64,
        Ty::Int64,
        Ty::Bool,
        Ty::Unicode,
    ];
    for row in IMPORTS {
        let primaries: &[Ty] = match (row.family, row.class) {
            (Family::Print, _)
            | (Family::Fail, _)
            | (Family::ListRaw, _)
            | (Family::ListFixed, _)
            | (Family::String, _)
            | (Family::View, _) => &[Ty::Nil],
            (_, KeyClass::String) => &[Ty::String],
            (_, KeyClass::View) => &[Ty::ViewByte],
            (Family::List, KeyClass::Scalar) => &[Ty::Int64, Ty::Float32],
            (_, KeyClass::Scalar) => &scalars,
            (_, KeyClass::Any) => &[Ty::Nil],
        };
        for primary in primaries {
            let found = import_row(row.family, row.op, *primary)
                .unwrap_or_else(|error| panic!("row {:?} {:?} missing: {error}", row.op, primary));
            assert_eq!(found.op, row.op);
        }
    }
}

/// Patterned symbols keep the exact historical names.
#[test]
fn patterned_symbols_match_history() {
    let cases = [
        (Family::Map, "insert", Ty::Byte, "snacc_map_u8_i64_insert"),
        (Family::Map, "key_at", Ty::Int64, "snacc_map_i64_i64_key_at"),
        (
            Family::Map,
            "key_at",
            Ty::ViewByte,
            "snacc_map_string_i64_key_at",
        ),
        (
            Family::MapRaw,
            "drop",
            Ty::Unicode,
            "snacc_map_unicode_raw_drop",
        ),
        (
            Family::MapRaw,
            "contains",
            Ty::String,
            "snacc_map_string_raw_contains",
        ),
        (Family::Set, "at", Ty::Int64, "snacc_set_i64_at"),
        (Family::Set, "at", Ty::ViewByte, "snacc_set_string_at"),
        (Family::Set, "clear", Ty::Int64, "snacc_set_i64_clear"),
        (Family::List, "push", Ty::Float64, "snacc_list_push_f64"),
        (Family::List, "remove", Ty::Bool, "snacc_list_remove_bool"),
    ];
    for (family, op, primary, expected) in cases {
        let row = import_row(family, op, primary).unwrap();
        let SymbolSpec::Pattern {
            prefix,
            middle,
            suffix_first,
        } = row.symbol
        else {
            panic!("expected a patterned row for {op}");
        };
        let suffix = name_suffix(primary);
        let name = if suffix_first {
            if middle.is_empty() {
                format!("snacc_{prefix}_{suffix}_{op}")
            } else {
                format!("snacc_{prefix}_{suffix}_{middle}_{op}")
            }
        } else if middle.is_empty() {
            format!("snacc_{prefix}_{op}_{suffix}")
        } else {
            format!("snacc_{prefix}_{op}_{middle}_{suffix}")
        };
        assert_eq!(name, expected);
    }
}
