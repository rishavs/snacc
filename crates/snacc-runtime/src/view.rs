//! Borrowed byte and Unicode views over string storage.

use super::*;

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_byte_length(value: SnaccView) -> i64 {
    i64::try_from(value.len).unwrap_or_else(|_| panic!("snacc: view length overflow"))
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_byte_length_ptr(value: *const SnaccView) -> i64 {
    let value = unsafe { value.as_ref().expect("View length received a null view") };
    snacc_view_byte_length(*value)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_unicode_length(value: SnaccView) -> i64 {
    // Safety: a Unicode view is created only from a valid SnaccString range.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    let text = std::str::from_utf8(bytes).unwrap_or("�");
    i64::try_from(text.chars().count()).unwrap_or_else(|_| panic!("snacc: view length overflow"))
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_unicode_length_ptr(value: *const SnaccView) -> i64 {
    let value = unsafe { value.as_ref().expect("View length received a null view") };
    snacc_view_unicode_length(*value)
}

/// Compares two borrowed views by their encoded element sequence. Both view
/// families use UTF-8 storage in the current runtime, so byte equality is
/// also scalar-sequence equality for Unicode views.
#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_equal(left: SnaccView, right: SnaccView) -> u8 {
    // Safety: views are created only from live string ranges by the compiler.
    let left_bytes = unsafe { std::slice::from_raw_parts(left.ptr, left.len) };
    let right_bytes = unsafe { std::slice::from_raw_parts(right.ptr, right.len) };
    u8::from(left_bytes == right_bytes)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_equal_ptr(left: *const SnaccView, right: *const SnaccView) -> u8 {
    let left = unsafe { left.as_ref().expect("View equality received a null left") };
    let right = unsafe { right.as_ref().expect("View equality received a null right") };
    snacc_view_equal(*left, *right)
}

/// Returns the byte at `index`, or `-1` for a negative/out-of-range index.
#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_byte_at(value: SnaccView, index: i64) -> i64 {
    if index < 0 {
        return -1;
    }
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    if index >= value.len {
        return -1;
    }
    // Safety: the checked compiler/runtime view invariant supplies this range.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    i64::from(bytes[index])
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_byte_at_ptr(value: *const SnaccView, index: i64) -> i64 {
    let value = unsafe { value.as_ref().expect("View lookup received a null view") };
    snacc_view_byte_at(*value, index)
}

/// Returns the Unicode scalar at `index`, or `-1` for a negative/out-of-range
/// index. The scan is intentionally linear because UTF-8 scalars are variable
/// width.
#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_unicode_at(value: SnaccView, index: i64) -> i64 {
    if index < 0 {
        return -1;
    }
    // Safety: a Unicode view is created only from a valid SnaccString range.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    let Some(scalar) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.chars().nth(usize::try_from(index).ok()?))
    else {
        return -1;
    };
    i64::from(u32::from(scalar))
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_unicode_at_ptr(value: *const SnaccView, index: i64) -> i64 {
    let value = unsafe { value.as_ref().expect("View lookup received a null view") };
    snacc_view_unicode_at(*value, index)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_byte_slice(value: SnaccView, start: i64, end: i64) -> SnaccView {
    if start < 0 || end < start {
        return SnaccView {
            ptr: std::ptr::null(),
            len: 0,
        };
    }
    let start = usize::try_from(start).unwrap_or(usize::MAX);
    let end = usize::try_from(end).unwrap_or(usize::MAX);
    if end > value.len {
        return SnaccView {
            ptr: std::ptr::null(),
            len: 0,
        };
    }
    // Safety: the compiler-created view owns a valid readable range.
    let ptr = unsafe { value.ptr.add(start) };
    SnaccView {
        ptr,
        len: end - start,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_byte_slice_out(
    out: *mut SnaccView,
    value: *const SnaccView,
    start: i64,
    end: i64,
) {
    let out = unsafe { out.as_mut().expect("View slice received a null output") };
    let value = unsafe { value.as_ref().expect("View slice received a null input") };
    *out = snacc_view_byte_slice(*value, start, end);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_unicode_slice(value: SnaccView, start: i64, end: i64) -> SnaccView {
    if start < 0 || end < start {
        return SnaccView {
            ptr: std::ptr::null(),
            len: 0,
        };
    }
    let start = usize::try_from(start).unwrap_or(usize::MAX);
    let end = usize::try_from(end).unwrap_or(usize::MAX);
    // Safety: a Unicode view is created only from valid UTF-8 storage.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    let Ok(text) = std::str::from_utf8(bytes) else {
        return SnaccView {
            ptr: std::ptr::null(),
            len: 0,
        };
    };
    let mut offsets = text
        .char_indices()
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    offsets.push(bytes.len());
    if end > offsets.len() - 1 {
        return SnaccView {
            ptr: std::ptr::null(),
            len: 0,
        };
    }
    let byte_start = offsets[start];
    let byte_end = offsets[end];
    // Safety: both offsets came from the valid UTF-8 range above.
    let ptr = unsafe { value.ptr.add(byte_start) };
    SnaccView {
        ptr,
        len: byte_end - byte_start,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_view_unicode_slice_out(
    out: *mut SnaccView,
    value: *const SnaccView,
    start: i64,
    end: i64,
) {
    let out = unsafe { out.as_mut().expect("View slice received a null output") };
    let value = unsafe { value.as_ref().expect("View slice received a null input") };
    *out = snacc_view_unicode_slice(*value, start, end);
}

#[doc(hidden)]
pub fn force_link_view() {
    let symbols = [
        snacc_view_byte_length as *const () as usize,
        snacc_view_byte_length_ptr as *const () as usize,
        snacc_view_unicode_length as *const () as usize,
        snacc_view_unicode_length_ptr as *const () as usize,
        snacc_view_equal as *const () as usize,
        snacc_view_equal_ptr as *const () as usize,
        snacc_view_byte_at as *const () as usize,
        snacc_view_byte_at_ptr as *const () as usize,
        snacc_view_unicode_at as *const () as usize,
        snacc_view_unicode_at_ptr as *const () as usize,
        snacc_view_byte_slice as *const () as usize,
        snacc_view_byte_slice_out as *const () as usize,
        snacc_view_unicode_slice as *const () as usize,
        snacc_view_unicode_slice_out as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
#[cfg(test)]
pub(crate) const VIEW_SYMBOLS: &[&str] = &[
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
];
