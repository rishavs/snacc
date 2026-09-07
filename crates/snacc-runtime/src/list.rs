//! `List<T>`: scalar fast paths plus byte-moving raw operations.
//!
//! The two macros generate the scalar push/pop/insert/remove entry points,
//! link anchors, and symbol-name lists from a suffix/element pair. The raw
//! operations stay hand-written: they move opaque bytes, never typed values.

use super::*;
use paste::paste;

fn list_reserve_to(
    list: &mut SnaccList,
    minimum: usize,
    element_size: usize,
    element_align: usize,
) {
    if minimum <= list.cap {
        return;
    }
    let mut new_cap = list.cap.max(4);
    while new_cap < minimum {
        new_cap = new_cap
            .checked_mul(2)
            .unwrap_or_else(|| panic!("snacc: list capacity overflow"));
    }
    let new_size = new_cap
        .checked_mul(element_size)
        .unwrap_or_else(|| panic!("snacc: list allocation overflow"));
    let new_ptr = snacc_alloc(new_size, element_align);
    if list.len != 0 {
        // Safety: the old descriptor contains `len` initialized scalar
        // elements, and the new allocation has room for all of them.
        unsafe {
            std::ptr::copy_nonoverlapping(
                list.ptr,
                new_ptr,
                list.len
                    .checked_mul(element_size)
                    .unwrap_or_else(|| panic!("snacc: list copy overflow")),
            );
        }
    }
    if list.cap != 0 {
        let old_size = list
            .cap
            .checked_mul(element_size)
            .unwrap_or_else(|| panic!("snacc: list allocation overflow"));
        snacc_dealloc(list.ptr, old_size, element_align);
    }
    list.ptr = new_ptr;
    list.cap = new_cap;
}

fn list_reserve(list: &mut SnaccList, element_size: usize, element_align: usize) {
    let minimum = list
        .len
        .checked_add(1)
        .unwrap_or_else(|| panic!("snacc: list length overflow"));
    list_reserve_to(list, minimum, element_size, element_align);
}

fn list_push<T: Copy>(list: *mut SnaccList, value: T) {
    if list.is_null() {
        panic!("snacc: List.push received a null descriptor")
    }
    // Safety: the compiler passes the address of a live List descriptor.
    let list = unsafe { &mut *list };
    let element_size = std::mem::size_of::<T>();
    list_reserve(list, element_size, std::mem::align_of::<T>());
    // Safety: reserve established an allocation with enough space and the
    // destination is the next uninitialized scalar slot.
    unsafe {
        std::ptr::write(list.ptr.add(list.len * element_size).cast::<T>(), value);
    }
    list.len += 1;
}

macro_rules! list_push_export {
    ($suffix:ident, $ty:ty) => {
        paste! {
            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_list_push_ $suffix >](list: *mut SnaccList, value: $ty) {
                list_push(list, value);
            }

            #[doc(hidden)]
            pub fn [< force_link_list_push_ $suffix >]() {
                let symbols = [[< snacc_list_push_ $suffix >] as *const () as usize];
                std::hint::black_box(symbols);
            }

            #[doc(hidden)]
            #[cfg(test)]
            pub(crate) const [< LIST_PUSH_SYMBOLS_ $suffix:upper >]: &[&str] =
                &[stringify!([< snacc_list_push_ $suffix >])];
        }
    };
}

list_push_export!(i64, i64);
list_push_export!(u8, u8);
list_push_export!(u16, u16);
list_push_export!(u32, u32);
list_push_export!(u64, u64);
list_push_export!(f32, f32);
list_push_export!(f64, f64);
list_push_export!(bool, u8);
list_push_export!(unicode, u32);

#[unsafe(no_mangle)]
pub extern "C" fn snacc_list_clear(list: *mut SnaccList) {
    if list.is_null() {
        panic!("snacc: List.clear received a null descriptor")
    }
    // Safety: the compiler passes the address of a live List descriptor.
    unsafe {
        (*list).len = 0;
    }
}

fn list_pop<T: Copy>(list: *mut SnaccList) -> T {
    if list.is_null() {
        panic!("snacc: List.pop received a null descriptor")
    }
    // Safety: the compiler passes the address of a live List descriptor.
    let list = unsafe { &mut *list };
    if list.len == 0 {
        panic!("snacc: List.pop on an empty list")
    }
    list.len -= 1;
    // Safety: the final slot is initialized and remains within the allocation.
    unsafe {
        std::ptr::read(
            list.ptr
                .add(list.len * std::mem::size_of::<T>())
                .cast::<T>(),
        )
    }
}

fn list_insert<T: Copy>(list: *mut SnaccList, index: i64, value: T) {
    if list.is_null() {
        panic!("snacc: List.insert received a null descriptor")
    }
    if index < 0 {
        snacc_collection_bounds_fail();
    }
    // Safety: the compiler passes the address of a live List descriptor.
    let list = unsafe { &mut *list };
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    if index > list.len {
        snacc_collection_bounds_fail();
    }
    let size = std::mem::size_of::<T>();
    list_reserve(list, size, std::mem::align_of::<T>());
    // Safety: reserve established one additional slot, and `copy` handles the
    // overlapping shift toward the end of the initialized range.
    unsafe {
        let ptr = list.ptr.cast::<T>();
        std::ptr::copy(ptr.add(index), ptr.add(index + 1), list.len - index);
        std::ptr::write(ptr.add(index), value);
    }
    list.len += 1;
}

fn list_remove<T: Copy>(list: *mut SnaccList, index: i64) -> T {
    if list.is_null() {
        panic!("snacc: List.remove received a null descriptor")
    }
    if index < 0 {
        snacc_collection_bounds_fail();
    }
    // Safety: the compiler passes the address of a live List descriptor.
    let list = unsafe { &mut *list };
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    if index >= list.len {
        snacc_collection_bounds_fail();
    }
    // Safety: the indexed slot is initialized and the shifted range overlaps
    // only where `copy` is specifically permitted to handle it.
    unsafe {
        let ptr = list.ptr.cast::<T>();
        let removed = std::ptr::read(ptr.add(index));
        std::ptr::copy(ptr.add(index + 1), ptr.add(index), list.len - index - 1);
        list.len -= 1;
        removed
    }
}

macro_rules! list_scalar_exports {
    ($suffix:ident, $ty:ty) => {
        paste! {
            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_list_pop_ $suffix >](list: *mut SnaccList) -> $ty {
                list_pop(list)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_list_insert_ $suffix >](
                list: *mut SnaccList,
                index: i64,
                value: $ty,
            ) {
                list_insert(list, index, value);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_list_remove_ $suffix >](
                list: *mut SnaccList,
                index: i64,
            ) -> $ty {
                list_remove(list, index)
            }

            #[doc(hidden)]
            pub fn [< force_link_list_scalar_ $suffix >]() {
                let symbols = [
                    [< snacc_list_pop_ $suffix >] as *const () as usize,
                    [< snacc_list_insert_ $suffix >] as *const () as usize,
                    [< snacc_list_remove_ $suffix >] as *const () as usize,
                ];
                std::hint::black_box(symbols);
            }

            #[doc(hidden)]
            #[cfg(test)]
            pub(crate) const [< LIST_SCALAR_SYMBOLS_ $suffix:upper >]: &[&str] = &[
                stringify!([< snacc_list_pop_ $suffix >]),
                stringify!([< snacc_list_insert_ $suffix >]),
                stringify!([< snacc_list_remove_ $suffix >]),
            ];
        }
    };
}

list_scalar_exports!(i64, i64);
list_scalar_exports!(u8, u8);
list_scalar_exports!(u16, u16);
list_scalar_exports!(u32, u32);
list_scalar_exports!(u64, u64);
list_scalar_exports!(f32, f32);
list_scalar_exports!(f64, f64);
list_scalar_exports!(bool, u8);
list_scalar_exports!(unicode, u32);

#[unsafe(no_mangle)]
pub extern "C" fn snacc_list_reserve(
    list: *mut SnaccList,
    minimum: i64,
    element_size: usize,
    element_align: usize,
) {
    if list.is_null() {
        panic!("snacc: List.reserve received a null descriptor")
    }
    if minimum < 0 {
        snacc_collection_bounds_fail();
    }
    // Safety: the compiler passes the address of a live List descriptor.
    let list = unsafe { &mut *list };
    let minimum = usize::try_from(minimum).unwrap_or(usize::MAX);
    list_reserve_to(list, minimum, element_size, element_align);
}

/// Moves opaque compiler-owned list elements by bytes. The compiler uses this
/// path for non-`Copy` elements such as strings, boxes, and aggregates; it
/// performs element destruction separately, so the runtime never guesses a
/// Rust drop implementation for a Snacc value.
#[unsafe(no_mangle)]
pub extern "C" fn snacc_list_push_raw(
    list: *mut SnaccList,
    value: *const u8,
    element_size: usize,
    element_align: usize,
) {
    let list = unsafe { list.as_mut().expect("List.push received a null descriptor") };
    let index = list.len;
    list_reserve(list, element_size, element_align);
    if element_size != 0 {
        // Safety: `value` points to one initialized value and reserve created
        // one uninitialized destination slot of exactly this byte size.
        unsafe {
            std::ptr::copy_nonoverlapping(value, list.ptr.add(index * element_size), element_size);
        }
    }
    list.len += 1;
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_list_pop_raw(list: *mut SnaccList, out: *mut u8, element_size: usize) {
    let list = unsafe { list.as_mut().expect("List.pop received a null descriptor") };
    if list.len == 0 {
        panic!("snacc: List.pop on an empty list")
    }
    list.len -= 1;
    if element_size != 0 {
        // Safety: the final slot is initialized and `out` points to storage
        // allocated by the compiler for one returned value.
        unsafe {
            std::ptr::copy_nonoverlapping(list.ptr.add(list.len * element_size), out, element_size);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_list_insert_raw(
    list: *mut SnaccList,
    index: i64,
    value: *const u8,
    element_size: usize,
    element_align: usize,
) {
    let list = unsafe {
        list.as_mut()
            .expect("List.insert received a null descriptor")
    };
    if index < 0 {
        snacc_collection_bounds_fail();
    }
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    if index > list.len {
        snacc_collection_bounds_fail();
    }
    list_reserve(list, element_size, element_align);
    if element_size != 0 {
        // Safety: reserve created one additional slot; `copy` handles the
        // overlapping shift and the incoming value remains live until copied.
        unsafe {
            let start = list.ptr.add(index * element_size);
            std::ptr::copy(
                start,
                start.add(element_size),
                (list.len - index) * element_size,
            );
            std::ptr::copy_nonoverlapping(value, start, element_size);
        }
    }
    list.len += 1;
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_list_remove_raw(
    list: *mut SnaccList,
    index: i64,
    out: *mut u8,
    element_size: usize,
) {
    let list = unsafe {
        list.as_mut()
            .expect("List.remove received a null descriptor")
    };
    if index < 0 {
        snacc_collection_bounds_fail();
    }
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    if index >= list.len {
        snacc_collection_bounds_fail();
    }
    if element_size != 0 {
        // Safety: the index is initialized, `out` is one compiler-owned
        // result slot, and the remaining initialized range is overlapping-safe.
        unsafe {
            let start = list.ptr.add(index * element_size);
            std::ptr::copy_nonoverlapping(start, out, element_size);
            std::ptr::copy(
                start.add(element_size),
                start,
                (list.len - index - 1) * element_size,
            );
        }
    }
    list.len -= 1;
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_list_clear_raw(list: *mut SnaccList) {
    let list = unsafe {
        list.as_mut()
            .expect("List.clear received a null descriptor")
    };
    list.len = 0;
}

#[doc(hidden)]
pub fn force_link_list() {
    force_link_list_push_i64();
    force_link_list_push_u8();
    force_link_list_push_u16();
    force_link_list_push_u32();
    force_link_list_push_u64();
    force_link_list_push_f32();
    force_link_list_push_f64();
    force_link_list_push_bool();
    force_link_list_push_unicode();
    force_link_list_scalar_i64();
    force_link_list_scalar_u8();
    force_link_list_scalar_u16();
    force_link_list_scalar_u32();
    force_link_list_scalar_u64();
    force_link_list_scalar_f32();
    force_link_list_scalar_f64();
    force_link_list_scalar_bool();
    force_link_list_scalar_unicode();
    force_link_list_fixed();
    force_link_list_raw();
}

#[doc(hidden)]
pub fn force_link_list_fixed() {
    let symbols = [
        snacc_list_clear as *const () as usize,
        snacc_list_reserve as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
pub fn force_link_list_raw() {
    let symbols = [
        snacc_list_push_raw as *const () as usize,
        snacc_list_pop_raw as *const () as usize,
        snacc_list_insert_raw as *const () as usize,
        snacc_list_remove_raw as *const () as usize,
        snacc_list_clear_raw as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
#[cfg(test)]
pub(crate) const LIST_FIXED_SYMBOLS: &[&str] = &["snacc_list_clear", "snacc_list_reserve"];

#[doc(hidden)]
#[cfg(test)]
pub(crate) const LIST_RAW_SYMBOLS: &[&str] = &[
    "snacc_list_push_raw",
    "snacc_list_pop_raw",
    "snacc_list_insert_raw",
    "snacc_list_remove_raw",
    "snacc_list_clear_raw",
];
