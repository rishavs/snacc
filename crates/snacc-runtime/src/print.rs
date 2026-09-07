//! Scalar and string printing.

use super::*;

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_f64(value: f64) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_i64(value: i64) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_bool(value: u8) {
    println!("{}", value != 0);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_u8(value: u8) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_u16(value: u16) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_u32(value: u32) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_u64(value: u64) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_f32(value: f32) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_unicode(value: u32) {
    let scalar = char::from_u32(value).unwrap_or('\u{FFFD}');
    println!("{scalar}");
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_string(value: SnaccString) {
    // Safety: the compiler/runtime string invariant guarantees that `ptr`
    // points to `len` initialized UTF-8 bytes for every live descriptor.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    let text = std::str::from_utf8(bytes).unwrap_or("�");
    println!("{text}");
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_string_ptr(value: *const SnaccString) {
    let Some(value) = (unsafe { value.as_ref() }) else {
        return;
    };
    snacc_print_string(*value);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_print_unicode_view(value: SnaccView) {
    // Safety: the compiler only sends string-backed Unicode views here.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    println!("{}", std::str::from_utf8(bytes).unwrap_or("�"));
}

#[doc(hidden)]
pub fn force_link_print() {
    let symbols = [
        snacc_print_f64 as *const () as usize,
        snacc_print_i64 as *const () as usize,
        snacc_print_bool as *const () as usize,
        snacc_print_u8 as *const () as usize,
        snacc_print_u16 as *const () as usize,
        snacc_print_u32 as *const () as usize,
        snacc_print_u64 as *const () as usize,
        snacc_print_f32 as *const () as usize,
        snacc_print_unicode as *const () as usize,
        snacc_print_string as *const () as usize,
        snacc_print_string_ptr as *const () as usize,
        snacc_print_unicode_view as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
#[cfg(test)]
pub(crate) const PRINT_SYMBOLS: &[&str] = &[
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
];
