//! Real-world fixture manifest correctness gate (issue #139).
//!
//! Reads `tests/fixtures/manifest.json`, the single source of truth shared with
//! the Lua benchmark harness, and validates every declared access path against
//! the fixture file. Each check is a `type` + optional `value` + optional `len`
//! triple resolved through the public FFI getters, exactly as a consumer would.

use std::collections::BTreeSet;
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::ptr;

use qjson::error::qjson_err;
use qjson::ffi::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize)]
struct Fixture {
    id: String,
    path: String,
    payload_type: String,
    format: String,
    #[serde(default)]
    checks: Vec<Check>,
}

#[derive(Deserialize)]
struct Check {
    #[serde(default)]
    record: Option<usize>,
    path: String,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    value: Option<serde_json::Value>,
    #[serde(default)]
    len: Option<u64>,
}

const OK: c_int = qjson_err::QJSON_OK as c_int;

fn type_tag(name: &str) -> c_int {
    match name {
        "null" => 0,
        "bool" => 1,
        "number" => 2,
        "string" => 3,
        "array" => 4,
        "object" => 5,
        other => panic!("unknown check type {other:?}"),
    }
}

unsafe fn parse(buf: &[u8]) -> *mut qjson_doc {
    let mut err = qjson_error::default();
    let d = qjson_parse(buf.as_ptr(), buf.len(), &mut err);
    assert!(!d.is_null(), "parse failed (code={})", err.code);
    d
}

unsafe fn run_check(doc: *mut qjson_doc, ctx: &str, c: &Check) {
    let p = c.path.as_ptr() as *const c_char;
    let pl = c.path.len();

    let mut ty: c_int = -1;
    let rc = qjson_typeof(doc, p, pl, &mut ty);
    assert_eq!(rc, OK, "{ctx}: typeof rc={rc}");
    assert_eq!(ty, type_tag(&c.ty), "{ctx}: type mismatch");

    match c.ty.as_str() {
        "string" => {
            let mut sp: *const u8 = ptr::null();
            let mut sl: usize = 0;
            let rc = qjson_get_str(doc, p, pl, &mut sp, &mut sl);
            assert_eq!(rc, OK, "{ctx}: get_str rc={rc}");
            let s = std::slice::from_raw_parts(sp, sl);
            if let Some(v) = &c.value {
                let want = v.as_str().expect("string check `value` must be a string");
                assert_eq!(s, want.as_bytes(), "{ctx}: string value mismatch");
            }
            if let Some(l) = c.len {
                assert_eq!(sl as u64, l, "{ctx}: string byte-len mismatch");
            }
        }
        "number" => {
            if let Some(v) = &c.value {
                let mut out: f64 = 0.0;
                let rc = qjson_get_f64(doc, p, pl, &mut out);
                assert_eq!(rc, OK, "{ctx}: get_f64 rc={rc}");
                let want = v.as_f64().expect("number check `value` must be a number");
                assert_eq!(out, want, "{ctx}: number value mismatch");
            }
        }
        "bool" => {
            if let Some(v) = &c.value {
                let mut out: c_int = -1;
                let rc = qjson_get_bool(doc, p, pl, &mut out);
                assert_eq!(rc, OK, "{ctx}: get_bool rc={rc}");
                let want = v.as_bool().expect("bool check `value` must be a bool");
                assert_eq!(out != 0, want, "{ctx}: bool value mismatch");
            }
        }
        "null" => {
            let mut out: c_int = -1;
            let rc = qjson_is_null(doc, p, pl, &mut out);
            assert_eq!(rc, OK, "{ctx}: is_null rc={rc}");
            assert_eq!(out, 1, "{ctx}: expected null");
        }
        "object" | "array" => {
            if let Some(l) = c.len {
                let mut out: usize = 0;
                let rc = qjson_len(doc, p, pl, &mut out);
                assert_eq!(rc, OK, "{ctx}: len rc={rc}");
                assert_eq!(out as u64, l, "{ctx}: container len mismatch");
            }
        }
        _ => unreachable!(),
    }
}

/// Split an NDJSON buffer into non-empty records, trimming a trailing `\r`.
fn ndjson_records(buf: &[u8]) -> Vec<&[u8]> {
    buf.split(|&b| b == b'\n')
        .map(|l| {
            if l.last() == Some(&b'\r') {
                &l[..l.len() - 1]
            } else {
                l
            }
        })
        .filter(|l| !l.is_empty())
        .collect()
}

#[test]
fn manifest_fixtures_validate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let raw = fs::read(root.join("tests/fixtures/manifest.json"))
        .unwrap_or_else(|e| panic!("read manifest: {e}"));
    let manifest: Manifest = serde_json::from_slice(&raw).expect("parse manifest.json");

    assert_eq!(manifest.version, 1, "unexpected manifest version");
    assert!(
        manifest.fixtures.len() >= 5,
        "manifest must declare at least 5 fixtures"
    );

    let payload_types: BTreeSet<&str> = manifest
        .fixtures
        .iter()
        .map(|f| f.payload_type.as_str())
        .collect();
    assert!(
        payload_types.len() >= 5,
        "manifest must cover at least 5 payload types, got {payload_types:?}"
    );

    for f in &manifest.fixtures {
        let buf = fs::read(root.join(&f.path))
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", f.path));

        if f.format == "ndjson" {
            let records = ndjson_records(&buf);
            for c in &f.checks {
                let rec = c.record.unwrap_or(0);
                let line = records
                    .get(rec)
                    .unwrap_or_else(|| panic!("{}: record {rec} out of range", f.id));
                unsafe {
                    let doc = parse(line);
                    run_check(doc, &format!("{}#rec{rec}:{}", f.id, c.path), c);
                    qjson_free(doc);
                }
            }
        } else {
            unsafe {
                let doc = parse(&buf);
                for c in &f.checks {
                    run_check(doc, &format!("{}:{}", f.id, c.path), c);
                }
                qjson_free(doc);
            }
        }
    }
}
