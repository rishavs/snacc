//! Insertion-ordered `Map<K, Int64>` and byte-storing raw maps.
//!
//! The three macros generate every monomorphized store, entry point,
//! link anchor, and symbol-name list from a suffix/type pair, so adding a
//! key type is one invocation line. The `String -> Int64` and `Int64 ->
//! Int64` instantiations stay hand-written: their descriptor capacity
//! bookkeeping differs from the macro stores, and unifying them would change
//! observable capacity values.

use super::*;
use paste::paste;
use std::collections::HashMap;

macro_rules! scalar_map_runtime {
    ($suffix:ident, $key:ty) => {
        paste! {
            struct [< $suffix:camel I64Map >] {
                map: HashMap<$key, i64>,
                order: Vec<$key>,
            }

            fn [< $suffix _i64_map_mut >](map: &mut SnaccMap) -> &mut [< $suffix:camel I64Map >] {
                let store = [< $suffix:camel I64Map >] {
                    map: HashMap::new(),
                    order: Vec::new(),
                };
                if map.ptr.is_null() {
                    map.ptr = Box::into_raw(Box::new(store)).cast();
                }
                // Safety: this descriptor is initialized with exactly this store
                // type by the corresponding compiler-selected runtime entry point.
                unsafe { &mut *map.ptr.cast::<[< $suffix:camel I64Map >]>() }
            }

            fn [< $suffix _i64_map_ref >](map: &SnaccMap) -> Option<&[< $suffix:camel I64Map >]> {
                if map.ptr.is_null() {
                    None
                } else {
                    // Safety: this descriptor was initialized by this store's
                    // corresponding mutable helper.
                    Some(unsafe { &*map.ptr.cast::<[< $suffix:camel I64Map >]>() })
                }
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_insert >](
                map: *mut SnaccMap,
                key: $key,
                value: i64,
            ) -> u8 {
                let map = unsafe { map.as_mut().expect("Map.insert received a null descriptor") };
                let store = [< $suffix _i64_map_mut >](map);
                let fresh = !store.map.contains_key(&key);
                if fresh {
                    store.order.push(key);
                }
                store.map.insert(key, value);
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
                u8::from(fresh)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_contains >](
                map: *const SnaccMap,
                key: $key,
            ) -> u8 {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    return 0;
                };
                u8::from([< $suffix _i64_map_ref >](map).is_some_and(|store| store.map.contains_key(&key)))
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_index >](
                map: *const SnaccMap,
                key: $key,
            ) -> i64 {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let Some(value) = [< $suffix _i64_map_ref >](map).and_then(|store| store.map.get(&key)) else {
                    snacc_collection_bounds_fail()
                };
                *value
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_key_at >](
                map: *const SnaccMap,
                index: i64,
            ) -> $key {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let Some(store) = [< $suffix _i64_map_ref >](map) else {
                    snacc_collection_bounds_fail()
                };
                let index = usize::try_from(index).unwrap_or(usize::MAX);
                *store
                    .order
                    .get(index)
                    .unwrap_or_else(|| snacc_collection_bounds_fail())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_value_at >](
                map: *const SnaccMap,
                index: i64,
            ) -> i64 {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let Some(store) = [< $suffix _i64_map_ref >](map) else {
                    snacc_collection_bounds_fail()
                };
                let index = usize::try_from(index).unwrap_or(usize::MAX);
                let key = store
                    .order
                    .get(index)
                    .unwrap_or_else(|| snacc_collection_bounds_fail());
                *store
                    .map
                    .get(key)
                    .unwrap_or_else(|| snacc_collection_bounds_fail())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_delete >](
                map: *mut SnaccMap,
                key: $key,
            ) -> u8 {
                let map = unsafe { map.as_mut().expect("Map.delete received a null descriptor") };
                let store = [< $suffix _i64_map_mut >](map);
                let existed = store.map.remove(&key).is_some();
                if existed {
                    store.order.retain(|entry| entry != &key);
                }
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
                u8::from(existed)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_take >](
                map: *mut SnaccMap,
                key: $key,
            ) -> i64 {
                let map = unsafe { map.as_mut().expect("Map.take received a null descriptor") };
                let store = [< $suffix _i64_map_mut >](map);
                let Some(value) = store.map.remove(&key) else {
                    snacc_collection_bounds_fail()
                };
                store.order.retain(|entry| entry != &key);
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
                value
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_clear >](map: *mut SnaccMap) {
                let map = unsafe { map.as_mut().expect("Map.clear received a null descriptor") };
                let store = [< $suffix _i64_map_mut >](map);
                store.map.clear();
                store.order.clear();
                let cap = store.map.capacity().max(store.order.capacity());
                sync_map(map, 0, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_reserve >](
                map: *mut SnaccMap,
                minimum: i64,
            ) {
                let map = unsafe {
                    map.as_mut()
                        .expect("Map.reserve received a null descriptor")
                };
                let store = [< $suffix _i64_map_mut >](map);
                let additional = reserve_target(minimum, store.map.len());
                if additional != 0 {
                    store.map.reserve(additional);
                    store.order.reserve(additional);
                }
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _i64_drop >](map: *const SnaccMap) {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    return;
                };
                if !map.ptr.is_null() {
                    // Safety: this descriptor was initialized as this concrete
                    // store and has not previously been destroyed.
                    unsafe { drop(Box::from_raw(map.ptr.cast::<[< $suffix:camel I64Map >]>())) };
                }
            }

            #[doc(hidden)]
            pub fn [< force_link_map_ $suffix _i64 >]() {
                let symbols = [
                    [< snacc_map_ $suffix _i64_insert >] as *const () as usize,
                    [< snacc_map_ $suffix _i64_contains >] as *const () as usize,
                    [< snacc_map_ $suffix _i64_index >] as *const () as usize,
                    [< snacc_map_ $suffix _i64_key_at >] as *const () as usize,
                    [< snacc_map_ $suffix _i64_value_at >] as *const () as usize,
                    [< snacc_map_ $suffix _i64_delete >] as *const () as usize,
                    [< snacc_map_ $suffix _i64_take >] as *const () as usize,
                    [< snacc_map_ $suffix _i64_clear >] as *const () as usize,
                    [< snacc_map_ $suffix _i64_reserve >] as *const () as usize,
                    [< snacc_map_ $suffix _i64_drop >] as *const () as usize,
                ];
                std::hint::black_box(symbols);
            }

            #[doc(hidden)]
            #[cfg(test)]
            pub(crate) const [< MAP_I64_SYMBOLS_ $suffix:upper >]: &[&str] = &[
                stringify!([< snacc_map_ $suffix _i64_insert >]),
                stringify!([< snacc_map_ $suffix _i64_contains >]),
                stringify!([< snacc_map_ $suffix _i64_index >]),
                stringify!([< snacc_map_ $suffix _i64_key_at >]),
                stringify!([< snacc_map_ $suffix _i64_value_at >]),
                stringify!([< snacc_map_ $suffix _i64_delete >]),
                stringify!([< snacc_map_ $suffix _i64_take >]),
                stringify!([< snacc_map_ $suffix _i64_clear >]),
                stringify!([< snacc_map_ $suffix _i64_reserve >]),
                stringify!([< snacc_map_ $suffix _i64_drop >]),
            ];
        }
    };
}

macro_rules! raw_scalar_map_runtime {
    ($suffix:ident, $key:ty) => {
        paste! {
            struct [< $suffix:camel RawMap >] {
                map: HashMap<$key, Vec<u8>>,
                order: Vec<$key>,
            }

            fn [< $suffix _raw_map_mut >](map: &mut SnaccMap) -> &mut [< $suffix:camel RawMap >] {
                let store = [< $suffix:camel RawMap >] {
                    map: HashMap::new(),
                    order: Vec::new(),
                };
                if map.ptr.is_null() {
                    map.ptr = Box::into_raw(Box::new(store)).cast();
                }
                // Safety: this descriptor is initialized with this store by the
                // compiler-selected raw runtime entry points.
                unsafe { &mut *map.ptr.cast::<[< $suffix:camel RawMap >]>() }
            }

            fn [< $suffix _raw_map_ref >](map: &SnaccMap) -> Option<&[< $suffix:camel RawMap >]> {
                if map.ptr.is_null() {
                    None
                } else {
                    // Safety: this descriptor was initialized by this store's
                    // corresponding mutable helper.
                    Some(unsafe { &*map.ptr.cast::<[< $suffix:camel RawMap >]>() })
                }
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_insert >](
                map: *mut SnaccMap,
                key: $key,
                value: *const u8,
                value_size: usize,
                old: *mut u8,
            ) -> u8 {
                let map = unsafe { map.as_mut().expect("Map.insert received a null descriptor") };
                let store = [< $suffix _raw_map_mut >](map);
                let fresh = !store.map.contains_key(&key);
                if fresh {
                    store.order.push(key);
                }
                let bytes = if value_size == 0 {
                    Vec::new()
                } else {
                    // Safety: `value` points to the initialized temporary value
                    // created by the compiler for exactly `value_size` bytes.
                    unsafe { std::slice::from_raw_parts(value, value_size).to_vec() }
                };
                if let Some(previous) = store.map.insert(key, bytes) {
                    // Safety: `old` is an aligned compiler-owned slot whenever
                    // the caller can need the replaced value.
                    unsafe { copy_raw_bytes(previous.as_ptr(), old, previous.len()) };
                }
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
                u8::from(fresh)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_contains >](
                map: *const SnaccMap,
                key: $key,
            ) -> u8 {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    return 0;
                };
                u8::from([< $suffix _raw_map_ref >](map).is_some_and(|store| store.map.contains_key(&key)))
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_index >](
                map: *const SnaccMap,
                key: $key,
                out: *mut u8,
                value_size: usize,
            ) {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let Some(bytes) = [< $suffix _raw_map_ref >](map).and_then(|store| store.map.get(&key)) else {
                    snacc_collection_bounds_fail()
                };
                if bytes.len() != value_size {
                    snacc_collection_bounds_fail()
                }
                // Safety: `out` is the compiler's aligned result slot.
                unsafe { copy_raw_bytes(bytes.as_ptr(), out, value_size) };
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_key_at >](
                map: *const SnaccMap,
                index: i64,
            ) -> $key {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let Some(store) = [< $suffix _raw_map_ref >](map) else {
                    snacc_collection_bounds_fail()
                };
                let index = usize::try_from(index).unwrap_or(usize::MAX);
                *store
                    .order
                    .get(index)
                    .unwrap_or_else(|| snacc_collection_bounds_fail())
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_value_at >](
                map: *const SnaccMap,
                index: i64,
                out: *mut u8,
                value_size: usize,
            ) {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let Some(store) = [< $suffix _raw_map_ref >](map) else {
                    snacc_collection_bounds_fail()
                };
                let index = usize::try_from(index).unwrap_or(usize::MAX);
                let key = store
                    .order
                    .get(index)
                    .unwrap_or_else(|| snacc_collection_bounds_fail());
                let bytes = store
                    .map
                    .get(key)
                    .unwrap_or_else(|| snacc_collection_bounds_fail());
                if bytes.len() != value_size {
                    snacc_collection_bounds_fail()
                }
                // Safety: `out` is the compiler's aligned result slot.
                unsafe { copy_raw_bytes(bytes.as_ptr(), out, value_size) };
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_delete >](
                map: *mut SnaccMap,
                key: $key,
                old: *mut u8,
                value_size: usize,
            ) -> u8 {
                let map = unsafe { map.as_mut().expect("Map.delete received a null descriptor") };
                let store = [< $suffix _raw_map_mut >](map);
                let Some(previous) = store.map.remove(&key) else {
                    return 0;
                };
                if previous.len() != value_size {
                    snacc_collection_bounds_fail()
                }
                // Safety: `old` is the compiler's aligned result slot.
                unsafe { copy_raw_bytes(previous.as_ptr(), old, value_size) };
                store.order.retain(|entry| entry != &key);
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
                1
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_take >](
                map: *mut SnaccMap,
                key: $key,
                out: *mut u8,
                value_size: usize,
            ) {
                let map = unsafe { map.as_mut().expect("Map.take received a null descriptor") };
                let store = [< $suffix _raw_map_mut >](map);
                let Some(value) = store.map.remove(&key) else {
                    snacc_collection_bounds_fail()
                };
                if value.len() != value_size {
                    snacc_collection_bounds_fail()
                }
                // Safety: `out` is the compiler's aligned result slot.
                unsafe { copy_raw_bytes(value.as_ptr(), out, value_size) };
                store.order.retain(|entry| entry != &key);
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_clear >](map: *mut SnaccMap) {
                let map = unsafe { map.as_mut().expect("Map.clear received a null descriptor") };
                let store = [< $suffix _raw_map_mut >](map);
                store.map.clear();
                store.order.clear();
                let cap = store.map.capacity().max(store.order.capacity());
                sync_map(map, 0, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_reserve >](
                map: *mut SnaccMap,
                minimum: i64,
            ) {
                let map = unsafe {
                    map.as_mut()
                        .expect("Map.reserve received a null descriptor")
                };
                let store = [< $suffix _raw_map_mut >](map);
                let additional = reserve_target(minimum, store.map.len());
                if additional != 0 {
                    store.map.reserve(additional);
                    store.order.reserve(additional);
                }
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn [< snacc_map_ $suffix _raw_drop >](map: *const SnaccMap) {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    return;
                };
                if !map.ptr.is_null() {
                    // Safety: this descriptor was initialized as this concrete
                    // store and has not previously been destroyed.
                    unsafe { drop(Box::from_raw(map.ptr.cast::<[< $suffix:camel RawMap >]>())) };
                }
            }

            #[doc(hidden)]
            pub fn [< force_link_map_ $suffix _raw >]() {
                let symbols = [
                    [< snacc_map_ $suffix _raw_insert >] as *const () as usize,
                    [< snacc_map_ $suffix _raw_contains >] as *const () as usize,
                    [< snacc_map_ $suffix _raw_index >] as *const () as usize,
                    [< snacc_map_ $suffix _raw_key_at >] as *const () as usize,
                    [< snacc_map_ $suffix _raw_value_at >] as *const () as usize,
                    [< snacc_map_ $suffix _raw_delete >] as *const () as usize,
                    [< snacc_map_ $suffix _raw_take >] as *const () as usize,
                    [< snacc_map_ $suffix _raw_clear >] as *const () as usize,
                    [< snacc_map_ $suffix _raw_reserve >] as *const () as usize,
                    [< snacc_map_ $suffix _raw_drop >] as *const () as usize,
                ];
                std::hint::black_box(symbols);
            }

            #[doc(hidden)]
            #[cfg(test)]
            pub(crate) const [< MAP_RAW_SYMBOLS_ $suffix:upper >]: &[&str] = &[
                stringify!([< snacc_map_ $suffix _raw_insert >]),
                stringify!([< snacc_map_ $suffix _raw_contains >]),
                stringify!([< snacc_map_ $suffix _raw_index >]),
                stringify!([< snacc_map_ $suffix _raw_key_at >]),
                stringify!([< snacc_map_ $suffix _raw_value_at >]),
                stringify!([< snacc_map_ $suffix _raw_delete >]),
                stringify!([< snacc_map_ $suffix _raw_take >]),
                stringify!([< snacc_map_ $suffix _raw_clear >]),
                stringify!([< snacc_map_ $suffix _raw_reserve >]),
                stringify!([< snacc_map_ $suffix _raw_drop >]),
            ];
        }
    };
}

/// Raw map storage for String keys. Keys are owned by the runtime, while the
/// value bytes remain opaque and are copied into compiler-owned typed slots.
/// String queries use borrowed UTF-8 views so lookup never consumes a query.
macro_rules! raw_string_map_runtime {
    () => {
        paste! {
            struct StringRawMap {
                map: HashMap<String, Vec<u8>>,
                order: Vec<String>,
            }

            fn string_raw_map_mut(map: &mut SnaccMap) -> &mut StringRawMap {
                let store = StringRawMap {
                    map: HashMap::new(),
                    order: Vec::new(),
                };
                if map.ptr.is_null() {
                    map.ptr = Box::into_raw(Box::new(store)).cast();
                }
                // Safety: the compiler selects this store for String-keyed raw maps.
                unsafe { &mut *map.ptr.cast::<StringRawMap>() }
            }

            fn string_raw_map_ref(map: &SnaccMap) -> Option<&StringRawMap> {
                if map.ptr.is_null() {
                    None
                } else {
                    // Safety: this descriptor was initialized by the matching helper.
                    Some(unsafe { &*map.ptr.cast::<StringRawMap>() })
                }
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_insert(
                map: *mut SnaccMap,
                key: *const SnaccString,
                value: *const u8,
                value_size: usize,
                old: *mut u8,
            ) -> u8 {
                let map = unsafe { map.as_mut().expect("Map.insert received a null descriptor") };
                let key = unsafe { key.as_ref().expect("Map.insert received a null key") };
                let key = take_string(*key);
                let store = string_raw_map_mut(map);
                let fresh = !store.map.contains_key(&key);
                if fresh {
                    store.order.push(key.clone());
                }
                let bytes = if value_size == 0 {
                    Vec::new()
                } else {
                    // Safety: the compiler passes one initialized value slot.
                    unsafe { std::slice::from_raw_parts(value, value_size).to_vec() }
                };
                if let Some(previous) = store.map.insert(key, bytes) {
                    // Safety: old is an aligned compiler-owned replacement slot.
                    unsafe { copy_raw_bytes(previous.as_ptr(), old, previous.len()) };
                }
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
                u8::from(fresh)
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_contains(
                map: *const SnaccMap,
                key: *const SnaccView,
            ) -> u8 {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    return 0;
                };
                let key = unsafe { key.as_ref().expect("Map.contains received a null key") };
                let Some(key) = view_text(*key) else { return 0 };
                u8::from(string_raw_map_ref(map).is_some_and(|store| store.map.contains_key(key)))
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_index(
                map: *const SnaccMap,
                key: *const SnaccView,
                out: *mut u8,
                value_size: usize,
            ) {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let key = unsafe { key.as_ref().expect("Map indexing received a null key") };
                let Some(key) = view_text(*key) else {
                    snacc_collection_bounds_fail()
                };
                let Some(bytes) = string_raw_map_ref(map).and_then(|store| store.map.get(key)) else {
                    snacc_collection_bounds_fail()
                };
                if bytes.len() != value_size {
                    snacc_collection_bounds_fail()
                }
                // Safety: out is the compiler's aligned result slot.
                unsafe { copy_raw_bytes(bytes.as_ptr(), out, value_size) };
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_key_at(
                out: *mut SnaccString,
                map: *const SnaccMap,
                index: i64,
            ) {
                let out = unsafe { out.as_mut().expect("Map key lookup received a null output") };
                let Some(map) = (unsafe { map.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let Some(store) = string_raw_map_ref(map) else {
                    snacc_collection_bounds_fail()
                };
                let index = usize::try_from(index).unwrap_or(usize::MAX);
                let key = store
                    .order
                    .get(index)
                    .unwrap_or_else(|| snacc_collection_bounds_fail());
                // cap=0 marks a borrowed descriptor; the compiler owns no key data.
                *out = SnaccString {
                    ptr: key.as_ptr().cast_mut(),
                    len: key.len(),
                    cap: 0,
                };
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_value_at(
                map: *const SnaccMap,
                index: i64,
                out: *mut u8,
                value_size: usize,
            ) {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    snacc_collection_bounds_fail()
                };
                let Some(store) = string_raw_map_ref(map) else {
                    snacc_collection_bounds_fail()
                };
                let index = usize::try_from(index).unwrap_or(usize::MAX);
                let key = store
                    .order
                    .get(index)
                    .unwrap_or_else(|| snacc_collection_bounds_fail());
                let bytes = store
                    .map
                    .get(key)
                    .unwrap_or_else(|| snacc_collection_bounds_fail());
                if bytes.len() != value_size {
                    snacc_collection_bounds_fail()
                }
                // Safety: out is the compiler's aligned result slot.
                unsafe { copy_raw_bytes(bytes.as_ptr(), out, value_size) };
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_delete(
                map: *mut SnaccMap,
                key: *const SnaccView,
                old: *mut u8,
                value_size: usize,
            ) -> u8 {
                let map = unsafe { map.as_mut().expect("Map.delete received a null descriptor") };
                let key = unsafe { key.as_ref().expect("Map.delete received a null key") };
                let Some(key) = view_text(*key).map(str::to_owned) else {
                    return 0;
                };
                let store = string_raw_map_mut(map);
                let Some(previous) = store.map.remove(&key) else {
                    return 0;
                };
                if previous.len() != value_size {
                    snacc_collection_bounds_fail()
                }
                // Safety: old is the compiler's aligned result slot.
                unsafe { copy_raw_bytes(previous.as_ptr(), old, value_size) };
                store.order.retain(|entry| entry != &key);
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
                1
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_take(
                map: *mut SnaccMap,
                key: *const SnaccView,
                out: *mut u8,
                value_size: usize,
            ) {
                let map = unsafe { map.as_mut().expect("Map.take received a null descriptor") };
                let key = unsafe { key.as_ref().expect("Map.take received a null key") };
                let Some(key) = view_text(*key).map(str::to_owned) else {
                    snacc_collection_bounds_fail()
                };
                let store = string_raw_map_mut(map);
                let Some(value) = store.map.remove(&key) else {
                    snacc_collection_bounds_fail()
                };
                if value.len() != value_size {
                    snacc_collection_bounds_fail()
                }
                // Safety: out is the compiler's aligned result slot.
                unsafe { copy_raw_bytes(value.as_ptr(), out, value_size) };
                store.order.retain(|entry| entry != &key);
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_clear(map: *mut SnaccMap) {
                let map = unsafe { map.as_mut().expect("Map.clear received a null descriptor") };
                let store = string_raw_map_mut(map);
                store.map.clear();
                store.order.clear();
                let cap = store.map.capacity().max(store.order.capacity());
                sync_map(map, 0, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_reserve(map: *mut SnaccMap, minimum: i64) {
                let map = unsafe {
                    map.as_mut()
                        .expect("Map.reserve received a null descriptor")
                };
                let store = string_raw_map_mut(map);
                let additional = reserve_target(minimum, store.map.len());
                if additional != 0 {
                    store.map.reserve(additional);
                    store.order.reserve(additional);
                }
                let (len, cap) = (
                    store.map.len(),
                    store.map.capacity().max(store.order.capacity()),
                );
                sync_map(map, len, cap);
            }

            #[unsafe(no_mangle)]
            pub extern "C" fn snacc_map_string_raw_drop(map: *const SnaccMap) {
                let Some(map) = (unsafe { map.as_ref() }) else {
                    return;
                };
                if !map.ptr.is_null() {
                    // Safety: this descriptor was initialized by this store.
                    unsafe { drop(Box::from_raw(map.ptr.cast::<StringRawMap>())) };
                }
            }

            #[doc(hidden)]
            pub fn force_link_map_string_raw() {
                let symbols = [
                    snacc_map_string_raw_insert as *const () as usize,
                    snacc_map_string_raw_contains as *const () as usize,
                    snacc_map_string_raw_index as *const () as usize,
                    snacc_map_string_raw_key_at as *const () as usize,
                    snacc_map_string_raw_value_at as *const () as usize,
                    snacc_map_string_raw_delete as *const () as usize,
                    snacc_map_string_raw_take as *const () as usize,
                    snacc_map_string_raw_clear as *const () as usize,
                    snacc_map_string_raw_reserve as *const () as usize,
                    snacc_map_string_raw_drop as *const () as usize,
                ];
                std::hint::black_box(symbols);
            }

#[doc(hidden)]
#[cfg(test)]
pub(crate) const MAP_RAW_SYMBOLS_STRING: &[&str] = &[
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
            ];
        }
    };
}

scalar_map_runtime!(u8, u8);
scalar_map_runtime!(u16, u16);
scalar_map_runtime!(u32, u32);
scalar_map_runtime!(u64, u64);
scalar_map_runtime!(bool, u8);
scalar_map_runtime!(unicode, u32);

raw_scalar_map_runtime!(u8, u8);
raw_scalar_map_runtime!(u16, u16);
raw_scalar_map_runtime!(u32, u32);
raw_scalar_map_runtime!(u64, u64);
raw_scalar_map_runtime!(bool, u8);
raw_scalar_map_runtime!(unicode, u32);
raw_scalar_map_runtime!(i64, i64);

raw_string_map_runtime!();

#[doc(hidden)]
pub fn force_link_map() {
    force_link_map_u8_i64();
    force_link_map_u16_i64();
    force_link_map_u32_i64();
    force_link_map_u64_i64();
    force_link_map_bool_i64();
    force_link_map_unicode_i64();
    force_link_map_u8_raw();
    force_link_map_u16_raw();
    force_link_map_u32_raw();
    force_link_map_u64_raw();
    force_link_map_bool_raw();
    force_link_map_unicode_raw();
    force_link_map_i64_raw();
    force_link_map_string_raw();
    force_link_map_string_i64();
    force_link_map_i64_i64();
}

struct StringI64Map {
    map: HashMap<String, i64>,
    order: Vec<String>,
}

struct I64I64Map {
    map: HashMap<i64, i64>,
    order: Vec<i64>,
}

fn map_string_i64_mut(map: &mut SnaccMap) -> &mut StringI64Map {
    if map.ptr.is_null() {
        map.ptr = Box::into_raw(Box::new(StringI64Map {
            map: HashMap::new(),
            order: Vec::new(),
        }))
        .cast();
    }
    // Safety: a StringI64Map is installed by this module for this descriptor.
    unsafe { &mut *map.ptr.cast::<StringI64Map>() }
}

fn map_string_i64_ref(map: &SnaccMap) -> Option<&StringI64Map> {
    if map.ptr.is_null() {
        None
    } else {
        // Safety: a StringI64Map is installed by this module for this descriptor.
        Some(unsafe { &*map.ptr.cast::<StringI64Map>() })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_insert(
    map: *mut SnaccMap,
    key: *const SnaccString,
    value: i64,
) -> u8 {
    let map = unsafe { map.as_mut().expect("Map.insert received a null descriptor") };
    let key = unsafe { key.as_ref().expect("Map.insert received a null key") };
    let key = take_string(*key);
    let store = map_string_i64_mut(map);
    let fresh = !store.map.contains_key(&key);
    if fresh {
        store.order.push(key.clone());
    }
    store.map.insert(key, value);
    let (len, cap) = (store.map.len(), store.map.capacity());
    sync_map(map, len, cap);
    u8::from(fresh)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_contains(map: *const SnaccMap, key: *const SnaccView) -> u8 {
    let Some(map) = (unsafe { map.as_ref() }) else {
        return 0;
    };
    let key = unsafe { key.as_ref().expect("Map.contains received a null key") };
    let Some(key) = view_text(*key) else { return 0 };
    u8::from(map_string_i64_ref(map).is_some_and(|store| store.map.contains_key(key)))
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_key_at(map: *const SnaccMap, index: i64) -> SnaccString {
    let Some(map) = (unsafe { map.as_ref() }) else {
        snacc_collection_bounds_fail()
    };
    let Some(store) = map_string_i64_ref(map) else {
        snacc_collection_bounds_fail()
    };
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    let key = store
        .order
        .get(index)
        .unwrap_or_else(|| snacc_collection_bounds_fail());
    // The zero capacity marks this as a borrowed descriptor. Loop bindings
    // cannot consume it, and `String.clone()` copies its bytes before any
    // owning operation can occur.
    SnaccString {
        ptr: key.as_ptr().cast_mut(),
        len: key.len(),
        cap: 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_key_at_out(
    out: *mut SnaccString,
    map: *const SnaccMap,
    index: i64,
) {
    let out = unsafe { out.as_mut().expect("Map key lookup received a null output") };
    *out = snacc_map_string_i64_key_at(map, index);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_index(map: *const SnaccMap, key: *const SnaccView) -> i64 {
    let Some(map) = (unsafe { map.as_ref() }) else {
        snacc_collection_bounds_fail()
    };
    let key = unsafe { key.as_ref().expect("Map indexing received a null key") };
    let Some(key) = view_text(*key) else {
        snacc_collection_bounds_fail()
    };
    let Some(value) = map_string_i64_ref(map).and_then(|store| store.map.get(key)) else {
        snacc_collection_bounds_fail()
    };
    *value
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_delete(map: *mut SnaccMap, key: *const SnaccView) -> u8 {
    let map = unsafe { map.as_mut().expect("Map.delete received a null descriptor") };
    let key = unsafe { key.as_ref().expect("Map.delete received a null key") };
    let Some(key) = view_text(*key).map(str::to_owned) else {
        return 0;
    };
    let store = map_string_i64_mut(map);
    let existed = store.map.remove(&key).is_some();
    if existed {
        store.order.retain(|entry| entry != &key);
    }
    let (len, cap) = (store.map.len(), store.map.capacity());
    sync_map(map, len, cap);
    u8::from(existed)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_take(map: *mut SnaccMap, key: *const SnaccView) -> i64 {
    let map = unsafe { map.as_mut().expect("Map.take received a null descriptor") };
    let key = unsafe { key.as_ref().expect("Map.take received a null key") };
    let Some(key) = view_text(*key).map(str::to_owned) else {
        snacc_collection_bounds_fail()
    };
    let store = map_string_i64_mut(map);
    let Some(value) = store.map.remove(&key) else {
        snacc_collection_bounds_fail()
    };
    store.order.retain(|entry| entry != &key);
    let (len, cap) = (store.map.len(), store.map.capacity());
    sync_map(map, len, cap);
    value
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_clear(map: *mut SnaccMap) {
    let map = unsafe { map.as_mut().expect("Map.clear received a null descriptor") };
    let store = map_string_i64_mut(map);
    store.map.clear();
    store.order.clear();
    let cap = store.map.capacity();
    sync_map(map, 0, cap);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_reserve(map: *mut SnaccMap, minimum: i64) {
    let map = unsafe {
        map.as_mut()
            .expect("Map.reserve received a null descriptor")
    };
    let store = map_string_i64_mut(map);
    let additional = reserve_target(minimum, store.map.len());
    if additional != 0 {
        store.map.reserve(additional);
        store.order.reserve(additional);
    }
    let (len, cap) = (
        store.map.len(),
        store.map.capacity().max(store.order.capacity()),
    );
    sync_map(map, len, cap);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_string_i64_drop(map: *const SnaccMap) {
    let Some(map) = (unsafe { map.as_ref() }) else {
        return;
    };
    if !map.ptr.is_null() {
        // Safety: this descriptor was created for a StringI64Map by this module.
        unsafe { drop(Box::from_raw(map.ptr.cast::<StringI64Map>())) };
    }
}

fn map_i64_i64_mut(map: &mut SnaccMap) -> &mut I64I64Map {
    if map.ptr.is_null() {
        map.ptr = Box::into_raw(Box::new(I64I64Map {
            map: HashMap::new(),
            order: Vec::new(),
        }))
        .cast();
    }
    // Safety: an I64I64Map is installed by this module for this descriptor.
    unsafe { &mut *map.ptr.cast::<I64I64Map>() }
}

fn map_i64_i64_ref(map: &SnaccMap) -> Option<&I64I64Map> {
    if map.ptr.is_null() {
        None
    } else {
        // Safety: an I64I64Map is installed by this module for this descriptor.
        Some(unsafe { &*map.ptr.cast::<I64I64Map>() })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_insert(map: *mut SnaccMap, key: i64, value: i64) -> u8 {
    let map = unsafe { map.as_mut().expect("Map.insert received a null descriptor") };
    let store = map_i64_i64_mut(map);
    let fresh = !store.map.contains_key(&key);
    if fresh {
        store.order.push(key);
    }
    store.map.insert(key, value);
    let (len, cap) = (store.map.len(), store.map.capacity());
    sync_map(map, len, cap);
    u8::from(fresh)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_contains(map: *const SnaccMap, key: i64) -> u8 {
    let Some(map) = (unsafe { map.as_ref() }) else {
        return 0;
    };
    u8::from(map_i64_i64_ref(map).is_some_and(|store| store.map.contains_key(&key)))
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_index(map: *const SnaccMap, key: i64) -> i64 {
    let Some(map) = (unsafe { map.as_ref() }) else {
        snacc_collection_bounds_fail()
    };
    let Some(value) = map_i64_i64_ref(map).and_then(|store| store.map.get(&key)) else {
        snacc_collection_bounds_fail()
    };
    *value
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_key_at(map: *const SnaccMap, index: i64) -> i64 {
    let Some(map) = (unsafe { map.as_ref() }) else {
        snacc_collection_bounds_fail()
    };
    let Some(store) = map_i64_i64_ref(map) else {
        snacc_collection_bounds_fail()
    };
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    *store
        .order
        .get(index)
        .unwrap_or_else(|| snacc_collection_bounds_fail())
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_value_at(map: *const SnaccMap, index: i64) -> i64 {
    let Some(map) = (unsafe { map.as_ref() }) else {
        snacc_collection_bounds_fail()
    };
    let Some(store) = map_i64_i64_ref(map) else {
        snacc_collection_bounds_fail()
    };
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    let key = store
        .order
        .get(index)
        .unwrap_or_else(|| snacc_collection_bounds_fail());
    *store
        .map
        .get(key)
        .unwrap_or_else(|| snacc_collection_bounds_fail())
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_delete(map: *mut SnaccMap, key: i64) -> u8 {
    let map = unsafe { map.as_mut().expect("Map.delete received a null descriptor") };
    let store = map_i64_i64_mut(map);
    let existed = store.map.remove(&key).is_some();
    if existed {
        store.order.retain(|entry| entry != &key);
    }
    let (len, cap) = (store.map.len(), store.map.capacity());
    sync_map(map, len, cap);
    u8::from(existed)
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_take(map: *mut SnaccMap, key: i64) -> i64 {
    let map = unsafe { map.as_mut().expect("Map.take received a null descriptor") };
    let store = map_i64_i64_mut(map);
    let Some(value) = store.map.remove(&key) else {
        snacc_collection_bounds_fail()
    };
    store.order.retain(|entry| entry != &key);
    let (len, cap) = (store.map.len(), store.map.capacity());
    sync_map(map, len, cap);
    value
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_clear(map: *mut SnaccMap) {
    let map = unsafe { map.as_mut().expect("Map.clear received a null descriptor") };
    let store = map_i64_i64_mut(map);
    store.map.clear();
    store.order.clear();
    let cap = store.map.capacity();
    sync_map(map, 0, cap);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_reserve(map: *mut SnaccMap, minimum: i64) {
    let map = unsafe {
        map.as_mut()
            .expect("Map.reserve received a null descriptor")
    };
    let store = map_i64_i64_mut(map);
    let additional = reserve_target(minimum, store.map.len());
    if additional != 0 {
        store.map.reserve(additional);
        store.order.reserve(additional);
    }
    let (len, cap) = (
        store.map.len(),
        store.map.capacity().max(store.order.capacity()),
    );
    sync_map(map, len, cap);
}

#[unsafe(no_mangle)]
pub extern "C" fn snacc_map_i64_i64_drop(map: *const SnaccMap) {
    let Some(map) = (unsafe { map.as_ref() }) else {
        return;
    };
    if !map.ptr.is_null() {
        // Safety: this descriptor was created for an I64I64Map by this module.
        unsafe { drop(Box::from_raw(map.ptr.cast::<I64I64Map>())) };
    }
}

#[doc(hidden)]
pub fn force_link_map_string_i64() {
    let symbols = [
        snacc_map_string_i64_insert as *const () as usize,
        snacc_map_string_i64_contains as *const () as usize,
        snacc_map_string_i64_key_at as *const () as usize,
        snacc_map_string_i64_key_at_out as *const () as usize,
        snacc_map_string_i64_index as *const () as usize,
        snacc_map_string_i64_delete as *const () as usize,
        snacc_map_string_i64_take as *const () as usize,
        snacc_map_string_i64_clear as *const () as usize,
        snacc_map_string_i64_reserve as *const () as usize,
        snacc_map_string_i64_drop as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
pub fn force_link_map_i64_i64() {
    let symbols = [
        snacc_map_i64_i64_insert as *const () as usize,
        snacc_map_i64_i64_contains as *const () as usize,
        snacc_map_i64_i64_index as *const () as usize,
        snacc_map_i64_i64_key_at as *const () as usize,
        snacc_map_i64_i64_value_at as *const () as usize,
        snacc_map_i64_i64_delete as *const () as usize,
        snacc_map_i64_i64_take as *const () as usize,
        snacc_map_i64_i64_clear as *const () as usize,
        snacc_map_i64_i64_reserve as *const () as usize,
        snacc_map_i64_i64_drop as *const () as usize,
    ];
    std::hint::black_box(symbols);
}

#[doc(hidden)]
#[cfg(test)]
pub(crate) const MAP_STRING_I64_SYMBOLS: &[&str] = &[
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
];

#[doc(hidden)]
#[cfg(test)]
pub(crate) const MAP_I64_I64_SYMBOLS: &[&str] = &[
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
];
