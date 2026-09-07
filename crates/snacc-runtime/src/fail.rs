//! Allocation and invalid-operation failure paths.

use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};

/// Terminates execution when a floating-point operation or bridge boundary
/// would expose an IEEE NaN as a Snacc value. The compiler emits an
/// unreachable edge after this call, so returning is not part of the ABI.
#[unsafe(no_mangle)]
pub extern "C" fn snacc_invalid_floating_operation() -> ! {
    panic!("snacc: InvalidFloatingOperation")
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_collection_bounds_fail() -> ! {
    panic!("snacc: collection index out of bounds")
}

/// Specification 016 section 8.2: `box(expression)` lowers to a call to this
/// allocator, which either returns valid, non-null, `align`-aligned storage
/// for `size` bytes or terminates the process -- never a null pointer, an
/// error code, or any other recoverable outcome.
///
/// A zero-sized pointee (Specification 016 section 8.2's closing paragraph)
/// is special-cased to a fixed non-null, well-aligned sentinel rather than
/// calling the global allocator, whose own safety contract forbids a
/// zero-size request (`std::alloc::alloc`'s documentation: "Undefined
/// behavior [...] if layout has zero size"). This is the same
/// never-read-or-written dangling-but-valid convention Rust's own
/// `Layout`/`NonNull::dangling()` use for a zero-sized type, so no real
/// allocation, and therefore no matching `snacc_dealloc` call, ever happens
/// for it -- `snacc_dealloc` mirrors this same `size == 0` special case.
#[unsafe(no_mangle)]
pub extern "C" fn snacc_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return align.max(1) as *mut u8;
    }
    let layout = Layout::from_size_align(size, align).unwrap_or_else(|_| {
        panic!("snacc: invalid allocation request (size {size}, align {align})")
    });
    // Safety: `size` is non-zero, checked above, and `layout` was just built
    // by `Layout::from_size_align`, so it satisfies `alloc`'s only
    // requirement (non-zero size).
    let ptr = unsafe { alloc(layout) };
    if ptr.is_null() {
        handle_alloc_error(layout);
    }
    ptr
}

/// Releases one allocation `snacc_alloc` returned for the exact same `size`
/// and `align` (Specification 016 section 8.1: a box releases its allocation
/// on normal destruction). A `size == 0` request never reaches the global
/// allocator here either, mirroring `snacc_alloc`'s own zero-size sentinel,
/// which must never be passed to `dealloc`.
#[unsafe(no_mangle)]
pub extern "C" fn snacc_dealloc(ptr: *mut u8, size: usize, align: usize) {
    if size == 0 {
        return;
    }
    let layout = Layout::from_size_align(size, align).unwrap_or_else(|_| {
        panic!("snacc: invalid allocation request (size {size}, align {align})")
    });
    // Safety: the checked cleanup plan that emits this call always pairs it
    // with the `snacc_alloc` call that produced `ptr`, using that same
    // pointee's size and alignment, so `ptr` was allocated by the global
    // allocator with this exact `layout` and has not been freed already.
    unsafe { dealloc(ptr, layout) };
}

#[doc(hidden)]
pub fn force_link_fail() {
    let symbols = [
        snacc_invalid_floating_operation as *const () as usize,
        snacc_collection_bounds_fail as *const () as usize,
        snacc_alloc as *const () as usize,
        snacc_dealloc as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
#[cfg(test)]
pub(crate) const FAIL_SYMBOLS: &[&str] = &[
    "snacc_invalid_floating_operation",
    "snacc_collection_bounds_fail",
    "snacc_alloc",
    "snacc_dealloc",
];
