//! Insertion-ordered `Set<T>`.
//!
//! `scalar_set_runtime` generates every scalar-element store, entry point,
//! link anchor, and symbol-name list from a suffix/element pair. The
//! `String` and `Int64` instantiations stay hand-written: owned strings
//! need cloning the macro stores cannot assume.

use super::*;
use paste::paste;
use std::collections::HashSet;

macro_rules! scalar_set_runtime {
    ($suffix:ident, $elem:ty) => {
        paste! {
            struct [< $suffix:camel Set >] {
                set: HashSet<$elem>,
                order: Vec<$elem>,
            }

            fn [< $suffix _set_mut >](set: &mut SnaccSet) -> &mut [< $suffix:camel Set >] {
                let store = [< $suffix:camel Set >] {
                    set: HashSet::new(),
                    order: Vec::new(),
                };
                if set.ptr.is_null() {
                    set.ptr = Box::into_raw(Box::new(store)).cast();
                }
                // Safety: this descriptor is initialized with exactly this store
                // type by the corresponding compiler-selected runtime entry point.
                unsafe { &mut *set.ptr.cast::<[< $suffix:camel Set >]>() }
            }

            fn [< $suffix _set_ref >](set: &SnaccSet) -> Option<&[< $suffix:camel Set >]> {
                if set.ptr.is_null() {
                    None
                } else {
                    // Safety: this descriptor was initialized by this store's
                    // corresponding mutable helper.
                    Some(unsafe { &*set.ptr.cast::<[< $suffix:camel Set >]>() })
                }
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_set_ $suffix _insert >](
                set: *mut SnaccSet,
                value: $elem,
            ) -> u8 {
                let set = unsafe { set.as_mut().expect("Set.insert received a null descriptor") };
                let store = [< $suffix _set_mut >](set);
                let fresh = store.set.insert(value);
                if fresh {
                    store.order.push(value);
                }
                let (len, cap) = (
                    store.set.len(),
                    store.set.capacity().max(store.order.capacity()),
                );
                sync_set(set, len, cap);
                u8::from(fresh)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_set_ $suffix _contains >](
                set: *const SnaccSet,
                value: $elem,
            ) -> u8 {
                let Some(set) = (unsafe { set.as_ref() }) else {
                    return 0;
                };
                u8::from([< $suffix _set_ref >](set).is_some_and(|store| store.set.contains(&value)))
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_set_ $suffix _at >](
                set: *const SnaccSet,
                index: i64,
            ) -> $elem {
                let Some(set) = (unsafe { set.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let Some(store) = [< $suffix _set_ref >](set) else {
                    snacc_collection_bounds_fail()
                };
                let index = usize::try_from(index).unwrap_or(usize::MAX);
                *store
                    .order
                    .get(index)
                    .unwrap_or_else(|| snacc_collection_bounds_fail())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_set_ $suffix _delete >](
                set: *mut SnaccSet,
                value: $elem,
            ) -> u8 {
                let set = unsafe { set.as_mut().expect("Set.delete received a null descriptor") };
                let store = [< $suffix _set_mut >](set);
                let existed = store.set.remove(&value);
                if existed {
                    store.order.retain(|entry| entry != &value);
                }
                let (len, cap) = (
                    store.set.len(),
                    store.set.capacity().max(store.order.capacity()),
                );
                sync_set(set, len, cap);
                u8::from(existed)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_set_ $suffix _clear >](set: *mut SnaccSet) {
                let set = unsafe { set.as_mut().expect("Set.clear received a null descriptor") };
                let store = [< $suffix _set_mut >](set);
                store.set.clear();
                store.order.clear();
                let cap = store.set.capacity().max(store.order.capacity());
                sync_set(set, 0, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_set_ $suffix _reserve >](
                set: *mut SnaccSet,
                minimum: i64,
            ) {
                let set = unsafe {
                    set.as_mut()
                        .expect("Set.reserve received a null descriptor")
                };
                let store = [< $suffix _set_mut >](set);
                let additional = reserve_target(minimum, store.set.len());
                if additional != 0 {
                    store.set.reserve(additional);
                    store.order.reserve(additional);
                }
                let (len, cap) = (
                    store.set.len(),
                    store.set.capacity().max(store.order.capacity()),
                );
                sync_set(set, len, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_set_ $suffix _drop >](set: *const SnaccSet) {
                let Some(set) = (unsafe { set.as_ref() }) else {
                    return;
                };
                if !set.ptr.is_null() {
                    // Safety: this descriptor was initialized as this concrete
                    // store and has not previously been destroyed.
                    unsafe { drop(Box::from_raw(set.ptr.cast::<[< $suffix:camel Set >]>())) };
                }
            }

            #[doc(hidden)]
            pub fn [< force_link_set_ $suffix >]() {
                let symbols = [
                    [< snacc_set_ $suffix _insert >] as *const () as usize,
                    [< snacc_set_ $suffix _contains >] as *const () as usize,
                    [< snacc_set_ $suffix _at >] as *const () as usize,
                    [< snacc_set_ $suffix _delete >] as *const () as usize,
                    [< snacc_set_ $suffix _clear >] as *const () as usize,
                    [< snacc_set_ $suffix _reserve >] as *const () as usize,
                    [< snacc_set_ $suffix _drop >] as *const () as usize,
                ];
                std::hint::black_box(symbols);
            }

            #[doc(hidden)]
            #[cfg(test)]
            pub(crate) const [< SET_SYMBOLS_ $suffix:upper >]: &[&str] = &[
                stringify!([< snacc_set_ $suffix _insert >]),
                stringify!([< snacc_set_ $suffix _contains >]),
                stringify!([< snacc_set_ $suffix _at >]),
                stringify!([< snacc_set_ $suffix _delete >]),
                stringify!([< snacc_set_ $suffix _clear >]),
                stringify!([< snacc_set_ $suffix _reserve >]),
                stringify!([< snacc_set_ $suffix _drop >]),
            ];
        }
    };
}

scalar_set_runtime!(u8, u8);
scalar_set_runtime!(u16, u16);
scalar_set_runtime!(u32, u32);
scalar_set_runtime!(u64, u64);
scalar_set_runtime!(bool, u8);
scalar_set_runtime!(unicode, u32);

struct StringSet {
    set: HashSet<String>,
    order: Vec<String>,
}

struct I64Set {
    set: HashSet<i64>,
    order: Vec<i64>,
}

fn set_string_mut(set: &mut SnaccSet) -> &mut StringSet {
    if set.ptr.is_null() {
        set.ptr = Box::into_raw(Box::new(StringSet {
            set: HashSet::new(),
            order: Vec::new(),
        }))
        .cast();
    }
    // Safety: a StringSet is installed by this module for this descriptor.
    unsafe { &mut *set.ptr.cast::<StringSet>() }
}

fn set_string_ref(set: &SnaccSet) -> Option<&StringSet> {
    if set.ptr.is_null() {
        None
    } else {
        // Safety: a StringSet is installed by this module for this descriptor.
        Some(unsafe { &*set.ptr.cast::<StringSet>() })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_string_insert(set: *mut SnaccSet, value: *const SnaccString) -> u8 {
    let set = unsafe { set.as_mut().expect("Set.insert received a null descriptor") };
    let value = unsafe { value.as_ref().expect("Set.insert received a null value") };
    let value = take_string(*value);
    let store = set_string_mut(set);
    let fresh = store.set.insert(value.clone());
    if fresh {
        store.order.push(value);
    }
    let (len, cap) = (store.set.len(), store.set.capacity());
    sync_set(set, len, cap);
    u8::from(fresh)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_string_contains(set: *const SnaccSet, value: *const SnaccView) -> u8 {
    let Some(set) = (unsafe { set.as_ref() }) else {
        return 0;
    };
    let value = unsafe { value.as_ref().expect("Set.contains received a null value") };
    let Some(value) = view_text(*value) else {
        return 0;
    };
    u8::from(set_string_ref(set).is_some_and(|store| store.set.contains(value)))
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_string_at(set: *const SnaccSet, index: i64) -> SnaccString {
    let Some(set) = (unsafe { set.as_ref() }) else {
        snacc_collection_bounds_fail()
    };
    let Some(store) = set_string_ref(set) else {
        snacc_collection_bounds_fail()
    };
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    let value = store
        .order
        .get(index)
        .unwrap_or_else(|| snacc_collection_bounds_fail());
    // See `snacc_map_string_i64_key_at`: this is a borrowed descriptor, not
    // an allocation owned by the loop binding.
    SnaccString {
        ptr: value.as_ptr().cast_mut(),
        len: value.len(),
        cap: 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_string_at_out(out: *mut SnaccString, set: *const SnaccSet, index: i64) {
    let out = unsafe {
        out.as_mut()
            .expect("Set element lookup received a null output")
    };
    *out = snacc_set_string_at(set, index);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_string_delete(set: *mut SnaccSet, value: *const SnaccView) -> u8 {
    let set = unsafe { set.as_mut().expect("Set.delete received a null descriptor") };
    let value = unsafe { value.as_ref().expect("Set.delete received a null value") };
    let Some(value) = view_text(*value).map(str::to_owned) else {
        return 0;
    };
    let store = set_string_mut(set);
    let existed = store.set.remove(&value);
    if existed {
        store.order.retain(|entry| entry != &value);
    }
    let (len, cap) = (store.set.len(), store.set.capacity());
    sync_set(set, len, cap);
    u8::from(existed)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_string_clear(set: *mut SnaccSet) {
    let set = unsafe { set.as_mut().expect("Set.clear received a null descriptor") };
    let store = set_string_mut(set);
    store.set.clear();
    store.order.clear();
    let cap = store.set.capacity();
    sync_set(set, 0, cap);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_string_reserve(set: *mut SnaccSet, minimum: i64) {
    let set = unsafe {
        set.as_mut()
            .expect("Set.reserve received a null descriptor")
    };
    let store = set_string_mut(set);
    let additional = reserve_target(minimum, store.set.len());
    if additional != 0 {
        store.set.reserve(additional);
        store.order.reserve(additional);
    }
    let (len, cap) = (
        store.set.len(),
        store.set.capacity().max(store.order.capacity()),
    );
    sync_set(set, len, cap);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_string_drop(set: *const SnaccSet) {
    let Some(set) = (unsafe { set.as_ref() }) else {
        return;
    };
    if !set.ptr.is_null() {
        // Safety: this descriptor was created for a StringSet by this module.
        unsafe { drop(Box::from_raw(set.ptr.cast::<StringSet>())) };
    }
}

fn set_i64_mut(set: &mut SnaccSet) -> &mut I64Set {
    if set.ptr.is_null() {
        set.ptr = Box::into_raw(Box::new(I64Set {
            set: HashSet::new(),
            order: Vec::new(),
        }))
        .cast();
    }
    // Safety: an I64Set is installed by this module for this descriptor.
    unsafe { &mut *set.ptr.cast::<I64Set>() }
}

fn set_i64_ref(set: &SnaccSet) -> Option<&I64Set> {
    if set.ptr.is_null() {
        None
    } else {
        // Safety: an I64Set is installed by this module for this descriptor.
        Some(unsafe { &*set.ptr.cast::<I64Set>() })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_i64_insert(set: *mut SnaccSet, value: i64) -> u8 {
    let set = unsafe { set.as_mut().expect("Set.insert received a null descriptor") };
    let store = set_i64_mut(set);
    let fresh = store.set.insert(value);
    if fresh {
        store.order.push(value);
    }
    let (len, cap) = (store.set.len(), store.set.capacity());
    sync_set(set, len, cap);
    u8::from(fresh)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_i64_contains(set: *const SnaccSet, value: i64) -> u8 {
    let Some(set) = (unsafe { set.as_ref() }) else {
        return 0;
    };
    u8::from(set_i64_ref(set).is_some_and(|store| store.set.contains(&value)))
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_i64_at(set: *const SnaccSet, index: i64) -> i64 {
    let Some(set) = (unsafe { set.as_ref() }) else {
        snacc_collection_bounds_fail()
    };
    let Some(store) = set_i64_ref(set) else {
        snacc_collection_bounds_fail()
    };
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    *store
        .order
        .get(index)
        .unwrap_or_else(|| snacc_collection_bounds_fail())
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_i64_delete(set: *mut SnaccSet, value: i64) -> u8 {
    let set = unsafe { set.as_mut().expect("Set.delete received a null descriptor") };
    let store = set_i64_mut(set);
    let existed = store.set.remove(&value);
    if existed {
        store.order.retain(|entry| entry != &value);
    }
    let (len, cap) = (store.set.len(), store.set.capacity());
    sync_set(set, len, cap);
    u8::from(existed)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_i64_clear(set: *mut SnaccSet) {
    let set = unsafe { set.as_mut().expect("Set.clear received a null descriptor") };
    let store = set_i64_mut(set);
    store.set.clear();
    store.order.clear();
    let cap = store.set.capacity();
    sync_set(set, 0, cap);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_i64_reserve(set: *mut SnaccSet, minimum: i64) {
    let set = unsafe {
        set.as_mut()
            .expect("Set.reserve received a null descriptor")
    };
    let store = set_i64_mut(set);
    let additional = reserve_target(minimum, store.set.len());
    if additional != 0 {
        store.set.reserve(additional);
        store.order.reserve(additional);
    }
    let (len, cap) = (
        store.set.len(),
        store.set.capacity().max(store.order.capacity()),
    );
    sync_set(set, len, cap);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_set_i64_drop(set: *const SnaccSet) {
    let Some(set) = (unsafe { set.as_ref() }) else {
        return;
    };
    if !set.ptr.is_null() {
        // Safety: this descriptor was created for an I64Set by this module.
        unsafe { drop(Box::from_raw(set.ptr.cast::<I64Set>())) };
    }
}

#[doc(hidden)]
pub fn force_link_set() {
    force_link_set_u8();
    force_link_set_u16();
    force_link_set_u32();
    force_link_set_u64();
    force_link_set_bool();
    force_link_set_unicode();
    force_link_set_string();
    force_link_set_i64();
}

#[doc(hidden)]
pub fn force_link_set_string() {
    let symbols = [
        snacc_set_string_insert as *const () as usize,
        snacc_set_string_contains as *const () as usize,
        snacc_set_string_at as *const () as usize,
        snacc_set_string_at_out as *const () as usize,
        snacc_set_string_delete as *const () as usize,
        snacc_set_string_clear as *const () as usize,
        snacc_set_string_reserve as *const () as usize,
        snacc_set_string_drop as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
pub fn force_link_set_i64() {
    let symbols = [
        snacc_set_i64_insert as *const () as usize,
        snacc_set_i64_contains as *const () as usize,
        snacc_set_i64_at as *const () as usize,
        snacc_set_i64_delete as *const () as usize,
        snacc_set_i64_clear as *const () as usize,
        snacc_set_i64_reserve as *const () as usize,
        snacc_set_i64_drop as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
#[cfg(test)]
pub(crate) const SET_STRING_SYMBOLS: &[&str] = &[
    "snacc_set_string_insert",
    "snacc_set_string_contains",
    "snacc_set_string_at",
    "snacc_set_string_at_out",
    "snacc_set_string_delete",
    "snacc_set_string_clear",
    "snacc_set_string_reserve",
    "snacc_set_string_drop",
];

#[doc(hidden)]
#[cfg(test)]
pub(crate) const SET_I64_SYMBOLS: &[&str] = &[
    "snacc_set_i64_insert",
    "snacc_set_i64_contains",
    "snacc_set_i64_at",
    "snacc_set_i64_delete",
    "snacc_set_i64_clear",
    "snacc_set_i64_reserve",
    "snacc_set_i64_drop",
];
