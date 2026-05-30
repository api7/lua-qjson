//! C ABI surface. Every public function is `unsafe extern "C"`.
//! All public symbols use the `qjson_` prefix.
//!
//! # Shared safety contract
//!
//! Most exports share the same FFI obligations on the caller:
//!
//! - A `*mut qjson_doc` argument must be NULL or a pointer previously returned
//!   by [`qjson_parse`] that has not yet been passed to [`qjson_free`].
//! - The input buffer passed to [`qjson_parse`] must remain valid and
//!   unmodified for as long as the document is alive; the document borrows it.
//! - Path / key pointer arguments must point to the indicated number of
//!   readable bytes, or be NULL when the length is `0`.
//! - Out pointers must be non-NULL and writable for their target type when
//!   the function returns `QJSON_OK`. Functions return `QJSON_INVALID_ARG`
//!   instead of writing through a NULL out pointer.
//! - A `*const qjson_cursor` must point to a cursor produced by one of
//!   [`qjson_open`], [`qjson_cursor_open`], [`qjson_cursor_field`], or
//!   [`qjson_cursor_index`], and whose `doc` field is still alive.
//! - A pointer/length pair returned by any `*_get_str`,
//!   `qjson_cursor_object_entry_at`, or `qjson_iter_next` call is invalidated
//!   by the next string-returning call on the same document (scratch buffer
//!   reuse).
//!
//! Every export catches Rust panics at the FFI boundary and converts them
//! to `QJSON_OOM`; a panic must not be allowed to unwind across the boundary.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::doc::Document;
use crate::error::qjson_err;

macro_rules! ffi_catch {
    ($body:block) => {{
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body));
        match r {
            Ok(code) => code,
            Err(_)   => qjson_err::QJSON_OOM as c_int,
        }
    }};
}

/// Opaque type exported to C as `qjson_doc*`.
pub struct qjson_doc(pub(crate) Document<'static>);

/// Return a static NUL-terminated message for the given error code.
///
/// # Safety
///
/// Has no preconditions. Marked `unsafe extern "C"` for C-ABI consistency
/// with the rest of the surface. The returned pointer is to static storage
/// and must not be freed.
#[no_mangle]
pub unsafe extern "C" fn qjson_strerror(code: c_int) -> *const c_char {
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
         9 => b"nesting depth exceeds limit\0",
        10 => b"trailing content after root value\0",
        11 => b"number out of representable range\0",
        12 => b"invalid number format (RFC 8259)\0",
        13 => b"invalid string content (unescaped control char)\0",
        14 => b"invalid UTF-8 in string\0",
         _ => b"unknown error code\0",
    };
    s.as_ptr() as *const c_char
}

/// Parse a JSON buffer into a document (Phase 1: structural scan).
///
/// # Safety
///
/// - `buf` must point to `len` readable bytes, or be NULL (in which case the
///   function returns NULL with `*err_out = QJSON_INVALID_ARG`).
/// - `err_out` may be NULL. When non-NULL it receives `QJSON_OK` on success or
///   an error code on failure.
/// - The buffer must remain valid and unmodified for the lifetime of the
///   returned `qjson_doc*`; the document borrows it.
/// - On success, the returned pointer must be freed exactly once with
///   [`qjson_free`].
#[no_mangle]
pub unsafe extern "C" fn qjson_parse(
    buf:     *const u8,
    len:     usize,
    err_out: *mut c_int,
) -> *mut qjson_doc {
    let default = crate::options::Options::default();
    qjson_parse_ex(buf, len, &default as *const _, err_out)
}

/// Parse with caller-supplied options. `opts` may be NULL to mean defaults
/// (eager mode, default max_depth).
///
/// # Safety
///
/// Same as `qjson_parse`, with the additional contract that `opts`, when
/// non-NULL, points to a readable `qjson_options` for the duration of the call
/// (the struct is copied internally).
#[no_mangle]
pub unsafe extern "C" fn qjson_parse_ex(
    buf:     *const u8,
    len:     usize,
    opts:    *const crate::options::Options,
    err_out: *mut c_int,
) -> *mut qjson_doc {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if buf.is_null() {
            if !err_out.is_null() { *err_out = qjson_err::QJSON_INVALID_ARG as c_int; }
            return ptr::null_mut();
        }
        let opts_owned = if opts.is_null() {
            crate::options::Options::default()
        } else {
            *opts
        };
        let slice: &'static [u8] = std::slice::from_raw_parts(buf, len);
        match Document::parse_with_options(slice, &opts_owned) {
            Ok(d) => {
                if !err_out.is_null() { *err_out = qjson_err::QJSON_OK as c_int; }
                Box::into_raw(Box::new(qjson_doc(d)))
            }
            Err(e) => {
                if !err_out.is_null() { *err_out = e as c_int; }
                ptr::null_mut()
            }
        }
    }));
    match r {
        Ok(p) => p,
        Err(_) => {
            if !err_out.is_null() { *err_out = qjson_err::QJSON_OOM as c_int; }
            std::ptr::null_mut()
        }
    }
}

/// Free a document returned by [`qjson_parse`]. NULL is a no-op.
///
/// # Safety
///
/// `doc` must be NULL or a pointer previously returned by [`qjson_parse`]
/// that has not yet been freed. Double-free or passing a pointer not
/// produced by `qjson_parse` is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn qjson_free(doc: *mut qjson_doc) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if doc.is_null() { return; }
        let _ = Box::from_raw(doc);
    }));
}

use crate::cursor::Cursor;
use crate::error::qjson_type;

unsafe fn resolve_root_path(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize,
) -> Result<(&'static Document<'static>, Cursor), qjson_err> {
    if doc.is_null() || (path.is_null() && path_len != 0) {
        return Err(qjson_err::QJSON_INVALID_ARG);
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

/// Write the JSON value type at `path` into `*type_out` (see [`qjson_type`]).
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `type_out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_typeof(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize, type_out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if type_out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        match resolve_root_path(doc, path, path_len) {
            Ok((d, cur)) => match d.type_of(cur) {
                Ok(t) => { *type_out = t as c_int; qjson_err::QJSON_OK as c_int }
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
pub unsafe extern "C" fn qjson_is_null(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize, out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        match resolve_root_path(doc, path, path_len) {
            Ok((d, cur)) => match d.type_of(cur) {
                Ok(qjson_type::QJSON_T_NULL) => { *out = 1; qjson_err::QJSON_OK as c_int }
                Ok(_)                    => { *out = 0; qjson_err::QJSON_OK as c_int }
                Err(e) => e as c_int,
            },
            Err(e) => e as c_int,
        }
    })
}

/// Write the number of direct children of the container at `path` into `*out`.
/// Returns `QJSON_TYPE_MISMATCH` if the target is not an array or object.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_len(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize, out: *mut usize,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        match resolve_root_path(doc, path, path_len) {
            Ok((d, cur)) => match d.cursor_len(cur) {
                Ok(n) => { *out = n; qjson_err::QJSON_OK as c_int }
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
/// `qjson_get_str` / `qjson_cursor_get_str` call on the same document**: the
/// scratch buffer used for escape decoding is reused. Callers must consume
/// the result (e.g. `ffi.string(p, n)` in LuaJIT) before issuing another
/// string read on the same document.
#[no_mangle]
pub unsafe extern "C" fn qjson_get_str(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize,
    out_ptr: *mut *const u8, out_len: *mut usize,
) -> c_int {
    ffi_catch!({
        if out_ptr.is_null() || out_len.is_null() {
            return qjson_err::QJSON_INVALID_ARG as c_int;
        }
        let (d, cur) = match resolve_root_path(doc, path, path_len) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let pos = d.indices[cur.idx_start as usize] as usize;
        if d.buf.get(pos).copied() != Some(b'"') {
            return qjson_err::QJSON_TYPE_MISMATCH as c_int;
        }
        // String ends at the close quote, whose indices position is idx_start + 1.
        let close = d.indices[(cur.idx_start + 1) as usize] as usize;

        let mut scratch = d.scratch.borrow_mut();
        match string::decode_string(d.buf, pos + 1, close, &mut scratch, d.eager_validated) {
            Ok((p, n)) => { *out_ptr = p; *out_len = n; qjson_err::QJSON_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

/// Parse the JSON number at `path` as `i64` and write into `*out`.
/// Returns `QJSON_OUT_OF_RANGE` if the value does not fit in `i64`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_get_i64(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize, out: *mut i64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match resolve_root_path(doc, path, path_len) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let bytes = match number_bytes(d, cur) {
            Ok(b) => b, Err(e) => return e as c_int,
        };
        match number::parse_i64(bytes, d.eager_validated) {
            Ok(v) => { *out = v; qjson_err::QJSON_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

/// Parse the JSON number at `path` as `u64` and write into `*out`.
/// Returns `QJSON_OUT_OF_RANGE` if the value does not fit in `u64`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_get_u64(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize, out: *mut u64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match resolve_root_path(doc, path, path_len) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let bytes = match number_bytes(d, cur) {
            Ok(b) => b, Err(e) => return e as c_int,
        };
        match number::parse_u64(bytes, d.eager_validated) {
            Ok(v) => { *out = v; qjson_err::QJSON_OK as c_int }
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
pub unsafe extern "C" fn qjson_get_f64(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize, out: *mut f64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match resolve_root_path(doc, path, path_len) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let bytes = match scalar_bytes(d, cur) {
            Ok(b) => b, Err(e) => return e as c_int,
        };
        match number::parse_f64(bytes, d.eager_validated) {
            Ok(v) => { *out = v; qjson_err::QJSON_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

/// Write `1` / `0` into `*out` for JSON `true` / `false` at `path`.
/// Returns `QJSON_TYPE_MISMATCH` for any other value.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `doc` must be live or NULL; `path` must point to `path_len` bytes or be
/// NULL with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_get_bool(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize, out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match resolve_root_path(doc, path, path_len) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let bytes = match scalar_bytes(d, cur) {
            Ok(b) => b, Err(e) => return e as c_int,
        };
        match bytes {
            b"true"  => { *out = 1; qjson_err::QJSON_OK as c_int }
            b"false" => { *out = 0; qjson_err::QJSON_OK as c_int }
            _ => qjson_err::QJSON_TYPE_MISMATCH as c_int,
        }
    })
}

/// Compute the byte range of a scalar value (number / true / false / null).
/// Uses the cursor convention: `cur.idx_start` is the position in indices
/// of the structural char AFTER the scalar (a separator or closer); the
/// scalar's bytes sit between `find_scalar_start(cur.idx_start)` and that
/// structural char, with trailing whitespace stripped.
unsafe fn scalar_byte_range(d: &Document<'_>, cur: Cursor) -> Result<(usize, usize), qjson_err> {
    let start = if d.is_root_scalar_cursor(cur) {
        d.root_scalar_start()
    } else {
        d.find_scalar_start(cur.idx_start)?
    };
    let end = if d.is_root_scalar_cursor(cur) {
        d.buf.len()
    } else {
        d.indices[cur.idx_start as usize] as usize
    };
    if end < start { return Err(qjson_err::QJSON_PARSE_ERROR); }
    let mut e = end;
    while e > start && matches!(d.buf[e - 1], b' '|b'\t'|b'\n'|b'\r') { e -= 1; }
    Ok((start, e))
}

/// Return the byte slice for a scalar value (number, true, false, null).
/// Uses the cursor convention: cur.idx_start is the position in indices of
/// the structural char AFTER the scalar (a separator or closer).
unsafe fn scalar_bytes<'d>(d: &'d Document<'d>, cur: Cursor) -> Result<&'d [u8], qjson_err> {
    let (s, e) = scalar_byte_range(d, cur)?;
    Ok(&d.buf[s..e])
}

unsafe fn number_bytes<'d>(d: &'d Document<'d>, cur: Cursor) -> Result<&'d [u8], qjson_err> {
    match d.type_of(cur)? {
        qjson_type::QJSON_T_NUM => scalar_bytes(d, cur),
        _                       => Err(qjson_err::QJSON_TYPE_MISMATCH),
    }
}

// ── qjson_cursor type and cursor-based FFI ────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub struct qjson_cursor {
    pub doc:        *const qjson_doc,
    pub idx_start:  u32,
    pub idx_end:    u32,
    pub _reserved0: u32,
    pub _reserved1: u32,
}

/// Stateful object iterator. Pure positional state; no heap resources.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct qjson_iter {
    pub doc:         *const qjson_doc,
    pub idx_current: u32,
    pub idx_end:     u32,
}

/// Turn a `*const qjson_cursor` into `(&'static Document<'static>, Cursor)` for Rust use.
unsafe fn cursor_to_internal(c: *const qjson_cursor) -> Result<(&'static Document<'static>, Cursor), qjson_err> {
    if c.is_null() { return Err(qjson_err::QJSON_INVALID_ARG); }
    let cc = &*c;
    if cc.doc.is_null() { return Err(qjson_err::QJSON_INVALID_ARG); }
    let d: &Document = &(*(cc.doc as *mut qjson_doc)).0;
    Ok((
        std::mem::transmute::<&Document<'_>, &'static Document<'static>>(d),
        Cursor { idx_start: cc.idx_start, idx_end: cc.idx_end },
    ))
}

fn internal_to_cursor(doc: *const qjson_doc, cur: Cursor) -> qjson_cursor {
    qjson_cursor {
        doc,
        idx_start:  cur.idx_start,
        idx_end:    cur.idx_end,
        _reserved0: 0,
        _reserved1: 0,
    }
}

unsafe fn iter_doc(it: *const qjson_iter) -> Result<&'static Document<'static>, qjson_err> {
    if it.is_null() { return Err(qjson_err::QJSON_INVALID_ARG); }
    let ii = &*it;
    if ii.doc.is_null() { return Err(qjson_err::QJSON_INVALID_ARG); }
    let d: &Document = &(*(ii.doc as *mut qjson_doc)).0;
    Ok(std::mem::transmute::<&Document<'_>, &'static Document<'static>>(d))
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
pub unsafe extern "C" fn qjson_open(
    doc: *mut qjson_doc, path: *const c_char, path_len: usize, out: *mut qjson_cursor,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        match resolve_root_path(doc, path, path_len) {
            Ok((_, cur)) => {
                *out = internal_to_cursor(doc as *const qjson_doc, cur);
                qjson_err::QJSON_OK as c_int
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
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_open(
    c: *const qjson_cursor, path: *const c_char, path_len: usize, out: *mut qjson_cursor,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        match cur.resolve(d, p) {
            Ok(child) => { *out = internal_to_cursor((*c).doc, child); qjson_err::QJSON_OK as c_int }
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
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `key` must point to `key_len` bytes or be NULL
/// with `key_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_field(
    c: *const qjson_cursor, key: *const c_char, key_len: usize, out: *mut qjson_cursor,
) -> c_int {
    ffi_catch!({
        if out.is_null() || (key.is_null() && key_len != 0) {
            return qjson_err::QJSON_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let k = if key.is_null() { &[][..] } else { std::slice::from_raw_parts(key as *const u8, key_len) };
        let child = match crate::cursor::resolve_single_key(d, cur, k) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        *out = internal_to_cursor((*c).doc, child);
        qjson_err::QJSON_OK as c_int
    })
}

/// Step into array index `i` and write the child cursor.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_index(
    c: *const qjson_cursor, i: usize, out: *mut qjson_cursor,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        if i > u32::MAX as usize { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let child = match crate::cursor::resolve_single_idx(d, cur, i as u32) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        *out = internal_to_cursor((*c).doc, child);
        qjson_err::QJSON_OK as c_int
    })
}

/// Decode the JSON string at `path` (relative to `*c`) and write `(ptr, len)`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out_ptr` and `out_len` must be non-NULL and
/// writable.
///
/// **The returned `(*out_ptr, *out_len)` pair is invalidated by the next
/// `qjson_get_str` / `qjson_cursor_get_str` call on the same document** (scratch
/// buffer reuse). See [`qjson_get_str`].
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_get_str(
    c: *const qjson_cursor, path: *const c_char, path_len: usize,
    out_ptr: *mut *const u8, out_len: *mut usize,
) -> c_int {
    ffi_catch!({
        if out_ptr.is_null() || out_len.is_null() {
            return qjson_err::QJSON_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        let pos = d.indices[cur.idx_start as usize] as usize;
        if d.buf.get(pos).copied() != Some(b'"') {
            return qjson_err::QJSON_TYPE_MISMATCH as c_int;
        }
        let close = d.indices[(cur.idx_start + 1) as usize] as usize;

        let mut scratch = d.scratch.borrow_mut();
        match string::decode_string(d.buf, pos + 1, close, &mut scratch, d.eager_validated) {
            Ok((p, n)) => { *out_ptr = p; *out_len = n; qjson_err::QJSON_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

/// Parse the JSON number at `path` (relative to `*c`) as `i64`.
/// Returns `QJSON_OUT_OF_RANGE` if the value does not fit in `i64`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_get_i64(
    c: *const qjson_cursor, path: *const c_char, path_len: usize, out: *mut i64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        let bytes = match number_bytes(d, cur) { Ok(b) => b, Err(e) => return e as c_int };
        match number::parse_i64(bytes, d.eager_validated) {
            Ok(v) => { *out = v; qjson_err::QJSON_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

/// Parse the JSON number at `path` (relative to `*c`) as `u64`.
/// Returns `QJSON_OUT_OF_RANGE` if the value does not fit in `u64`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_get_u64(
    c: *const qjson_cursor, path: *const c_char, path_len: usize, out: *mut u64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        let bytes = match number_bytes(d, cur) { Ok(b) => b, Err(e) => return e as c_int };
        match number::parse_u64(bytes, d.eager_validated) {
            Ok(v) => { *out = v; qjson_err::QJSON_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

/// Parse the JSON number at `path` (relative to `*c`) as `f64`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_get_f64(
    c: *const qjson_cursor, path: *const c_char, path_len: usize, out: *mut f64,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        let bytes = match scalar_bytes(d, cur) { Ok(b) => b, Err(e) => return e as c_int };
        match number::parse_f64(bytes, d.eager_validated) {
            Ok(v) => { *out = v; qjson_err::QJSON_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

/// Write `1` / `0` into `*out` for JSON `true` / `false` at `path`
/// (relative to `*c`). Returns `QJSON_TYPE_MISMATCH` for any other value.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_get_bool(
    c: *const qjson_cursor, path: *const c_char, path_len: usize, out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        let bytes = match scalar_bytes(d, cur) { Ok(b) => b, Err(e) => return e as c_int };
        match bytes {
            b"true"  => { *out = 1; qjson_err::QJSON_OK as c_int }
            b"false" => { *out = 0; qjson_err::QJSON_OK as c_int }
            _ => qjson_err::QJSON_TYPE_MISMATCH as c_int,
        }
    })
}

/// Write the JSON value type at `path` (relative to `*c`) into `*type_out`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `type_out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_typeof(
    c: *const qjson_cursor, path: *const c_char, path_len: usize, type_out: *mut c_int,
) -> c_int {
    ffi_catch!({
        if type_out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        match d.type_of(cur) {
            Ok(t) => { *type_out = t as c_int; qjson_err::QJSON_OK as c_int }
            Err(e) => e as c_int,
        }
    })
}

/// Write the number of direct children of the container at `path`
/// (relative to `*c`) into `*out`. Returns `QJSON_TYPE_MISMATCH` for non-containers.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `path` must point to `path_len` bytes or be NULL
/// with `path_len == 0`; `out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_len(
    c: *const qjson_cursor, path: *const c_char, path_len: usize, out: *mut usize,
) -> c_int {
    ffi_catch!({
        if out.is_null() { return qjson_err::QJSON_INVALID_ARG as c_int; }
        let (d, cur) = match cursor_to_internal(c) { Ok(x) => x, Err(e) => return e as c_int };
        let p: &[u8] = if path.is_null() { &[] } else {
            std::slice::from_raw_parts(path as *const u8, path_len)
        };
        let cur = match cur.resolve(d, p) { Ok(x) => x, Err(e) => return e as c_int };
        match d.cursor_len(cur) {
            Ok(n) => { *out = n; qjson_err::QJSON_OK as c_int }
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
/// `c` must point to a cursor produced by an earlier `qjson_*` call whose
/// document is still alive; `byte_start` and `byte_end` must be non-NULL
/// and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_bytes(
    c: *const qjson_cursor, byte_start: *mut usize, byte_end: *mut usize,
) -> c_int {
    ffi_catch!({
        if byte_start.is_null() || byte_end.is_null() {
            return qjson_err::QJSON_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        if d.is_root_scalar_cursor(cur) {
            let (s, e) = match scalar_byte_range(d, cur) {
                Ok(x) => x, Err(e) => return e as c_int,
            };
            *byte_start = s;
            *byte_end = e;
            return qjson_err::QJSON_OK as c_int;
        }

        let pos = d.indices[cur.idx_start as usize] as usize;
        let lead = match d.buf.get(pos) {
            Some(b) => *b,
            None => return qjson_err::QJSON_PARSE_ERROR as c_int,
        };
        match lead {
            b'{' | b'[' | b'"' => {
                // Container or string: span runs from opener to the matching
                // closer, inclusive.
                let end = d.indices[cur.idx_end as usize] as usize;
                if end >= d.buf.len() {
                    return qjson_err::QJSON_PARSE_ERROR as c_int;
                }
                *byte_start = pos;
                *byte_end = end + 1;
                qjson_err::QJSON_OK as c_int
            }
            _ => {
                // Scalar: delegate to scalar_byte_range.
                let (s, e) = match scalar_byte_range(d, cur) {
                    Ok(x) => x, Err(e) => return e as c_int,
                };
                *byte_start = s;
                *byte_end = e;
                qjson_err::QJSON_OK as c_int
            }
        }
    })
}

/// Write the i-th object entry's key (decoded into the doc's scratch
/// buffer) and value cursor into the out parameters.
///
/// Returns `QJSON_TYPE_MISMATCH` when the cursor is not an object, or
/// `QJSON_NOT_FOUND` when `i` is past the end.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a live cursor; `key_ptr`, `key_len`, and `value_out`
/// must be non-NULL and writable. The `(*key_ptr, *key_len)` pair is
/// invalidated by the next `qjson_get_str` / `qjson_cursor_get_str` /
/// `qjson_cursor_object_entry_at` call on the same document (scratch reuse).
#[no_mangle]
pub unsafe extern "C" fn qjson_cursor_object_entry_at(
    c: *const qjson_cursor, i: usize,
    key_ptr: *mut *const u8, key_len: *mut usize,
    value_out: *mut qjson_cursor,
) -> c_int {
    ffi_catch!({
        if key_ptr.is_null() || key_len.is_null() || value_out.is_null() {
            return qjson_err::QJSON_INVALID_ARG as c_int;
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
        match string::decode_string(d.buf, open_pos + 1, close_pos, &mut scratch, d.eager_validated) {
            Ok((p, n)) => {
                *key_ptr = p;
                *key_len = n;
                *value_out = internal_to_cursor((*c).doc, value_cur);
                qjson_err::QJSON_OK as c_int
            }
            Err(e) => e as c_int,
        }
    })
}

/// Initialize a stateful iterator over the object pointed to by `*c`.
///
/// Returns `QJSON_TYPE_MISMATCH` when the cursor is not an object. For an
/// empty object, initialization succeeds and the first `qjson_iter_next`
/// returns `QJSON_NOT_FOUND`.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a live cursor and `it` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_iter_init(
    c: *const qjson_cursor, it: *mut qjson_iter,
) -> c_int {
    ffi_catch!({
        if it.is_null() {
            return qjson_err::QJSON_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let pos = d.indices[cur.idx_start as usize] as usize;
        if d.buf.get(pos).copied() != Some(b'{') {
            return qjson_err::QJSON_TYPE_MISMATCH as c_int;
        }

        let closer_pos = d.indices[cur.idx_end as usize] as usize;
        let mut p = pos + 1;
        while p < closer_pos && matches!(d.buf[p], b' ' | b'\t' | b'\n' | b'\r') {
            p += 1;
        }
        let idx_current = if p == closer_pos { cur.idx_end } else { cur.idx_start + 1 };
        *it = qjson_iter {
            doc: (*c).doc,
            idx_current,
            idx_end: cur.idx_end,
        };
        qjson_err::QJSON_OK as c_int
    })
}

/// Advance a stateful object iterator by one key/value pair.
///
/// On success, writes the decoded key and a cursor for the value. Returns
/// `QJSON_NOT_FOUND` once exhausted. The returned key pointer follows the same
/// scratch lifetime contract as `qjson_cursor_object_entry_at` and is
/// invalidated by the next string-returning call on the same document.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `it`, `key_ptr`, `key_len`, and `value_out` must be non-NULL and writable.
#[no_mangle]
pub unsafe extern "C" fn qjson_iter_next(
    it: *mut qjson_iter,
    key_ptr: *mut *const u8, key_len: *mut usize,
    value_out: *mut qjson_cursor,
) -> c_int {
    ffi_catch!({
        if it.is_null() || key_ptr.is_null() || key_len.is_null() || value_out.is_null() {
            return qjson_err::QJSON_INVALID_ARG as c_int;
        }
        let d = match iter_doc(it) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let ii = &mut *it;
        if ii.idx_current >= ii.idx_end {
            return qjson_err::QJSON_NOT_FOUND as c_int;
        }

        let key_idx_start = ii.idx_current;
        let open_pos = d.indices[key_idx_start as usize] as usize;
        let close_pos = d.indices[(key_idx_start + 1) as usize] as usize;
        let colon_pos = d.indices[(key_idx_start + 2) as usize] as usize;
        if d.buf.get(open_pos).copied() != Some(b'"')
            || d.buf.get(close_pos).copied() != Some(b'"')
            || d.buf.get(colon_pos).copied() != Some(b':')
        {
            return qjson_err::QJSON_PARSE_ERROR as c_int;
        }

        let value_idx_start = key_idx_start + 3;
        let (cursor_end, skip_end) = match crate::cursor::find_value_span(d, value_idx_start) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let after_pos = d.indices[skip_end as usize] as usize;
        let after = match d.buf.get(after_pos).copied() {
            Some(b) => b,
            None => return qjson_err::QJSON_PARSE_ERROR as c_int,
        };

        let mut scratch = d.scratch.borrow_mut();
        match string::decode_string(d.buf, open_pos + 1, close_pos, &mut scratch, d.eager_validated) {
            Ok((p, n)) => {
                *key_ptr = p;
                *key_len = n;
                *value_out = internal_to_cursor(ii.doc, Cursor {
                    idx_start: value_idx_start,
                    idx_end:   cursor_end,
                });
                match after {
                    b',' => ii.idx_current = skip_end + 1,
                    b'}' => ii.idx_current = ii.idx_end,
                    _ => return qjson_err::QJSON_PARSE_ERROR as c_int,
                }
                qjson_err::QJSON_OK as c_int
            }
            Err(e) => e as c_int,
        }
    })
}

/// Test-only export that forces a Rust panic to verify the FFI panic barrier
/// converts it to `QJSON_OOM` instead of unwinding across the boundary.
///
/// # Safety
///
/// Has no preconditions. Marked `unsafe extern "C"` for ABI consistency.
#[cfg(feature = "test-panic")]
#[no_mangle]
pub unsafe extern "C" fn qjson_test_panic() -> c_int {
    ffi_catch!({
        panic!("forced panic for test");
    })
}
