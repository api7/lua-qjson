//! FFI-level integration tests for the pooled decoder API.
//!
//! Covers:
//! - Equivalence between the legacy `qjd_parse` path and the new
//!   `qjd_decoder_parse` path against the shipped fixtures.
//! - Stale-doc detection across re-parses, resets, and destroys.
//! - Cursor staleness derived through the doc's gen.
//! - Destroyed-decoder rejection.

use std::os::raw::c_int;
use std::ptr;

use quickdecode::ffi::*;

const FIXTURE_SMALL:  &[u8] = include_bytes!("../benches/fixtures/small_api.json");
const FIXTURE_MEDIUM: &[u8] = include_bytes!("../benches/fixtures/medium_resp.json");

// ── helpers ────────────────────────────────────────────────────────────────

fn dec_new() -> *mut qjd_decoder {
    let p = unsafe { qjd_decoder_new() };
    assert!(!p.is_null());
    p
}

fn dec_parse(dec: *mut qjd_decoder, buf: &[u8]) -> *mut qjd_doc {
    let mut err: c_int = -1;
    let d = unsafe { qjd_decoder_parse(dec, buf.as_ptr(), buf.len(), &mut err) };
    assert_eq!(err, 0, "qjd_decoder_parse unexpected err {}", err);
    assert!(!d.is_null());
    d
}

fn legacy_parse(buf: &[u8]) -> *mut qjd_doc {
    let mut err: c_int = -1;
    let d = unsafe { qjd_parse(buf.as_ptr(), buf.len(), &mut err) };
    assert_eq!(err, 0);
    assert!(!d.is_null());
    d
}

/// Try get_str. Returns (rc, value-on-success).
unsafe fn try_get_str(doc: *mut qjd_doc, path: &str) -> (c_int, Option<Vec<u8>>) {
    let mut p: *const u8 = ptr::null();
    let mut n: usize     = 0;
    let rc = qjd_get_str(doc, path.as_ptr() as *const i8, path.len(), &mut p, &mut n);
    let v = if rc == 0 { Some(std::slice::from_raw_parts(p, n).to_vec()) } else { None };
    (rc, v)
}

unsafe fn try_get_f64(doc: *mut qjd_doc, path: &str) -> (c_int, f64) {
    let mut v: f64 = 0.0;
    let rc = qjd_get_f64(doc, path.as_ptr() as *const i8, path.len(), &mut v);
    (rc, v)
}

unsafe fn try_typeof(doc: *mut qjd_doc, path: &str) -> (c_int, c_int) {
    let mut t: c_int = -1;
    let rc = qjd_typeof(doc, path.as_ptr() as *const i8, path.len(), &mut t);
    (rc, t)
}

unsafe fn try_len(doc: *mut qjd_doc, path: &str) -> (c_int, usize) {
    let mut n: usize = 0;
    let rc = qjd_len(doc, path.as_ptr() as *const i8, path.len(), &mut n);
    (rc, n)
}

/// Sweep a fixture's interesting paths and assert two docs agree on every
/// accessor return — both the rc and the produced value where rc == 0.
unsafe fn assert_doc_equivalence(legacy: *mut qjd_doc, pooled: *mut qjd_doc, probes: &[&str]) {
    for p in probes {
        let (l_rc, l_v) = try_get_str(legacy, p);
        let (r_rc, r_v) = try_get_str(pooled, p);
        assert_eq!((l_rc, &l_v), (r_rc, &r_v), "get_str mismatch at {}", p);

        let (l_rc, l_v) = try_get_f64(legacy, p);
        let (r_rc, r_v) = try_get_f64(pooled, p);
        assert_eq!(l_rc, r_rc, "get_f64 rc mismatch at {}", p);
        if l_rc == 0 {
            assert_eq!(l_v.to_bits(), r_v.to_bits(), "get_f64 value mismatch at {}", p);
        }

        let (l_rc, l_t) = try_typeof(legacy, p);
        let (r_rc, r_t) = try_typeof(pooled, p);
        assert_eq!((l_rc, l_t), (r_rc, r_t), "typeof mismatch at {}", p);

        let (l_rc, l_n) = try_len(legacy, p);
        let (r_rc, r_n) = try_len(pooled, p);
        assert_eq!((l_rc, l_n), (r_rc, r_n), "len mismatch at {}", p);
    }
}

// ── equivalence with the legacy qjd_parse path ─────────────────────────────

#[test]
fn decoder_path_matches_legacy_on_small_fixture() {
    let probes = &[
        "model", "temperature", "max_tokens", "top_p", "stream",
        "messages", "messages[0].role", "messages[0].content",
        "missing", "messages[100]",
    ];
    unsafe {
        let legacy = legacy_parse(FIXTURE_SMALL);
        let dec    = dec_new();
        let pooled = dec_parse(dec, FIXTURE_SMALL);
        assert_doc_equivalence(legacy, pooled, probes);
        qjd_free(legacy);
        qjd_free(pooled);
        qjd_decoder_free(dec);
    }
}

#[test]
fn decoder_path_matches_legacy_on_medium_fixture() {
    let probes = &[
        "id", "object", "created", "model",
        "choices", "choices[0].index", "choices[0].message.role",
        "choices[0].message.content", "choices[0].finish_reason",
        "usage", "usage.prompt_tokens", "usage.completion_tokens",
        "missing.path", "choices[99]",
    ];
    unsafe {
        let legacy = legacy_parse(FIXTURE_MEDIUM);
        let dec    = dec_new();
        let pooled = dec_parse(dec, FIXTURE_MEDIUM);
        assert_doc_equivalence(legacy, pooled, probes);
        qjd_free(legacy);
        qjd_free(pooled);
        qjd_decoder_free(dec);
    }
}

// ── stale-doc & cursor detection ───────────────────────────────────────────

#[test]
fn second_parse_marks_first_doc_stale() {
    unsafe {
        let dec  = dec_new();
        let doc1 = dec_parse(dec, b"{\"a\":1}");
        let doc2 = dec_parse(dec, b"{\"b\":2}");

        // doc2 is current and works.
        let mut v: i64 = 0;
        let rc = qjd_get_i64(doc2, b"b".as_ptr() as *const i8, 1, &mut v);
        assert_eq!(rc, 0);
        assert_eq!(v, 2);

        // doc1 is stale.
        let rc = qjd_get_i64(doc1, b"a".as_ptr() as *const i8, 1, &mut v);
        assert_eq!(rc, 9, "expected QJD_STALE_DOC, got {}", rc);

        qjd_free(doc1);
        qjd_free(doc2);
        qjd_decoder_free(dec);
    }
}

#[test]
fn cursor_opened_before_reparse_becomes_stale() {
    unsafe {
        let dec  = dec_new();
        let doc1 = dec_parse(dec, b"{\"a\":1,\"b\":[10,20,30]}");

        let mut cur = std::mem::MaybeUninit::<qjd_cursor>::uninit();
        let rc = qjd_open(doc1, b"b".as_ptr() as *const i8, 1, cur.as_mut_ptr());
        assert_eq!(rc, 0);
        let cur = cur.assume_init();

        // Reparse. Cursor opened against doc1 must now report stale.
        let doc2 = dec_parse(dec, b"{\"a\":2}");
        let mut n: usize = 0;
        let rc = qjd_cursor_len(&cur, ptr::null(), 0, &mut n);
        assert_eq!(rc, 9, "expected QJD_STALE_DOC, got {}", rc);

        qjd_free(doc1);
        qjd_free(doc2);
        qjd_decoder_free(dec);
    }
}

#[test]
fn reset_invalidates_outstanding_doc() {
    unsafe {
        let dec  = dec_new();
        let doc  = dec_parse(dec, b"{\"a\":1}");
        qjd_decoder_reset(dec);

        let mut v: i64 = 0;
        let rc = qjd_get_i64(doc, b"a".as_ptr() as *const i8, 1, &mut v);
        assert_eq!(rc, 9, "expected QJD_STALE_DOC after reset, got {}", rc);

        qjd_free(doc);
        qjd_decoder_free(dec);
    }
}

#[test]
fn decoder_is_reusable_after_reset() {
    unsafe {
        let dec  = dec_new();
        let doc1 = dec_parse(dec, b"{\"a\":1}");
        qjd_free(doc1);
        qjd_decoder_reset(dec);

        // Re-use is fine and the new doc works.
        let doc2 = dec_parse(dec, b"{\"b\":42}");
        let mut v: i64 = 0;
        let rc = qjd_get_i64(doc2, b"b".as_ptr() as *const i8, 1, &mut v);
        assert_eq!(rc, 0);
        assert_eq!(v, 42);

        qjd_free(doc2);
        qjd_decoder_free(dec);
    }
}

// ── destroy semantics ──────────────────────────────────────────────────────

#[test]
fn destroyed_decoder_rejects_parse() {
    unsafe {
        let dec = dec_new();
        qjd_decoder_destroy(dec);

        let mut err: c_int = -1;
        let d = qjd_decoder_parse(dec, b"{}".as_ptr(), 2, &mut err);
        assert!(d.is_null());
        assert_eq!(err, 7, "expected QJD_INVALID_ARG, got {}", err);

        qjd_decoder_free(dec);
    }
}

#[test]
fn destroyed_decoder_rejects_doc_ops_with_invalid_arg() {
    // Doc operations after destroy must return QJD_INVALID_ARG (terminal
    // state takes precedence over the gen check).
    unsafe {
        let dec  = dec_new();
        let doc  = dec_parse(dec, b"{\"a\":1}");
        qjd_decoder_destroy(dec);

        let mut v: i64 = 0;
        let rc = qjd_get_i64(doc, b"a".as_ptr() as *const i8, 1, &mut v);
        assert_eq!(rc, 7, "expected QJD_INVALID_ARG after destroy, got {}", rc);

        qjd_free(doc);
        qjd_decoder_free(dec);
    }
}

#[test]
fn destroy_is_idempotent_across_ffi() {
    unsafe {
        let dec = dec_new();
        qjd_decoder_destroy(dec);
        qjd_decoder_destroy(dec);   // second call is a no-op
        qjd_decoder_free(dec);
    }
}

// ── error paths ────────────────────────────────────────────────────────────

#[test]
fn null_decoder_yields_invalid_arg() {
    let mut err: c_int = -1;
    let d = unsafe { qjd_decoder_parse(ptr::null_mut(), b"{}".as_ptr(), 2, &mut err) };
    assert!(d.is_null());
    assert_eq!(err, 7);
}

#[test]
fn null_err_out_yields_null_silent() {
    let dec = dec_new();
    let d = unsafe {
        qjd_decoder_parse(dec, b"{}".as_ptr(), 2, ptr::null_mut())
    };
    assert!(d.is_null());
    unsafe { qjd_decoder_free(dec); }
}

#[test]
fn parse_error_does_not_destroy_decoder() {
    unsafe {
        let dec = dec_new();
        let mut err: c_int = -1;
        let d = qjd_decoder_parse(dec, b"{".as_ptr(), 1, &mut err);
        assert!(d.is_null());
        assert_eq!(err, 1, "expected QJD_PARSE_ERROR, got {}", err);

        // Decoder remains usable; a subsequent valid parse succeeds.
        let d = qjd_decoder_parse(dec, b"{\"x\":7}".as_ptr(), 7, &mut err);
        assert!(!d.is_null());
        assert_eq!(err, 0);
        qjd_free(d);
        qjd_decoder_free(dec);
    }
}

#[test]
fn free_null_doc_and_decoder_is_safe() {
    unsafe {
        qjd_free(ptr::null_mut());
        qjd_decoder_free(ptr::null_mut());
        qjd_decoder_reset(ptr::null_mut());
        qjd_decoder_destroy(ptr::null_mut());
    }
}

#[test]
fn legacy_parse_doc_works_independently_of_decoder_api() {
    // Sanity: even after spinning up a separate decoder, the legacy path
    // is unaffected.
    unsafe {
        let dec  = dec_new();
        let doc1 = dec_parse(dec, b"{\"x\":1}");
        let doc2 = legacy_parse(b"{\"y\":2}");

        let mut v: i64 = 0;
        let rc = qjd_get_i64(doc2, b"y".as_ptr() as *const i8, 1, &mut v);
        assert_eq!(rc, 0);
        assert_eq!(v, 2);

        // Re-parsing on the decoder must not stale-out the legacy doc.
        let _doc3 = dec_parse(dec, b"{\"z\":3}");
        let rc = qjd_get_i64(doc2, b"y".as_ptr() as *const i8, 1, &mut v);
        assert_eq!(rc, 0, "legacy doc must remain valid; got rc {}", rc);

        qjd_free(doc1);
        qjd_free(doc2);
        qjd_free(_doc3);
        qjd_decoder_free(dec);
    }
}
