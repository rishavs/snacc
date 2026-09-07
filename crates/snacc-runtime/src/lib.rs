//! Stable Rust-side ABI used by generated Snacc objects.
//!
//! The ABI vocabulary (descriptors) and shared byte helpers live here; each
//! collection family lives in its own module. `force_link` aggregates the
//! per-module link anchors so hosted builds keep every runtime symbol.

mod fail;
mod list;
mod map;
mod print;
mod set;
mod string;
mod view;

pub use fail::*;
pub use list::*;
pub use map::*;
pub use print::*;
pub use set::*;
pub use string::*;
pub use view::*;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SnaccString {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SnaccView {
    pub ptr: *const u8,
    pub len: usize,
}

/// Erased, borrow-only input to one compiler-canonicalized concatenation.
/// `first` and `second` contain either a pointer/length pair or the exact bits
/// of one scalar selected by `tag`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SnaccConcatPart {
    pub tag: u64,
    pub first: u64,
    pub second: u64,
}

/// The private descriptor used by compiler-generated `List<T>` values. Its
/// layout mirrors the compiler's `{ pointer, length, capacity }` collection
/// representation on every supported target.
#[repr(C)]
pub struct SnaccList {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

/// Private descriptors for compiler-owned hash collections. The pointed-to
/// allocation is one concrete Rust `HashMap`/`HashSet` selected by the
/// compiler for the collection's key and value types; the descriptor itself
/// is the only representation crossing the LLVM/runtime boundary.
#[repr(C)]
pub struct SnaccMap {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

#[repr(C)]
pub struct SnaccSet {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

/// Contract version for generated objects, runtime imports, and Rust bridges.
pub const ABI_VERSION: u32 = 12;

/// Copies an opaque compiler-owned value between aligned storage and a byte
/// vector. The runtime never interprets the value or runs its destructor;
/// typed ownership remains with compiler-generated lowering.
unsafe fn copy_raw_bytes(src: *const u8, dst: *mut u8, size: usize) {
    if size != 0 {
        // Safety: callers provide initialized, non-overlapping ranges of the
        // exact byte count represented by the checked value type.
        unsafe { std::ptr::copy_nonoverlapping(src, dst, size) };
    }
}

pub(crate) fn view_text(value: SnaccView) -> Option<&'static str> {
    // The returned reference is used only for the duration of the caller's
    // operation. The runtime cannot express that relationship in this C ABI,
    // so the helper is kept private and never stores the reference.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    std::str::from_utf8(bytes).ok()
}

pub(crate) fn take_string(value: SnaccString) -> String {
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    let text = String::from_utf8(bytes.to_vec()).expect("SnaccString must contain UTF-8");
    snacc_string_drop(value);
    text
}

pub(crate) fn sync_map(map: &mut SnaccMap, len: usize, cap: usize) {
    map.len = len;
    map.cap = cap;
}

pub(crate) fn sync_set(set: &mut SnaccSet, len: usize, cap: usize) {
    set.len = len;
    set.cap = cap;
}

pub(crate) fn reserve_target(minimum: i64, len: usize) -> usize {
    if minimum < 0 {
        snacc_collection_bounds_fail();
    }
    usize::try_from(minimum)
        .unwrap_or(usize::MAX)
        .saturating_sub(len)
}

#[doc(hidden)]
#[inline(never)]
pub fn force_link() {
    force_link_print();
    force_link_string();
    force_link_view();
    force_link_list();
    force_link_map();
    force_link_set();
    force_link_fail();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exported symbol set is identical across the Phase 5 rewrite: every
    /// per-module symbol list, sorted, matches this golden list taken from
    /// the pre-split `force_link`. A new runtime symbol fails here until its
    /// name is recorded below, keeping the addition reviewed.
    #[test]
    fn exported_symbol_set_is_unchanged() {
        let mut actual: Vec<&str> = Vec::new();
        actual.extend_from_slice(PRINT_SYMBOLS);
        actual.extend_from_slice(STRING_SYMBOLS);
        actual.extend_from_slice(VIEW_SYMBOLS);
        actual.extend_from_slice(FAIL_SYMBOLS);
        actual.extend_from_slice(LIST_PUSH_SYMBOLS_I64);
        actual.extend_from_slice(LIST_PUSH_SYMBOLS_U8);
        actual.extend_from_slice(LIST_PUSH_SYMBOLS_U16);
        actual.extend_from_slice(LIST_PUSH_SYMBOLS_U32);
        actual.extend_from_slice(LIST_PUSH_SYMBOLS_U64);
        actual.extend_from_slice(LIST_PUSH_SYMBOLS_F32);
        actual.extend_from_slice(LIST_PUSH_SYMBOLS_F64);
        actual.extend_from_slice(LIST_PUSH_SYMBOLS_BOOL);
        actual.extend_from_slice(LIST_PUSH_SYMBOLS_UNICODE);
        actual.extend_from_slice(LIST_SCALAR_SYMBOLS_I64);
        actual.extend_from_slice(LIST_SCALAR_SYMBOLS_U8);
        actual.extend_from_slice(LIST_SCALAR_SYMBOLS_U16);
        actual.extend_from_slice(LIST_SCALAR_SYMBOLS_U32);
        actual.extend_from_slice(LIST_SCALAR_SYMBOLS_U64);
        actual.extend_from_slice(LIST_SCALAR_SYMBOLS_F32);
        actual.extend_from_slice(LIST_SCALAR_SYMBOLS_F64);
        actual.extend_from_slice(LIST_SCALAR_SYMBOLS_BOOL);
        actual.extend_from_slice(LIST_SCALAR_SYMBOLS_UNICODE);
        actual.extend_from_slice(LIST_FIXED_SYMBOLS);
        actual.extend_from_slice(LIST_RAW_SYMBOLS);
        actual.extend_from_slice(MAP_I64_SYMBOLS_U8);
        actual.extend_from_slice(MAP_I64_SYMBOLS_U16);
        actual.extend_from_slice(MAP_I64_SYMBOLS_U32);
        actual.extend_from_slice(MAP_I64_SYMBOLS_U64);
        actual.extend_from_slice(MAP_I64_SYMBOLS_BOOL);
        actual.extend_from_slice(MAP_I64_SYMBOLS_UNICODE);
        actual.extend_from_slice(MAP_RAW_SYMBOLS_U8);
        actual.extend_from_slice(MAP_RAW_SYMBOLS_U16);
        actual.extend_from_slice(MAP_RAW_SYMBOLS_U32);
        actual.extend_from_slice(MAP_RAW_SYMBOLS_U64);
        actual.extend_from_slice(MAP_RAW_SYMBOLS_BOOL);
        actual.extend_from_slice(MAP_RAW_SYMBOLS_UNICODE);
        actual.extend_from_slice(MAP_RAW_SYMBOLS_I64);
        actual.extend_from_slice(MAP_RAW_SYMBOLS_STRING);
        actual.extend_from_slice(MAP_STRING_I64_SYMBOLS);
        actual.extend_from_slice(MAP_I64_I64_SYMBOLS);
        actual.extend_from_slice(SET_SYMBOLS_U8);
        actual.extend_from_slice(SET_SYMBOLS_U16);
        actual.extend_from_slice(SET_SYMBOLS_U32);
        actual.extend_from_slice(SET_SYMBOLS_U64);
        actual.extend_from_slice(SET_SYMBOLS_BOOL);
        actual.extend_from_slice(SET_SYMBOLS_UNICODE);
        actual.extend_from_slice(SET_STRING_SYMBOLS);
        actual.extend_from_slice(SET_I64_SYMBOLS);
        actual.sort_unstable();

        let mut expected: Vec<&str> = vec![
            "snacc_print_f64",
            "snacc_print_i64",
            "snacc_print_bool",
            "snacc_print_u8",
            "snacc_print_u16",
            "snacc_print_u32",
            "snacc_print_u64",
            "snacc_print_f32",
            "snacc_print_unicode",
            "snacc_print_string",
            "snacc_print_string_ptr",
            "snacc_print_unicode_view",
            "snacc_string_new",
            "snacc_string_new_out",
            "snacc_string_clone",
            "snacc_string_clone_out",
            "snacc_string_concat_parts_out",
            "snacc_string_equal",
            "snacc_string_equal_ptr",
            "snacc_string_drop",
            "snacc_string_drop_ptr",
            "snacc_string_bytes",
            "snacc_string_bytes_out",
            "snacc_string_unicode",
            "snacc_string_unicode_out",
            "snacc_string_from_view",
            "snacc_string_from_view_out",
            "snacc_string_from_utf8",
            "snacc_string_from_utf8_out",
            "snacc_view_byte_length",
            "snacc_view_byte_length_ptr",
            "snacc_view_unicode_length",
            "snacc_view_unicode_length_ptr",
            "snacc_view_equal",
            "snacc_view_equal_ptr",
            "snacc_view_byte_at",
            "snacc_view_byte_at_ptr",
            "snacc_view_unicode_at",
            "snacc_view_unicode_at_ptr",
            "snacc_view_byte_slice",
            "snacc_view_byte_slice_out",
            "snacc_view_unicode_slice",
            "snacc_view_unicode_slice_out",
            "snacc_invalid_floating_operation",
            "snacc_collection_bounds_fail",
            "snacc_alloc",
            "snacc_dealloc",
            "snacc_list_push_i64",
            "snacc_list_push_u8",
            "snacc_list_push_u16",
            "snacc_list_push_u32",
            "snacc_list_push_u64",
            "snacc_list_push_f32",
            "snacc_list_push_f64",
            "snacc_list_push_bool",
            "snacc_list_push_unicode",
            "snacc_list_pop_i64",
            "snacc_list_insert_i64",
            "snacc_list_remove_i64",
            "snacc_list_pop_u8",
            "snacc_list_insert_u8",
            "snacc_list_remove_u8",
            "snacc_list_pop_u16",
            "snacc_list_insert_u16",
            "snacc_list_remove_u16",
            "snacc_list_pop_u32",
            "snacc_list_insert_u32",
            "snacc_list_remove_u32",
            "snacc_list_pop_u64",
            "snacc_list_insert_u64",
            "snacc_list_remove_u64",
            "snacc_list_pop_f32",
            "snacc_list_insert_f32",
            "snacc_list_remove_f32",
            "snacc_list_pop_f64",
            "snacc_list_insert_f64",
            "snacc_list_remove_f64",
            "snacc_list_pop_bool",
            "snacc_list_insert_bool",
            "snacc_list_remove_bool",
            "snacc_list_pop_unicode",
            "snacc_list_insert_unicode",
            "snacc_list_remove_unicode",
            "snacc_list_clear",
            "snacc_list_reserve",
            "snacc_list_push_raw",
            "snacc_list_pop_raw",
            "snacc_list_insert_raw",
            "snacc_list_remove_raw",
            "snacc_list_clear_raw",
            "snacc_map_u8_i64_insert",
            "snacc_map_u8_i64_contains",
            "snacc_map_u8_i64_index",
            "snacc_map_u8_i64_key_at",
            "snacc_map_u8_i64_value_at",
            "snacc_map_u8_i64_delete",
            "snacc_map_u8_i64_take",
            "snacc_map_u8_i64_clear",
            "snacc_map_u8_i64_reserve",
            "snacc_map_u8_i64_drop",
            "snacc_map_u16_i64_insert",
            "snacc_map_u16_i64_contains",
            "snacc_map_u16_i64_index",
            "snacc_map_u16_i64_key_at",
            "snacc_map_u16_i64_value_at",
            "snacc_map_u16_i64_delete",
            "snacc_map_u16_i64_take",
            "snacc_map_u16_i64_clear",
            "snacc_map_u16_i64_reserve",
            "snacc_map_u16_i64_drop",
            "snacc_map_u32_i64_insert",
            "snacc_map_u32_i64_contains",
            "snacc_map_u32_i64_index",
            "snacc_map_u32_i64_key_at",
            "snacc_map_u32_i64_value_at",
            "snacc_map_u32_i64_delete",
            "snacc_map_u32_i64_take",
            "snacc_map_u32_i64_clear",
            "snacc_map_u32_i64_reserve",
            "snacc_map_u32_i64_drop",
            "snacc_map_u64_i64_insert",
            "snacc_map_u64_i64_contains",
            "snacc_map_u64_i64_index",
            "snacc_map_u64_i64_key_at",
            "snacc_map_u64_i64_value_at",
            "snacc_map_u64_i64_delete",
            "snacc_map_u64_i64_take",
            "snacc_map_u64_i64_clear",
            "snacc_map_u64_i64_reserve",
            "snacc_map_u64_i64_drop",
            "snacc_map_bool_i64_insert",
            "snacc_map_bool_i64_contains",
            "snacc_map_bool_i64_index",
            "snacc_map_bool_i64_key_at",
            "snacc_map_bool_i64_value_at",
            "snacc_map_bool_i64_delete",
            "snacc_map_bool_i64_take",
            "snacc_map_bool_i64_clear",
            "snacc_map_bool_i64_reserve",
            "snacc_map_bool_i64_drop",
            "snacc_map_unicode_i64_insert",
            "snacc_map_unicode_i64_contains",
            "snacc_map_unicode_i64_index",
            "snacc_map_unicode_i64_key_at",
            "snacc_map_unicode_i64_value_at",
            "snacc_map_unicode_i64_delete",
            "snacc_map_unicode_i64_take",
            "snacc_map_unicode_i64_clear",
            "snacc_map_unicode_i64_reserve",
            "snacc_map_unicode_i64_drop",
            "snacc_map_u8_raw_insert",
            "snacc_map_u8_raw_contains",
            "snacc_map_u8_raw_index",
            "snacc_map_u8_raw_key_at",
            "snacc_map_u8_raw_value_at",
            "snacc_map_u8_raw_delete",
            "snacc_map_u8_raw_take",
            "snacc_map_u8_raw_clear",
            "snacc_map_u8_raw_reserve",
            "snacc_map_u8_raw_drop",
            "snacc_map_u16_raw_insert",
            "snacc_map_u16_raw_contains",
            "snacc_map_u16_raw_index",
            "snacc_map_u16_raw_key_at",
            "snacc_map_u16_raw_value_at",
            "snacc_map_u16_raw_delete",
            "snacc_map_u16_raw_take",
            "snacc_map_u16_raw_clear",
            "snacc_map_u16_raw_reserve",
            "snacc_map_u16_raw_drop",
            "snacc_map_u32_raw_insert",
            "snacc_map_u32_raw_contains",
            "snacc_map_u32_raw_index",
            "snacc_map_u32_raw_key_at",
            "snacc_map_u32_raw_value_at",
            "snacc_map_u32_raw_delete",
            "snacc_map_u32_raw_take",
            "snacc_map_u32_raw_clear",
            "snacc_map_u32_raw_reserve",
            "snacc_map_u32_raw_drop",
            "snacc_map_u64_raw_insert",
            "snacc_map_u64_raw_contains",
            "snacc_map_u64_raw_index",
            "snacc_map_u64_raw_key_at",
            "snacc_map_u64_raw_value_at",
            "snacc_map_u64_raw_delete",
            "snacc_map_u64_raw_take",
            "snacc_map_u64_raw_clear",
            "snacc_map_u64_raw_reserve",
            "snacc_map_u64_raw_drop",
            "snacc_map_bool_raw_insert",
            "snacc_map_bool_raw_contains",
            "snacc_map_bool_raw_index",
            "snacc_map_bool_raw_key_at",
            "snacc_map_bool_raw_value_at",
            "snacc_map_bool_raw_delete",
            "snacc_map_bool_raw_take",
            "snacc_map_bool_raw_clear",
            "snacc_map_bool_raw_reserve",
            "snacc_map_bool_raw_drop",
            "snacc_map_unicode_raw_insert",
            "snacc_map_unicode_raw_contains",
            "snacc_map_unicode_raw_index",
            "snacc_map_unicode_raw_key_at",
            "snacc_map_unicode_raw_value_at",
            "snacc_map_unicode_raw_delete",
            "snacc_map_unicode_raw_take",
            "snacc_map_unicode_raw_clear",
            "snacc_map_unicode_raw_reserve",
            "snacc_map_unicode_raw_drop",
            "snacc_map_i64_raw_insert",
            "snacc_map_i64_raw_contains",
            "snacc_map_i64_raw_index",
            "snacc_map_i64_raw_key_at",
            "snacc_map_i64_raw_value_at",
            "snacc_map_i64_raw_delete",
            "snacc_map_i64_raw_take",
            "snacc_map_i64_raw_clear",
            "snacc_map_i64_raw_reserve",
            "snacc_map_i64_raw_drop",
            "snacc_map_string_raw_insert",
            "snacc_map_string_raw_contains",
            "snacc_map_string_raw_index",
            "snacc_map_string_raw_key_at",
            "snacc_map_string_raw_value_at",
            "snacc_map_string_raw_delete",
            "snacc_map_string_raw_take",
            "snacc_map_string_raw_clear",
            "snacc_map_string_raw_reserve",
            "snacc_map_string_raw_drop",
            "snacc_map_string_i64_insert",
            "snacc_map_string_i64_contains",
            "snacc_map_string_i64_key_at",
            "snacc_map_string_i64_key_at_out",
            "snacc_map_string_i64_index",
            "snacc_map_string_i64_delete",
            "snacc_map_string_i64_take",
            "snacc_map_string_i64_clear",
            "snacc_map_string_i64_reserve",
            "snacc_map_string_i64_drop",
            "snacc_map_i64_i64_insert",
            "snacc_map_i64_i64_contains",
            "snacc_map_i64_i64_index",
            "snacc_map_i64_i64_key_at",
            "snacc_map_i64_i64_value_at",
            "snacc_map_i64_i64_delete",
            "snacc_map_i64_i64_take",
            "snacc_map_i64_i64_clear",
            "snacc_map_i64_i64_reserve",
            "snacc_map_i64_i64_drop",
            "snacc_set_u8_insert",
            "snacc_set_u8_contains",
            "snacc_set_u8_at",
            "snacc_set_u8_delete",
            "snacc_set_u8_clear",
            "snacc_set_u8_reserve",
            "snacc_set_u8_drop",
            "snacc_set_u16_insert",
            "snacc_set_u16_contains",
            "snacc_set_u16_at",
            "snacc_set_u16_delete",
            "snacc_set_u16_clear",
            "snacc_set_u16_reserve",
            "snacc_set_u16_drop",
            "snacc_set_u32_insert",
            "snacc_set_u32_contains",
            "snacc_set_u32_at",
            "snacc_set_u32_delete",
            "snacc_set_u32_clear",
            "snacc_set_u32_reserve",
            "snacc_set_u32_drop",
            "snacc_set_u64_insert",
            "snacc_set_u64_contains",
            "snacc_set_u64_at",
            "snacc_set_u64_delete",
            "snacc_set_u64_clear",
            "snacc_set_u64_reserve",
            "snacc_set_u64_drop",
            "snacc_set_bool_insert",
            "snacc_set_bool_contains",
            "snacc_set_bool_at",
            "snacc_set_bool_delete",
            "snacc_set_bool_clear",
            "snacc_set_bool_reserve",
            "snacc_set_bool_drop",
            "snacc_set_unicode_insert",
            "snacc_set_unicode_contains",
            "snacc_set_unicode_at",
            "snacc_set_unicode_delete",
            "snacc_set_unicode_clear",
            "snacc_set_unicode_reserve",
            "snacc_set_unicode_drop",
            "snacc_set_string_insert",
            "snacc_set_string_contains",
            "snacc_set_string_at",
            "snacc_set_string_at_out",
            "snacc_set_string_delete",
            "snacc_set_string_clear",
            "snacc_set_string_reserve",
            "snacc_set_string_drop",
            "snacc_set_i64_insert",
            "snacc_set_i64_contains",
            "snacc_set_i64_at",
            "snacc_set_i64_delete",
            "snacc_set_i64_clear",
            "snacc_set_i64_reserve",
            "snacc_set_i64_drop",
        ];
        expected.sort_unstable();
        assert_eq!(actual, expected);
    }
}
