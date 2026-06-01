//! Deterministic max-depth boundary stress tests (issue #140).
//!
//! `fuzz_depth` already explores randomized depth/shape combinations and the
//! EAGER/LAZY relation is fixed by `ffi_mode_relations`. These tests pin the
//! *named* boundaries that the fuzzer reaches only probabilistically:
//!
//! - the default `max_depth = 0 -> 1024` resolution at the FFI/parse level,
//! - the `4096` ceiling and the clamp of over-ceiling requests,
//! - object-only and mixed nesting (deterministic coverage is array-only
//!   elsewhere),
//! - a pathologically deep input rejecting fast without recursing, and
//! - cursor reachability of an accepted at-limit document.

use qjson::error::qjson_err;
use qjson::ffi::{
    qjson_cursor, qjson_cursor_bytes, qjson_cursor_index, qjson_cursor_len, qjson_doc, qjson_error,
    qjson_free, qjson_open, qjson_parse_ex,
};
use qjson::options::{
    Options, QJSON_DEFAULT_MAX_DEPTH, QJSON_MAX_MAX_DEPTH, QJSON_MODE_EAGER, QJSON_MODE_LAZY,
};
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int};

const MODES: [u32; 2] = [QJSON_MODE_EAGER, QJSON_MODE_LAZY];

fn opts(mode: u32, max_depth: u32) -> Options {
    Options { mode, max_depth }
}

struct DocGuard(*mut qjson_doc);

impl Drop for DocGuard {
    fn drop(&mut self) {
        unsafe { qjson_free(self.0) }
    }
}

fn parse_ok(buf: &[u8], o: &Options) -> DocGuard {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(buf.as_ptr(), buf.len(), o, &mut err) };
    assert!(
        !doc.is_null(),
        "parse_ex unexpectedly failed (mode={} max_depth={}): {:?}",
        o.mode,
        o.max_depth,
        err,
    );
    assert_eq!(err.code, qjson_err::QJSON_OK as c_int);
    DocGuard(doc)
}

fn parse_err(buf: &[u8], o: &Options) -> qjson_error {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(buf.as_ptr(), buf.len(), o, &mut err) };
    assert!(
        doc.is_null(),
        "parse_ex unexpectedly succeeded (mode={} max_depth={})",
        o.mode,
        o.max_depth,
    );
    err
}

// ── nesting skeletons ───────────────────────────────────────────────

/// `depth` nested arrays around a scalar `0`.
fn nested_arrays(depth: usize) -> Vec<u8> {
    let mut buf = vec![b'['; depth];
    buf.push(b'0');
    buf.extend(std::iter::repeat_n(b']', depth));
    buf
}

/// `depth` nested single-key objects around a scalar `0`. Each level is the
/// 5-byte `{"k":` prefix, so the opening brace of level `n` (1-based) sits at
/// byte `(n - 1) * 5`.
fn nested_objects(depth: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(depth * 6 + 2);
    for _ in 0..depth {
        buf.extend_from_slice(br#"{"k":"#);
    }
    buf.push(b'0');
    buf.extend(std::iter::repeat_n(b'}', depth));
    buf
}

/// Alternating array / object nesting (array at even levels, object at odd).
fn nested_mixed(depth: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(depth * 6 + 2);
    let mut closers = Vec::with_capacity(depth);
    for level in 0..depth {
        if level % 2 == 0 {
            buf.push(b'[');
            closers.push(b']');
        } else {
            buf.extend_from_slice(br#"{"k":"#);
            closers.push(b'}');
        }
    }
    buf.push(b'0');
    closers.reverse();
    buf.extend_from_slice(&closers);
    buf
}

// ── default depth (max_depth = 0 -> 1024) ───────────────────────────

#[test]
fn default_depth_accepts_1024_rejects_1025_both_modes() {
    let at_limit = nested_arrays(QJSON_DEFAULT_MAX_DEPTH as usize);
    let one_past = nested_arrays(QJSON_DEFAULT_MAX_DEPTH as usize + 1);

    for mode in MODES {
        // max_depth = 0 must resolve to the documented 1024 default.
        let _doc = parse_ok(&at_limit, &opts(mode, 0));

        let err = parse_err(&one_past, &opts(mode, 0));
        assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as c_int);
        // Offset points at the opening bracket that first exceeds the limit:
        // the (1025th) '[' lives at byte index 1024.
        assert_eq!(err.offset, QJSON_DEFAULT_MAX_DEPTH as usize);
    }
}

// ── 4096 ceiling and clamp ──────────────────────────────────────────

#[test]
fn ceiling_accepts_4096_rejects_4097_both_modes() {
    let at_ceiling = nested_arrays(QJSON_MAX_MAX_DEPTH as usize);
    let one_past = nested_arrays(QJSON_MAX_MAX_DEPTH as usize + 1);

    for mode in MODES {
        let _doc = parse_ok(&at_ceiling, &opts(mode, QJSON_MAX_MAX_DEPTH));

        let err = parse_err(&one_past, &opts(mode, QJSON_MAX_MAX_DEPTH));
        assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as c_int);
        assert_eq!(err.offset, QJSON_MAX_MAX_DEPTH as usize);
    }
}

#[test]
fn request_above_ceiling_clamps_to_4096_not_honored() {
    let at_ceiling = nested_arrays(QJSON_MAX_MAX_DEPTH as usize);
    let one_past = nested_arrays(QJSON_MAX_MAX_DEPTH as usize + 1);

    // Both an explicit over-ceiling request and u32::MAX must behave exactly
    // like max_depth = 4096 (clamp, not honor the larger request).
    for requested in [QJSON_MAX_MAX_DEPTH + 1, 5000, u32::MAX] {
        for mode in MODES {
            let _doc = parse_ok(&at_ceiling, &opts(mode, requested));

            let err = parse_err(&one_past, &opts(mode, requested));
            assert_eq!(
                err.code,
                qjson_err::QJSON_NESTING_TOO_DEEP as c_int,
                "requested={requested} should clamp to {QJSON_MAX_MAX_DEPTH}",
            );
            assert_eq!(err.offset, QJSON_MAX_MAX_DEPTH as usize);
        }
    }
}

// ── object-only / mixed nesting ─────────────────────────────────────

#[test]
fn object_only_depth_boundary_both_modes() {
    let limit = 8u32;
    let at_limit = nested_objects(limit as usize);
    let one_past = nested_objects(limit as usize + 1);

    for mode in MODES {
        let _doc = parse_ok(&at_limit, &opts(mode, limit));

        let err = parse_err(&one_past, &opts(mode, limit));
        assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as c_int);
        // The (limit+1)-th '{' opens at byte limit * 5 (5-byte `{"k":` levels).
        assert_eq!(err.offset, limit as usize * 5);
    }
}

#[test]
fn mixed_nesting_depth_boundary_both_modes() {
    let limit = 9u32; // odd -> ends on an object level, exercising both kinds
    let at_limit = nested_mixed(limit as usize);
    let one_past = nested_mixed(limit as usize + 1);

    for mode in MODES {
        let _doc = parse_ok(&at_limit, &opts(mode, limit));

        let err = parse_err(&one_past, &opts(mode, limit));
        assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as c_int);
    }
}

// ── pathological depth: fast reject, no recursion ───────────────────

#[test]
fn pathological_depth_rejects_without_stack_overflow() {
    // Far beyond the ceiling: an honest recursive-descent parser would blow
    // the stack here. qjson walks `indices` iteratively, so this must return
    // a clean NESTING_TOO_DEEP at the default limit's boundary.
    let deep = nested_arrays(100_000);
    for mode in MODES {
        let err = parse_err(&deep, &opts(mode, 0));
        assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as c_int);
        assert_eq!(err.offset, QJSON_DEFAULT_MAX_DEPTH as usize);
    }
}

// ── cursor reachability of an at-limit accepted document ────────────

#[test]
fn at_limit_document_is_reachable_via_cursor() {
    let depth = QJSON_DEFAULT_MAX_DEPTH as usize;
    let json = nested_arrays(depth);

    for mode in MODES {
        let doc = parse_ok(&json, &opts(mode, 0));
        unsafe {
            let mut cur = MaybeUninit::<qjson_cursor>::uninit();
            assert_eq!(
                qjson_open(doc.0, std::ptr::null::<c_char>(), 0, cur.as_mut_ptr()),
                qjson_err::QJSON_OK as c_int,
            );
            let mut cur = cur.assume_init();

            // Descend index 0 at every level down to the innermost scalar.
            for _ in 0..depth {
                let mut len = usize::MAX;
                assert_eq!(
                    qjson_cursor_len(&cur, std::ptr::null::<c_char>(), 0, &mut len),
                    qjson_err::QJSON_OK as c_int,
                );
                assert_eq!(len, 1);

                let mut next = MaybeUninit::<qjson_cursor>::uninit();
                assert_eq!(
                    qjson_cursor_index(&cur, 0, next.as_mut_ptr()),
                    qjson_err::QJSON_OK as c_int,
                );
                cur = next.assume_init();
            }

            // The final cursor is the scalar `0`; its byte span must be valid.
            let mut start = usize::MAX;
            let mut end = usize::MAX;
            assert_eq!(
                qjson_cursor_bytes(&cur, &mut start, &mut end),
                qjson_err::QJSON_OK as c_int,
            );
            assert!(start < end && end <= json.len());
            assert_eq!(&json[start..end], b"0");
        }
    }
}
