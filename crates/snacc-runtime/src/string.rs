//! Owned UTF-8 strings and their conversions.

use super::*;

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_new(ptr: *const u8, len: usize) -> SnaccString {
    if len == 0 {
        return SnaccString {
            ptr: std::ptr::NonNull::<u8>::dangling().as_ptr(),
            len: 0,
            cap: 0,
        };
    }
    // Safety: callers pass a compiler-created global or another validated
    // string range with at least `len` readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let mut owned = Vec::with_capacity(len);
    owned.extend_from_slice(bytes);
    let result = SnaccString {
        ptr: owned.as_mut_ptr(),
        len,
        cap: owned.capacity(),
    };
    std::mem::forget(owned);
    result
}

/// Pointer-output form used by LLVM lowering because a three-word descriptor
/// has an indirect return convention on the Windows C ABI.
#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_new_out(out: *mut SnaccString, ptr: *const u8, len: usize) {
    let out = unsafe {
        out.as_mut()
            .expect("String construction received a null output")
    };
    *out = snacc_string_new(ptr, len);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_clone(value: SnaccString) -> SnaccString {
    snacc_string_new(value.ptr, value.len)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_clone_out(out: *mut SnaccString, value: *const SnaccString) {
    let out = unsafe { out.as_mut().expect("String clone received a null output") };
    let value = unsafe { value.as_ref().expect("String clone received a null input") };
    *out = snacc_string_clone(*value);
}

const CONCAT_TEXT: u64 = 0;
const CONCAT_I64: u64 = 1;
const CONCAT_U8: u64 = 2;
const CONCAT_U16: u64 = 3;
const CONCAT_U32: u64 = 4;
const CONCAT_U64: u64 = 5;
const CONCAT_F32: u64 = 6;
const CONCAT_F64: u64 = 7;
const CONCAT_BOOL: u64 = 8;
const CONCAT_UNICODE: u64 = 9;

struct StackText {
    bytes: [u8; 64],
    len: usize,
}

impl StackText {
    fn new() -> Self {
        Self {
            bytes: [0; 64],
            len: 0,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl std::fmt::Write for StackText {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(std::fmt::Error)?;
        let destination = self.bytes.get_mut(self.len..end).ok_or(std::fmt::Error)?;
        destination.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn scalar_concat_text(part: SnaccConcatPart) -> StackText {
    use std::fmt::Write as _;

    let mut text = StackText::new();
    let result = match part.tag {
        CONCAT_I64 => write!(&mut text, "{}", part.first as i64),
        CONCAT_U8 => write!(&mut text, "{}", part.first as u8),
        CONCAT_U16 => write!(&mut text, "{}", part.first as u16),
        CONCAT_U32 => write!(&mut text, "{}", part.first as u32),
        CONCAT_U64 => write!(&mut text, "{}", { part.first }),
        CONCAT_F32 => write!(&mut text, "{}", f32::from_bits(part.first as u32)),
        CONCAT_F64 => write!(&mut text, "{}", f64::from_bits(part.first)),
        CONCAT_BOOL => write!(&mut text, "{}", part.first != 0),
        CONCAT_UNICODE => {
            let scalar = char::from_u32(part.first as u32)
                .unwrap_or_else(|| panic!("snacc: invalid Unicode scalar in concatenation"));
            write!(&mut text, "{scalar}")
        }
        _ => panic!("snacc: invalid scalar concatenation tag"),
    };
    result.unwrap_or_else(|_| panic!("snacc: scalar formatting exceeded its fixed buffer"));
    text
}

unsafe fn concat_part_bytes<'a>(part: SnaccConcatPart) -> Option<&'a [u8]> {
    if part.tag != CONCAT_TEXT {
        return None;
    }
    // Safety: compiler lowering constructs text parts from a live String,
    // Unicode view, or global literal and retains every owner through this
    // call. The descriptor carries exactly `second` readable UTF-8 bytes.
    Some(unsafe {
        std::slice::from_raw_parts(part.first as usize as *const u8, part.second as usize)
    })
}

/// Builds a complete concat/interpolation plan with exactly one allocation for
/// the resulting String. Scalar formatting uses bounded stack storage.
#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_concat_parts_out(
    out: *mut SnaccString,
    parts: *const SnaccConcatPart,
    count: usize,
) {
    let out = unsafe {
        out.as_mut()
            .expect("String concatenation received a null output")
    };
    // Safety: lowering passes an array of exactly `count` initialized parts.
    let parts = unsafe { std::slice::from_raw_parts(parts, count) };
    let mut total = 0usize;
    for part in parts {
        // Safety: every text part points to storage retained through this call.
        let length = match unsafe { concat_part_bytes(*part) } {
            Some(bytes) => bytes.len(),
            None => scalar_concat_text(*part).len,
        };
        total = total
            .checked_add(length)
            .unwrap_or_else(|| panic!("snacc: string length overflow"));
    }
    if total == 0 {
        *out = SnaccString {
            ptr: std::ptr::NonNull::<u8>::dangling().as_ptr(),
            len: 0,
            cap: 0,
        };
        return;
    }
    let mut owned = Vec::with_capacity(total);
    for part in parts {
        // Safety: every text part points to storage retained through this call.
        match unsafe { concat_part_bytes(*part) } {
            Some(bytes) => owned.extend_from_slice(bytes),
            None => owned.extend_from_slice(scalar_concat_text(*part).as_bytes()),
        }
    }
    debug_assert_eq!(owned.len(), total);
    *out = SnaccString {
        ptr: owned.as_mut_ptr(),
        len: owned.len(),
        cap: owned.capacity(),
    };
    std::mem::forget(owned);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_equal(left: SnaccString, right: SnaccString) -> u8 {
    // Safety: descriptors are validated live string ranges.
    let left_bytes = unsafe { std::slice::from_raw_parts(left.ptr, left.len) };
    let right_bytes = unsafe { std::slice::from_raw_parts(right.ptr, right.len) };
    u8::from(left_bytes == right_bytes)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_equal_ptr(
    left: *const SnaccString,
    right: *const SnaccString,
) -> u8 {
    let left = unsafe { left.as_ref().expect("String equality received a null left") };
    let right = unsafe {
        right
            .as_ref()
            .expect("String equality received a null right")
    };
    snacc_string_equal(*left, *right)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_drop(value: SnaccString) {
    if value.cap == 0 {
        return;
    }
    // Safety: this descriptor was produced by the string runtime and has not
    // previously been dropped; capacity and length are the original Vec facts.
    unsafe {
        drop(Vec::from_raw_parts(value.ptr, value.len, value.cap));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_drop_ptr(value: *const SnaccString) {
    let Some(value) = (unsafe { value.as_ref() }) else {
        return;
    };
    snacc_string_drop(*value);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_bytes(value: SnaccString) -> SnaccView {
    SnaccView {
        ptr: value.ptr,
        len: value.len,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_bytes_out(out: *mut SnaccView, value: *const SnaccString) {
    let out = unsafe { out.as_mut().expect("String view received a null output") };
    let value = unsafe { value.as_ref().expect("String view received a null input") };
    *out = snacc_string_bytes(*value);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_unicode(value: SnaccString) -> SnaccView {
    snacc_string_bytes(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_unicode_out(out: *mut SnaccView, value: *const SnaccString) {
    let out = unsafe { out.as_mut().expect("String view received a null output") };
    let value = unsafe { value.as_ref().expect("String view received a null input") };
    *out = snacc_string_unicode(*value);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_from_view(value: SnaccView) -> SnaccString {
    // Safety: the Unicode-view constructor accepts only valid UTF-8 ranges.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    snacc_string_new(bytes.as_ptr(), bytes.len())
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_from_view_out(out: *mut SnaccString, value: *const SnaccView) {
    let out = unsafe {
        out.as_mut()
            .expect("String conversion received a null output")
    };
    let value = unsafe {
        value
            .as_ref()
            .expect("String conversion received a null view")
    };
    *out = snacc_string_from_view(*value);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_from_utf8(value: SnaccView) -> SnaccString {
    // Safety: this view is a compiler-created range; validation happens before
    // the bytes are copied into the new owner.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    if std::str::from_utf8(bytes).is_err() {
        return SnaccString {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        };
    }
    snacc_string_new(bytes.as_ptr(), bytes.len())
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_string_from_utf8_out(out: *mut SnaccString, value: *const SnaccView) {
    let out = unsafe {
        out.as_mut()
            .expect("UTF-8 conversion received a null output")
    };
    let value = unsafe {
        value
            .as_ref()
            .expect("UTF-8 conversion received a null view")
    };
    *out = snacc_string_from_utf8(*value);
}

#[doc(hidden)]
pub fn force_link_string() {
    let symbols = [
        snacc_string_new as *const () as usize,
        snacc_string_new_out as *const () as usize,
        snacc_string_clone as *const () as usize,
        snacc_string_clone_out as *const () as usize,
        snacc_string_concat_parts_out as *const () as usize,
        snacc_string_equal as *const () as usize,
        snacc_string_equal_ptr as *const () as usize,
        snacc_string_drop as *const () as usize,
        snacc_string_drop_ptr as *const () as usize,
        snacc_string_bytes as *const () as usize,
        snacc_string_bytes_out as *const () as usize,
        snacc_string_unicode as *const () as usize,
        snacc_string_unicode_out as *const () as usize,
        snacc_string_from_view as *const () as usize,
        snacc_string_from_view_out as *const () as usize,
        snacc_string_from_utf8 as *const () as usize,
        snacc_string_from_utf8_out as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
#[cfg(test)]
pub(crate) const STRING_SYMBOLS: &[&str] = &[
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
];
