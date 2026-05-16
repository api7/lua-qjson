//! C ABI surface. Every public function is `unsafe extern "C"`.
//! All public symbols use the `qjd_` prefix.
//!
//! # Shared safety contract
//!
//! Most exports share the same FFI obligations on the caller:
//!
//! - A `*mut qjd_doc` argument must be NULL or a pointer previously returned
//!   by [`qjd_parse`] that has not yet been passed to [`qjd_free`].
//! - The input buffer passed to [`qjd_parse`] must remain valid and
//!   unmodified for as long as the document is alive; the document borrows it.
//! - Path / key pointer arguments must point to the indicated number of
//!   readable bytes, or be NULL when the length is `0`.
//! - Out pointers must be non-NULL and writable for their target type when
//!   the function returns `QJD_OK`. Functions return `QJD_INVALID_ARG`
//!   instead of writing through a NULL out pointer.
//! - A `*const qjd_cursor` must point to a cursor produced by one of
//!   [`qjd_open`], [`qjd_cursor_open`], [`qjd_cursor_field`], or
//!   [`qjd_cursor_index`], and whose `doc` field is still alive.
//! - A pointer/length pair returned by any `*_get_str` is invalidated by
//!   the next `*_get_str` call on the same document (scratch buffer reuse).
//!
//! Every export catches Rust panics at the FFI boundary and converts them
//! to `QJD_OOM`; a panic must not be allowed to unwind across the boundary.

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

/// Return a static NUL-terminated message for the given error code.
///
/// # Safety
///
/// Has no preconditions. Marked `unsafe extern "C"` for C-ABI consistency
/// with the rest of the surface. The returned pointer is to static storage
/// and must not be freed.
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

/// Parse a JSON buffer into a document (Phase 1: structural scan).
///
/// # Safety
///
/// - `buf` must point to `len` readable bytes, or be NULL (in which case the
///   function returns NULL with `*err_out = QJD_INVALID_ARG`).
/// - `err_out` must point to a writable `int`, or be NULL (in which case the
///   function returns NULL with no error code written).
/// - The buffer must remain valid and unmodified for the lifetime of the
///   returned `qjd_doc*`; the document borrows it.
/// - On success, the returned pointer must be freed exactly once with
///   [`qjd_free`].
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

/// Free a document returned by [`qjd_parse`]. NULL is a no-op.
///
/// # Safety
///
/// `doc` must be NULL or a pointer previously returned by [`qjd_parse`]
/// that has not yet been freed. Double-free or passing a pointer not
/// produced by `qjd_parse` is undefined behavior.
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
    Ok((std::mem::transmute::<&Document<'_>, &'static Document<'static>>(d), cur))
}

/// Write the JSON value type at `path` into `*type_out` (see [`qjd_type`]).
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `type_out` must be non-NULL and writable.
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

/// Write `1` into `*out` if the value at `path` is JSON `null`, else `0`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Write the number of direct children of the container at `path` into `*out`.
/// Returns `QJD_TYPE_MISMATCH` if the target is not an array or object.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Decode the JSON string at `path` and write `(ptr, len)` into the outputs.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out_ptr` and `out_len` must be non-NULL and
/// writable.
///
/// **The returned `(*out_ptr, *out_len)` pair is invalidated by the next
/// `qjd_get_str` / `qjd_cursor_get_str` call on the same document**: the
/// scratch buffer used for escape decoding is reused. Callers must consume
/// the result (e.g. `ffi.string(p, n)` in LuaJIT) before issuing another
/// string read on the same document.
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

/// Parse the JSON number at `path` as `i64` and write into `*out`.
/// Returns `QJD_OUT_OF_RANGE` if the value does not fit in `i64`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Parse the JSON number at `path` as `f64` and write into `*out`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Write `1` / `0` into `*out` for JSON `true` / `false` at `path`.
/// Returns `QJD_TYPE_MISMATCH` for any other value.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Compute the byte range of a scalar value (number / true / false / null).
/// Uses the cursor convention: `cur.idx_start` is the position in indices
/// of the structural char AFTER the scalar (a separator or closer); the
/// scalar's bytes sit between `find_scalar_start(cur.idx_start)` and that
/// structural char, with trailing whitespace stripped.
unsafe fn scalar_byte_range(d: &Document<'_>, cur: Cursor) -> Result<(usize, usize), qjd_err> {
    let start = d.find_scalar_start(cur.idx_start)?;
    let end = d.indices[cur.idx_start as usize] as usize;
    if end < start { return Err(qjd_err::QJD_PARSE_ERROR); }
    let mut e = end;
    while e > start && matches!(d.buf[e - 1], b' '|b'\t'|b'\n'|b'\r') { e -= 1; }
    Ok((start, e))
}

/// Return the byte slice for a scalar value (number, true, false, null).
/// Uses the cursor convention: cur.idx_start is the position in indices of
/// the structural char AFTER the scalar (a separator or closer).
unsafe fn scalar_bytes<'d>(d: &'d Document<'d>, cur: Cursor) -> Result<&'d [u8], qjd_err> {
    let (s, e) = scalar_byte_range(d, cur)?;
    Ok(&d.buf[s..e])
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
    Ok((
        std::mem::transmute::<&Document<'_>, &'static Document<'static>>(d),
        Cursor { idx_start: cc.idx_start, idx_end: cc.idx_end },
    ))
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

/// Resolve `path` against the root of `doc` and write the resulting cursor
/// into `*out`. The cursor borrows the document — do not use after free.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Resolve `path` from the position `*c` and write the resulting cursor
/// into `*out`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Step into the object field named `key` and write the child cursor.
/// Lets the caller bypass path-string parsing for keys that contain `.` or `[`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `key` must point to `key_len` bytes or be NULL
/// with `key_len == 0`; `out` must be non-NULL and writable.
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

/// Step into array index `i` and write the child cursor.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `out` must be non-NULL and writable.
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

/// Decode the JSON string at `path` (relative to `*c`) and write `(ptr, len)`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out_ptr` and `out_len` must be non-NULL and
/// writable.
///
/// **The returned `(*out_ptr, *out_len)` pair is invalidated by the next
/// `qjd_get_str` / `qjd_cursor_get_str` call on the same document** (scratch
/// buffer reuse). See [`qjd_get_str`].
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

/// Parse the JSON number at `path` (relative to `*c`) as `i64`.
/// Returns `QJD_OUT_OF_RANGE` if the value does not fit in `i64`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Parse the JSON number at `path` (relative to `*c`) as `f64`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Write `1` / `0` into `*out` for JSON `true` / `false` at `path`
/// (relative to `*c`). Returns `QJD_TYPE_MISMATCH` for any other value.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Write the JSON value type at `path` (relative to `*c`) into `*type_out`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `type_out` must be non-NULL and writable.
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

/// Write the number of direct children of the container at `path`
/// (relative to `*c`) into `*out`. Returns `QJD_TYPE_MISMATCH` for non-containers.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
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

/// Write the original-buffer byte range `[byte_start, byte_end)` that the
/// cursor's value occupies. For containers, the range spans the opening
/// bracket through the closing bracket inclusive (so `byte_end` is one past
/// the close char). For scalars, leading and trailing whitespace and
/// surrounding separators are stripped (same convention as `scalar_bytes`).
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `byte_start` and `byte_end` must be non-NULL
/// and writable.
#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_bytes(
    c: *const qjd_cursor, byte_start: *mut usize, byte_end: *mut usize,
) -> c_int {
    ffi_catch!({
        if byte_start.is_null() || byte_end.is_null() {
            return qjd_err::QJD_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let pos = d.indices[cur.idx_start as usize] as usize;
        let lead = match d.buf.get(pos) {
            Some(b) => *b,
            None => return qjd_err::QJD_PARSE_ERROR as c_int,
        };
        match lead {
            b'{' | b'[' | b'"' => {
                // Container or string: span runs from opener to the matching
                // closer, inclusive.
                let end = d.indices[cur.idx_end as usize] as usize;
                if end >= d.buf.len() {
                    return qjd_err::QJD_PARSE_ERROR as c_int;
                }
                *byte_start = pos;
                *byte_end = end + 1;
                qjd_err::QJD_OK as c_int
            }
            _ => {
                // Scalar: delegate to scalar_byte_range.
                let (s, e) = match scalar_byte_range(d, cur) {
                    Ok(x) => x, Err(e) => return e as c_int,
                };
                *byte_start = s;
                *byte_end = e;
                qjd_err::QJD_OK as c_int
            }
        }
    })
}

/// Write the i-th object entry's key (decoded into the doc's scratch
/// buffer) and value cursor into the out parameters.
///
/// Returns `QJD_TYPE_MISMATCH` when the cursor is not an object, or
/// `QJD_NOT_FOUND` when `i` is past the end.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a live cursor; `key_ptr`, `key_len`, and `value_out`
/// must be non-NULL and writable. The `(*key_ptr, *key_len)` pair is
/// invalidated by the next `qjd_get_str` / `qjd_cursor_get_str` /
/// `qjd_cursor_object_entry_at` call on the same document (scratch reuse).
#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_object_entry_at(
    c: *const qjd_cursor, i: usize,
    key_ptr: *mut *const u8, key_len: *mut usize,
    value_out: *mut qjd_cursor,
) -> c_int {
    ffi_catch!({
        if key_ptr.is_null() || key_len.is_null() || value_out.is_null() {
            return qjd_err::QJD_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let (key_idx_start, value_cur) = match d.nth_object_entry(cur, i) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        // Decode the key: it sits at indices[key_idx_start..=key_idx_start+1]
        // — open quote at key_idx_start, close quote at key_idx_start+1.
        let open_pos = d.indices[key_idx_start as usize] as usize;
        let close_pos = d.indices[(key_idx_start + 1) as usize] as usize;
        let mut scratch = d.scratch.borrow_mut();
        match string::decode_string(d.buf, open_pos + 1, close_pos, &mut scratch) {
            Ok((p, n)) => {
                *key_ptr = p;
                *key_len = n;
                *value_out = internal_to_cursor((*c).doc, value_cur);
                qjd_err::QJD_OK as c_int
            }
            Err(e) => e as c_int,
        }
    })
}

/// Test-only export that forces a Rust panic to verify the FFI panic barrier
/// converts it to `QJD_OOM` instead of unwinding across the boundary.
///
/// # Safety
///
/// Has no preconditions. Marked `unsafe extern "C"` for ABI consistency.
#[cfg(feature = "test-panic")]
#[no_mangle]
pub unsafe extern "C" fn qjd_test_panic() -> c_int {
    ffi_catch!({
        panic!("forced panic for test");
    })
}
