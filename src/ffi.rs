//! C ABI surface. Every public function is `unsafe extern "C"`.
//! All public symbols use the `qjd_` prefix.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::doc::Document;
use crate::error::qjd_err;

macro_rules! ffi_catch {
    ($body:block) => {{
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body));
        match r {
            Ok(code) => code,
            Err(_)   => qjd_err::QJD_OOM as c_int,
        }
    }};
}

/// Opaque type exported to C as `qjd_doc*`.
pub struct qjd_doc(pub(crate) Document<'static>);

#[no_mangle]
pub unsafe extern "C" fn qjd_strerror(code: c_int) -> *const c_char {
    // Hardcoded NUL-terminated map; avoids runtime allocation and lifetime issues.
    let s: &'static [u8] = match code {
        0 => b"ok\0",
        1 => b"JSON parse error\0",
        2 => b"path not found\0",
        3 => b"type mismatch at path\0",
        4 => b"numeric out of range\0",
        5 => b"decode failed\0",
        6 => b"invalid path syntax\0",
        7 => b"invalid argument\0",
        8 => b"out of memory\0",
        _ => b"unknown error code\0",
    };
    s.as_ptr() as *const c_char
}

#[no_mangle]
pub unsafe extern "C" fn qjd_parse(
    buf:     *const u8,
    len:     usize,
    err_out: *mut c_int,
) -> *mut qjd_doc {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if buf.is_null() || err_out.is_null() {
            if !err_out.is_null() { *err_out = qjd_err::QJD_INVALID_ARG as c_int; }
            return ptr::null_mut();
        }
        let slice: &'static [u8] = std::slice::from_raw_parts(buf, len);
        match Document::parse(slice) {
            Ok(d) => {
                *err_out = qjd_err::QJD_OK as c_int;
                Box::into_raw(Box::new(qjd_doc(d)))
            }
            Err(e) => {
                *err_out = e as c_int;
                ptr::null_mut()
            }
        }
    }));
    match r {
        Ok(p) => p,
        Err(_) => {
            if !err_out.is_null() { *err_out = qjd_err::QJD_OOM as c_int; }
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn qjd_free(doc: *mut qjd_doc) {
    if doc.is_null() { return; }
    let _ = Box::from_raw(doc);
}

use crate::cursor::Cursor;
use crate::error::qjd_type;

unsafe fn resolve_root_path(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize,
) -> Result<(&'static Document<'static>, Cursor), qjd_err> {
    if doc.is_null() || (path.is_null() && path_len != 0) {
        return Err(qjd_err::QJD_INVALID_ARG);
    }
    let d: &Document = &(*doc).0;
    let p: &[u8] = if path.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(path as *const u8, path_len)
    };
    let cur = Cursor::root(d).resolve(d, p)?;
    Ok((std::mem::transmute(d), cur))
}

#[no_mangle]
pub unsafe extern "C" fn qjd_typeof(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize, type_out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if type_out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        match resolve_root_path(doc, path, path_len) {
            Ok((d, cur)) => match d.type_of(cur) {
                Ok(t) => { *type_out = t as c_int; qjd_err::QJD_OK as c_int }
                Err(e) => e as c_int,
            },
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_is_null(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize, out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        match resolve_root_path(doc, path, path_len) {
            Ok((d, cur)) => match d.type_of(cur) {
                Ok(qjd_type::QJD_T_NULL) => { *out = 1; qjd_err::QJD_OK as c_int }
                Ok(_)                    => { *out = 0; qjd_err::QJD_OK as c_int }
                Err(e) => e as c_int,
            },
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_len(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize, out: *mut usize,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        match resolve_root_path(doc, path, path_len) {
            Ok((d, cur)) => match d.cursor_len(cur) {
                Ok(n) => { *out = n; qjd_err::QJD_OK as c_int }
                Err(e) => e as c_int,
            },
            Err(e) => e as c_int,
        }
    })
}

use crate::decode::number;
use crate::decode::string;

#[no_mangle]
pub unsafe extern "C" fn qjd_get_str(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize,
    out_ptr: *mut *const u8, out_len: *mut usize,
) -> c_int {
    ffi_catch!({
        if out_ptr.is_null() || out_len.is_null() {
            return qjd_err::QJD_INVALID_ARG as c_int;
        }
        let (d, cur) = match resolve_root_path(doc, path, path_len) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let pos = d.indices[cur.idx_start as usize] as usize;
        if d.buf.get(pos).copied() != Some(b'"') {
            return qjd_err::QJD_TYPE_MISMATCH as c_int;
        }
        // String ends at the close quote, whose indices position is idx_start + 1.
        let close = d.indices[(cur.idx_start + 1) as usize] as usize;

        let mut scratch = d.scratch.borrow_mut();
        match string::decode_string(d.buf, pos + 1, close, &mut scratch) {
            Ok((p, n)) => { *out_ptr = p; *out_len = n; qjd_err::QJD_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_get_i64(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize, out: *mut i64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match resolve_root_path(doc, path, path_len) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let bytes = match scalar_bytes(d, cur) {
            Ok(b) => b, Err(e) => return e as c_int,
        };
        match number::parse_i64(bytes) {
            Ok(v) => { *out = v; qjd_err::QJD_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_get_f64(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize, out: *mut f64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match resolve_root_path(doc, path, path_len) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let bytes = match scalar_bytes(d, cur) {
            Ok(b) => b, Err(e) => return e as c_int,
        };
        match number::parse_f64(bytes) {
            Ok(v) => { *out = v; qjd_err::QJD_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_get_bool(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize, out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match resolve_root_path(doc, path, path_len) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let bytes = match scalar_bytes(d, cur) {
            Ok(b) => b, Err(e) => return e as c_int,
        };
        match bytes {
            b"true"  => { *out = 1; qjd_err::QJD_OK as c_int }
            b"false" => { *out = 0; qjd_err::QJD_OK as c_int }
            _ => qjd_err::QJD_TYPE_MISMATCH as c_int,
        }
    })
}

/// Return the byte slice for a scalar value (number, true, false, null).
/// Uses the cursor convention: cur.idx_start is the position in indices of
/// the structural char AFTER the scalar (a separator or closer).
unsafe fn scalar_bytes<'d>(d: &'d Document<'d>, cur: Cursor) -> Result<&'d [u8], qjd_err> {
    // First byte: just after the previous structural char (skip whitespace).
    let start = d.find_scalar_start(cur.idx_start)?;
    // End byte: position of the structural char at cur.idx_start (exclusive).
    let end = d.indices[cur.idx_start as usize] as usize;
    if end < start { return Err(qjd_err::QJD_PARSE_ERROR); }
    // Strip trailing whitespace.
    let mut e = end;
    while e > start && matches!(d.buf[e - 1], b' '|b'\t'|b'\n'|b'\r') { e -= 1; }
    Ok(&d.buf[start..e])
}

// ── qjd_cursor type and cursor-based FFI ────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub struct qjd_cursor {
    pub doc:        *const qjd_doc,
    pub idx_start:  u32,
    pub idx_end:    u32,
    pub _reserved0: u32,
    pub _reserved1: u32,
}

/// Turn a `*const qjd_cursor` into `(&'static Document<'static>, Cursor)` for Rust use.
unsafe fn cursor_to_internal(c: *const qjd_cursor) -> Result<(&'static Document<'static>, Cursor), qjd_err> {
    if c.is_null() { return Err(qjd_err::QJD_INVALID_ARG); }
    let cc = &*c;
    if cc.doc.is_null() { return Err(qjd_err::QJD_INVALID_ARG); }
    let d: &Document = &(*(cc.doc as *mut qjd_doc)).0;
    Ok((std::mem::transmute(d), Cursor { idx_start: cc.idx_start, idx_end: cc.idx_end }))
}

fn internal_to_cursor(doc: *const qjd_doc, cur: Cursor) -> qjd_cursor {
    qjd_cursor {
        doc,
        idx_start:  cur.idx_start,
        idx_end:    cur.idx_end,
        _reserved0: 0,
        _reserved1: 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn qjd_open(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize, out: *mut qjd_cursor,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        match resolve_root_path(doc, path, path_len) {
            Ok((_, cur)) => {
                *out = internal_to_cursor(doc as *const qjd_doc, cur);
                qjd_err::QJD_OK as c_int
            }
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_open(
    c: *const qjd_cursor, path: *const c_char, path_len: usize, out: *mut qjd_cursor,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        match cur.resolve(d, p) {
            Ok(child) => { *out = internal_to_cursor((*c).doc, child); qjd_err::QJD_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_field(
    c: *const qjd_cursor, key: *const c_char, key_len: usize, out: *mut qjd_cursor,
) -> c_int {
    ffi_catch!({
        if out.is_null() || (key.is_null() && key_len != 0) {
            return qjd_err::QJD_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let k = if key.is_null() { &[][..] } else { std::slice::from_raw_parts(key as *const u8, key_len) };
        let child = match crate::cursor::resolve_single_key(d, cur, k) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        *out = internal_to_cursor((*c).doc, child);
        qjd_err::QJD_OK as c_int
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_index(
    c: *const qjd_cursor, i: usize, out: *mut qjd_cursor,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        if i > u32::MAX as usize { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let child = match crate::cursor::resolve_single_idx(d, cur, i as u32) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        *out = internal_to_cursor((*c).doc, child);
        qjd_err::QJD_OK as c_int
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_get_str(
    c: *const qjd_cursor, path: *const c_char, path_len: usize,
    out_ptr: *mut *const u8, out_len: *mut usize,
) -> c_int {
    ffi_catch!({
        if out_ptr.is_null() || out_len.is_null() {
            return qjd_err::QJD_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        let pos = d.indices[cur.idx_start as usize] as usize;
        if d.buf.get(pos).copied() != Some(b'"') {
            return qjd_err::QJD_TYPE_MISMATCH as c_int;
        }
        let close = d.indices[(cur.idx_start + 1) as usize] as usize;

        let mut scratch = d.scratch.borrow_mut();
        match string::decode_string(d.buf, pos + 1, close, &mut scratch) {
            Ok((p, n)) => { *out_ptr = p; *out_len = n; qjd_err::QJD_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_get_i64(
    c: *const qjd_cursor, path: *const c_char, path_len: usize, out: *mut i64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        let bytes = match scalar_bytes(d, cur) { Ok(b) => b, Err(e) => return e as c_int };
        match number::parse_i64(bytes) {
            Ok(v) => { *out = v; qjd_err::QJD_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_get_f64(
    c: *const qjd_cursor, path: *const c_char, path_len: usize, out: *mut f64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        let bytes = match scalar_bytes(d, cur) { Ok(b) => b, Err(e) => return e as c_int };
        match number::parse_f64(bytes) {
            Ok(v) => { *out = v; qjd_err::QJD_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_get_bool(
    c: *const qjd_cursor, path: *const c_char, path_len: usize, out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        let bytes = match scalar_bytes(d, cur) { Ok(b) => b, Err(e) => return e as c_int };
        match bytes {
            b"true"  => { *out = 1; qjd_err::QJD_OK as c_int }
            b"false" => { *out = 0; qjd_err::QJD_OK as c_int }
            _ => qjd_err::QJD_TYPE_MISMATCH as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_typeof(
    c: *const qjd_cursor, path: *const c_char, path_len: usize, type_out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if type_out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        match d.type_of(cur) {
            Ok(t) => { *type_out = t as c_int; qjd_err::QJD_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_len(
    c: *const qjd_cursor, path: *const c_char, path_len: usize, out: *mut usize,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjd_err::QJD_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        match d.cursor_len(cur) {
            Ok(n) => { *out = n; qjd_err::QJD_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

#[cfg(feature = "test-panic")]
#[no_mangle]
pub unsafe extern "C" fn qjd_test_panic() -> c_int {
    ffi_catch!({
        panic!("forced panic for test");
    })
}
