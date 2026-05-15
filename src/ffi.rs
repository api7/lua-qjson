//! C ABI surface. Every public function is `unsafe extern "C"`.
//! All public symbols use the `qjd_` prefix.
//!
//! # Shared safety contract
//!
//! Most exports share the same FFI obligations on the caller:
//!
//! - A `*mut qjd_doc` argument must be NULL or a pointer previously returned
//!   by [`qjd_parse`] or [`qjd_decoder_parse`] that has not yet been passed
//!   to [`qjd_free`].
//! - A `*mut qjd_decoder` argument must be NULL or a pointer previously
//!   returned by [`qjd_decoder_new`] that has not yet been passed to
//!   [`qjd_decoder_free`].
//! - The input buffer passed to [`qjd_parse`] must remain valid and
//!   unmodified for the lifetime of the returned document.
//! - The input buffer passed to [`qjd_decoder_parse`] must remain valid and
//!   unmodified until the next [`qjd_decoder_parse`] / [`qjd_decoder_reset`]
//!   / [`qjd_decoder_destroy`] / [`qjd_decoder_free`] call on the same
//!   decoder, or any doc operation referencing it.
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
//! After [`qjd_decoder_parse`] is called on a decoder, all docs and cursors
//! produced by *prior* parses on that decoder become stale; operations on
//! them return [`qjd_err::QJD_STALE_DOC`] (Lua wrapper: `nil`). After
//! [`qjd_decoder_destroy`], all operations return `QJD_INVALID_ARG`.
//!
//! Every export catches Rust panics at the FFI boundary and converts them
//! to `QJD_OOM`; a panic must not be allowed to unwind across the boundary.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int};
use std::ptr::{self, NonNull};

use crate::decoder::{Decoder, DecoderState};
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

// ── Opaque types ────────────────────────────────────────────────────────────

/// Opaque type exported to C as `qjd_decoder*`.
pub struct qjd_decoder(pub(crate) Decoder);

/// Opaque type exported to C as `qjd_doc*`. A doc is a thin handle:
/// a pointer to the owning decoder plus the generation that was current
/// at the time the doc was produced. Successive [`qjd_decoder_parse`] (or
/// `reset` / `destroy`) calls bump the decoder's generation, so prior docs
/// detect they are stale via the gen check at every entry point.
pub struct qjd_doc {
    decoder:      NonNull<qjd_decoder>,
    gen:          u32,
    /// True for the one-shot [`qjd_parse`] path: the decoder is owned by
    /// this doc and is freed when the doc is freed. False for docs produced
    /// by [`qjd_decoder_parse`] (the user owns the decoder).
    owns_decoder: bool,
}

/// Heap layout for the one-shot [`qjd_parse`] path: `qjd_doc` and its
/// private `qjd_decoder` live in the same allocation. `#[repr(C)]` pins
/// `doc` at offset 0 so the public `*mut qjd_doc` returned to callers can
/// be cast back to `*mut OwnedDocBlock` on free.
#[repr(C)]
struct OwnedDocBlock {
    doc:     qjd_doc,
    decoder: qjd_decoder,
}

// ── Entry-point safety helpers ──────────────────────────────────────────────

/// Validate `doc` and return the live decoder. Order matters: a destroyed
/// decoder is reported as `QJD_INVALID_ARG`, not `QJD_STALE_DOC`.
///
/// Docs created by [`qjd_parse`] (`owns_decoder == true`) take a fast path:
/// the private decoder is unreachable to any of the `qjd_decoder_*` mutating
/// operations, so state and gen are pristine by construction.
unsafe fn check_doc_alive(doc: *mut qjd_doc) -> Result<&'static Decoder, qjd_err> {
    if doc.is_null() { return Err(qjd_err::QJD_INVALID_ARG); }
    let d = &*doc;
    if d.owns_decoder {
        // Legacy: the decoder is the sibling field in the same OwnedDocBlock.
        // Compute its address from the static struct offset — no pointer load,
        // matching the pre-pool layout where the decoder sat directly inside
        // the doc allocation.
        let block_ptr = doc as *const OwnedDocBlock;
        let dec: &Decoder = &(*block_ptr).decoder.0;
        return Ok(std::mem::transmute::<&Decoder, &'static Decoder>(dec));
    }
    let dec: &Decoder = &d.decoder.as_ref().0;
    if matches!(dec.state, DecoderState::Destroyed) {
        return Err(qjd_err::QJD_INVALID_ARG);
    }
    if dec.gen != d.gen {
        return Err(qjd_err::QJD_STALE_DOC);
    }
    Ok(std::mem::transmute::<&Decoder, &'static Decoder>(dec))
}

// ── strerror ────────────────────────────────────────────────────────────────

/// Return a static NUL-terminated message for the given error code.
///
/// # Safety
///
/// Has no preconditions. Marked `unsafe extern "C"` for C-ABI consistency
/// with the rest of the surface. The returned pointer is to static storage
/// and must not be freed.
#[no_mangle]
pub unsafe extern "C" fn qjd_strerror(code: c_int) -> *const c_char {
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
        9 => b"stale document or cursor\0",
        _ => b"unknown error code\0",
    };
    s.as_ptr() as *const c_char
}

// ── qjd_parse / qjd_free (one-shot, backward-compatible) ────────────────────

/// Parse a JSON buffer into a one-shot document (Phase 1: structural scan).
///
/// Internally allocates a private decoder owned by the returned document.
/// For repeated parses on hot paths, prefer [`qjd_decoder_new`] +
/// [`qjd_decoder_parse`].
///
/// # Safety
///
/// - `buf` must point to `len` readable bytes, or be NULL (in which case the
///   function returns NULL with `*err_out = QJD_INVALID_ARG`).
/// - `err_out` must point to a writable `int`, or be NULL (in which case the
///   function returns NULL with no error code written).
/// - The buffer must remain valid and unmodified for the lifetime of the
///   returned `qjd_doc*`; the underlying decoder borrows it.
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
        // Allocate the block first with a fresh Decoder, then parse in-place.
        // This avoids the ~100-byte stack→heap memcpy a naive `let mut decoder
        // = Decoder::new(); ... Box::new(...decoder)` path pays per parse.
        let block_ptr = Box::into_raw(Box::new(OwnedDocBlock {
            doc: qjd_doc {
                decoder:      NonNull::dangling(),
                gen:          0,
                owns_decoder: true,
            },
            decoder: qjd_decoder(Decoder::new()),
        }));
        (*block_ptr).doc.decoder = NonNull::new_unchecked(&mut (*block_ptr).decoder);
        let slice: &[u8] = std::slice::from_raw_parts(buf, len);
        match (*block_ptr).decoder.0.parse(slice) {
            Ok(()) => {
                (*block_ptr).doc.gen = (*block_ptr).decoder.0.gen;
                *err_out = qjd_err::QJD_OK as c_int;
                &mut (*block_ptr).doc
            }
            Err(e) => {
                let _ = Box::from_raw(block_ptr);
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

/// Free a document returned by [`qjd_parse`] or [`qjd_decoder_parse`].
/// NULL is a no-op. For docs produced by [`qjd_parse`], this also frees the
/// private decoder. For docs produced by [`qjd_decoder_parse`], the decoder
/// is left alone — free it with [`qjd_decoder_free`].
///
/// # Safety
///
/// `doc` must be NULL or a pointer previously returned by [`qjd_parse`] or
/// [`qjd_decoder_parse`] that has not yet been freed. Double-free or passing
/// a pointer not produced by those functions is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn qjd_free(doc: *mut qjd_doc) {
    if doc.is_null() { return; }
    // Read owns_decoder before taking ownership: the legacy doc lives inside
    // an OwnedDocBlock and must be freed through that layout, while a pool
    // doc is a standalone Box.
    if (*doc).owns_decoder {
        let _ = Box::from_raw(doc as *mut OwnedDocBlock);
    } else {
        let _ = Box::from_raw(doc);
    }
}

// ── qjd_decoder_* (pooled API) ──────────────────────────────────────────────

/// Allocate a reusable decoder. Returns NULL on allocation failure.
///
/// # Safety
///
/// Has no preconditions. The returned pointer must be freed exactly once
/// with [`qjd_decoder_free`].
#[no_mangle]
pub unsafe extern "C" fn qjd_decoder_new() -> *mut qjd_decoder {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Box::into_raw(Box::new(qjd_decoder(Decoder::new())))
    }));
    r.unwrap_or(std::ptr::null_mut())
}

/// Free a decoder returned by [`qjd_decoder_new`]. NULL is a no-op.
///
/// # Safety
///
/// `dec` must be NULL or a pointer previously returned by
/// [`qjd_decoder_new`] that has not yet been freed. All docs and cursors
/// produced by this decoder must have been freed first, or the caller must
/// ensure they are not used after this call.
#[no_mangle]
pub unsafe extern "C" fn qjd_decoder_free(dec: *mut qjd_decoder) {
    if dec.is_null() { return; }
    let _ = Box::from_raw(dec);
}

/// Reset a decoder: drop all cached state and release allocated capacity.
/// The decoder remains usable and its generation advances so any
/// outstanding docs/cursors become stale.
///
/// # Safety
///
/// `dec` must be NULL or a pointer previously returned by
/// [`qjd_decoder_new`] that has not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn qjd_decoder_reset(dec: *mut qjd_decoder) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !dec.is_null() { (*dec).0.reset(); }
    }));
}

/// Permanently retire a decoder. Frees the bulk internal buffers; the
/// decoder struct itself is freed when [`qjd_decoder_free`] is called.
/// After destroy, all subsequent decoder operations return errors and all
/// doc/cursor operations against docs produced by this decoder return
/// `QJD_INVALID_ARG`.
///
/// # Safety
///
/// `dec` must be NULL or a pointer previously returned by
/// [`qjd_decoder_new`] that has not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn qjd_decoder_destroy(dec: *mut qjd_decoder) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !dec.is_null() { (*dec).0.destroy(); }
    }));
}

/// Parse `buf` into `dec` and return a new doc handle. Any prior doc/cursor
/// produced by this decoder is invalidated (their generation no longer
/// matches and operations on them return `QJD_STALE_DOC`).
///
/// # Safety
///
/// - `dec` must be a live decoder pointer returned by [`qjd_decoder_new`].
///   NULL or a destroyed decoder yields `QJD_INVALID_ARG`.
/// - `buf` must point to `len` readable bytes. NULL is rejected with
///   `QJD_INVALID_ARG` even when `len == 0`, matching [`qjd_parse`].
/// - `err_out` must point to a writable `int`; NULL yields NULL with no
///   error code written.
/// - The buffer must remain valid and unmodified until the next
///   `qjd_decoder_parse` / `_reset` / `_destroy` / `_free` call on `dec`,
///   or any operation on a doc/cursor produced by this parse.
/// - On success, the returned pointer must be freed with [`qjd_free`].
#[no_mangle]
pub unsafe extern "C" fn qjd_decoder_parse(
    dec:     *mut qjd_decoder,
    buf:     *const u8,
    len:     usize,
    err_out: *mut c_int,
) -> *mut qjd_doc {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if err_out.is_null() { return ptr::null_mut(); }
        if dec.is_null() || buf.is_null() {
            *err_out = qjd_err::QJD_INVALID_ARG as c_int;
            return ptr::null_mut();
        }
        if matches!((*dec).0.state, DecoderState::Destroyed) {
            *err_out = qjd_err::QJD_INVALID_ARG as c_int;
            return ptr::null_mut();
        }
        let slice: &[u8] = std::slice::from_raw_parts(buf, len);
        match (*dec).0.parse(slice) {
            Ok(()) => {
                *err_out = qjd_err::QJD_OK as c_int;
                let doc = qjd_doc {
                    decoder:      NonNull::new_unchecked(dec),
                    gen:          (*dec).0.gen,
                    owns_decoder: false,
                };
                Box::into_raw(Box::new(doc))
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

// ── Root-path resolution helper ─────────────────────────────────────────────

use crate::cursor::Cursor;
use crate::error::qjd_type;

unsafe fn resolve_root_path(
    doc: *mut qjd_doc, path: *const c_char, path_len: usize,
) -> Result<(&'static Decoder, Cursor), qjd_err> {
    if path.is_null() && path_len != 0 {
        return Err(qjd_err::QJD_INVALID_ARG);
    }
    let d = check_doc_alive(doc)?;
    let p: &[u8] = if path.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(path as *const u8, path_len)
    };
    let cur = Cursor::root(d).resolve(d, p)?;
    Ok((std::mem::transmute::<&Decoder, &'static Decoder>(d), cur))
}

// ── Path-based getters ──────────────────────────────────────────────────────

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

/// Return the byte slice for a scalar value (number, true, false, null).
/// Uses the cursor convention: cur.idx_start is the position in indices of
/// the structural char AFTER the scalar (a separator or closer).
unsafe fn scalar_bytes(d: &Decoder, cur: Cursor) -> Result<&[u8], qjd_err> {
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

/// Turn a `*const qjd_cursor` into `(&Decoder, Cursor)` for Rust use, after
/// validating both the doc handle and the gen against the underlying decoder.
unsafe fn cursor_to_internal(c: *const qjd_cursor) -> Result<(&'static Decoder, Cursor), qjd_err> {
    if c.is_null() { return Err(qjd_err::QJD_INVALID_ARG); }
    let cc = &*c;
    if cc.doc.is_null() { return Err(qjd_err::QJD_INVALID_ARG); }
    let d = check_doc_alive(cc.doc as *mut qjd_doc)?;
    Ok((d, Cursor { idx_start: cc.idx_start, idx_end: cc.idx_end }))
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
