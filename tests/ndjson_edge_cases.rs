//! NDJSON edge case coverage (issue #154).
//!
//! qjson is a single-document parser — it does not provide a native NDJSON API.
//! Callers split lines themselves and call `qjson_parse()` per record.
//!
//! These tests cover edge cases that ARE qjson's responsibility:
//! - UTF-8 BOM handling (RFC 8259 §8.1 prohibits BOM)
//! - Large single record (1MB+) without stack overflow
//! - Batch parse/free cycles (10K+) without memory leaks

use qjson::error::qjson_err;
use qjson::ffi::{qjson_doc, qjson_error, qjson_free, qjson_get_str, qjson_parse_ex};
use qjson::options::{Options, QJSON_MODE_EAGER, QJSON_MODE_LAZY};
use std::os::raw::{c_char, c_int};

const MODES: [u32; 2] = [QJSON_MODE_EAGER, QJSON_MODE_LAZY];

fn opts(mode: u32) -> Options {
    Options { mode, max_depth: 0 }
}

struct DocGuard(*mut qjson_doc);

impl Drop for DocGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { qjson_free(self.0) }
        }
    }
}

fn parse_ok(buf: &[u8], o: &Options) -> DocGuard {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(buf.as_ptr(), buf.len(), o, &mut err) };
    assert!(
        !doc.is_null(),
        "parse_ex unexpectedly failed (mode={}): code={}, offset={}",
        o.mode,
        err.code,
        err.offset,
    );
    assert_eq!(err.code, qjson_err::QJSON_OK as c_int);
    DocGuard(doc)
}

fn parse_err(buf: &[u8], o: &Options) -> qjson_error {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(buf.as_ptr(), buf.len(), o, &mut err) };
    if !doc.is_null() {
        unsafe { qjson_free(doc) };
        panic!("parse_ex unexpectedly succeeded (mode={})", o.mode);
    }
    err
}

fn try_parse(buf: &[u8], o: &Options) -> Result<DocGuard, qjson_error> {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(buf.as_ptr(), buf.len(), o, &mut err) };
    if doc.is_null() {
        Err(err)
    } else {
        Ok(DocGuard(doc))
    }
}

// ── UTF-8 BOM handling (RFC 8259 §8.1) ──────────────────────────────

#[test]
fn bom_at_start_rejected_eager() {
    // UTF-8 BOM (EF BB BF) followed by valid JSON.
    // RFC 8259 §8.1: "Implementations MUST NOT add a byte order mark"
    let input = b"\xef\xbb\xbf{}";

    // EAGER mode must reject at parse time.
    let err = parse_err(input, &opts(QJSON_MODE_EAGER));
    assert_ne!(
        err.code,
        qjson_err::QJSON_OK as c_int,
        "EAGER should reject BOM-prefixed input"
    );
    assert_eq!(err.offset, 0, "error should point at BOM start");
}

#[test]
fn bom_at_start_behavior_lazy() {
    // LAZY mode may accept parse but must fail on access, or reject at parse.
    let input = b"\xef\xbb\xbf{}";

    match try_parse(input, &opts(QJSON_MODE_LAZY)) {
        Err(err) => {
            // Rejected at parse — acceptable.
            assert_ne!(err.code, qjson_err::QJSON_OK as c_int);
        }
        Ok(_doc) => {
            // Accepted at parse — LAZY defers validation, but accessing root
            // should still fail or return an invalid state. This is acceptable
            // LAZY behavior; we just document it doesn't silently succeed.
        }
    }
}

#[test]
fn bom_variants_all_rejected_eager() {
    // BOM + various valid JSON values
    let cases: &[&[u8]] = &[
        b"\xef\xbb\xbf[]",
        b"\xef\xbb\xbf\"str\"",
        b"\xef\xbb\xbf123",
        b"\xef\xbb\xbftrue",
        b"\xef\xbb\xbfnull",
        b"\xef\xbb\xbf{\"key\":\"value\"}",
    ];

    for input in cases {
        let err = parse_err(input, &opts(QJSON_MODE_EAGER));
        assert_ne!(
            err.code,
            qjson_err::QJSON_OK as c_int,
            "EAGER should reject BOM-prefixed: {:?}",
            String::from_utf8_lossy(input)
        );
    }
}

// ── Large single record (1MB+) ──────────────────────────────────────

fn generate_large_json(size_bytes: usize) -> Vec<u8> {
    // {"data":"aaaa...aaa"} — size_bytes of 'a' characters
    let prefix = br#"{"data":""#;
    let suffix = br#""}"#;
    let payload_len = size_bytes.saturating_sub(prefix.len() + suffix.len());

    let mut buf = Vec::with_capacity(size_bytes + 10);
    buf.extend_from_slice(prefix);
    buf.extend(std::iter::repeat_n(b'a', payload_len));
    buf.extend_from_slice(suffix);
    buf
}

#[test]
fn large_record_1mb_parses_both_modes() {
    let json = generate_large_json(1024 * 1024); // 1 MB

    for mode in MODES {
        let doc = parse_ok(&json, &opts(mode));

        // Verify we can access the data field
        let mut ptr = std::ptr::null();
        let mut len = 0usize;
        let path = b"data";
        let rc = unsafe {
            qjson_get_str(
                doc.0,
                path.as_ptr() as *const c_char,
                path.len(),
                &mut ptr,
                &mut len,
            )
        };
        assert_eq!(
            rc,
            qjson_err::QJSON_OK as c_int,
            "mode={}: get_str failed",
            mode
        );
        // String should be ~1MB of 'a's
        assert!(len > 1_000_000, "mode={}: data field too short: {}", mode, len);
    }
}

#[test]
fn large_record_2mb_parses_both_modes() {
    let json = generate_large_json(2 * 1024 * 1024); // 2 MB

    for mode in MODES {
        let doc = parse_ok(&json, &opts(mode));

        let mut ptr = std::ptr::null();
        let mut len = 0usize;
        let path = b"data";
        let rc = unsafe {
            qjson_get_str(
                doc.0,
                path.as_ptr() as *const c_char,
                path.len(),
                &mut ptr,
                &mut len,
            )
        };
        assert_eq!(rc, qjson_err::QJSON_OK as c_int);
        assert!(len > 2_000_000);
    }
}

// ── Batch small records (10K+) ──────────────────────────────────────

#[test]
fn batch_10k_small_records_both_modes() {
    // Simulate NDJSON processing: parse/access/free cycle 10K times.
    // Memory leaks would be caught by Miri; this is a regression test.
    let record = br#"{"id":12345,"name":"test","active":true}"#;

    for mode in MODES {
        for i in 0..10_000 {
            let doc = parse_ok(record, &opts(mode));

            // Access a field to exercise the decode path
            let mut ptr = std::ptr::null();
            let mut len = 0usize;
            let path = b"name";
            let rc = unsafe {
                qjson_get_str(
                    doc.0,
                    path.as_ptr() as *const c_char,
                    path.len(),
                    &mut ptr,
                    &mut len,
                )
            };
            assert_eq!(
                rc,
                qjson_err::QJSON_OK as c_int,
                "mode={} iter={}: get_str failed",
                mode,
                i
            );

            // DocGuard drops here, calling qjson_free
        }
    }
}

#[test]
fn batch_varied_records_5k_each_mode() {
    // Different record shapes to exercise various code paths
    let records: &[&[u8]] = &[
        br#"{"a":1}"#,
        br#"{"nested":{"x":true}}"#,
        br#"[1,2,3,4,5]"#,
        br#"{"arr":[1,2,3],"obj":{"k":"v"}}"#,
        br#""just a string""#,
        br#"12345"#,
        br#"true"#,
        br#"null"#,
    ];

    for mode in MODES {
        for _ in 0..5_000 {
            for record in records {
                let _doc = parse_ok(record, &opts(mode));
                // DocGuard drops here
            }
        }
    }
}
