//! Rust half of the spike. Built as a `staticlib` for `wasm32-unknown-emscripten`
//! and linked INTO dotnet.native.wasm by the .NET wasm SDK (NativeFileReference).
//!
//! Nothing in here touches JavaScript. The `extern "C"` block below binds to C
//! symbols that the Mono wasm build generates for C# methods marked
//! `[UnmanagedCallersOnly(EntryPoint = "...")]`. They are ordinary wasm functions
//! in the same module, resolved by wasm-ld at link time.

use std::ffi::{c_char, c_void, CStr, CString};

extern "C" {
    // C# exports (UnmanagedCallersOnly). Strings are malloc'd, NUL-terminated UTF-8,
    // owned by the caller and released via engine_free.
    fn engine_version() -> *mut c_char;
    fn engine_search(query: *const u8, query_len: usize) -> *mut c_char;
    fn engine_free(p: *mut c_void);
    fn engine_add(a: i32, b: i32) -> i32;
}

fn take_cstring(p: *mut c_char) -> String {
    if p.is_null() {
        return String::from("<null>");
    }
    // Copy out of the C# allocation, then hand it back to C# to free.
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { engine_free(p as *mut c_void) };
    s
}

/// Trivial .NET -> Rust call (no callbacks).
#[no_mangle]
pub extern "C" fn spike_add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// Proves C#-owned memory is directly readable from Rust: sums a byte buffer
/// that C# allocated (pinned managed array or NativeMemory) and passed by pointer.
#[no_mangle]
pub extern "C" fn spike_sum_bytes(p: *const u8, len: usize) -> u32 {
    if p.is_null() {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(p, len) };
    bytes.iter().map(|&b| b as u32).sum()
}

/// The round trip: C# calls this; it calls back into C# three times
/// (engine_add, engine_version, engine_search) and returns a report string.
/// The returned pointer is malloc'd by Rust; C# releases it with spike_free.
#[no_mangle]
pub extern "C" fn spike_run(query: *const u8, query_len: usize) -> *mut c_char {
    let query_str = if query.is_null() {
        String::new()
    } else {
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(query, query_len) }).into_owned()
    };

    let sum = unsafe { engine_add(40, 2) };
    let version = take_cstring(unsafe { engine_version() });
    let json = take_cstring(unsafe { engine_search(query_str.as_ptr(), query_str.len()) });

    // Cheap "did Rust std actually work" checks: heap alloc, formatting, iteration.
    let mut items: Vec<String> = Vec::new();
    items.push(format!("engine_add(40, 2) = {sum}"));
    items.push(format!("engine_version() = {version:?}"));
    items.push(format!("engine_search({query_str:?}) = {json}"));
    let report = format!(
        "[rust] spike_run called with query {:?} ({} bytes read directly at {:p})\n[rust] {}\n[rust] round trip complete; this report was built with std::format! and Vec, and is malloc'd by Rust",
        query_str,
        query_len,
        query,
        items.join("\n[rust] "),
    );
    CString::new(report).unwrap().into_raw()
}

/// Frees a string returned by spike_run.
#[no_mangle]
pub extern "C" fn spike_free(p: *mut c_char) {
    if !p.is_null() {
        unsafe { drop(CString::from_raw(p)) };
    }
}

/// Busy-loops for roughly `iters` iterations so C# can demonstrate that a long
/// synchronous native call blocks the browser's main thread.
#[no_mangle]
pub extern "C" fn spike_spin(iters: u64) -> u64 {
    let mut x: u64 = 0x9E3779B97F4A7C15;
    for i in 0..iters {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x = x.wrapping_add(i);
    }
    x
}
