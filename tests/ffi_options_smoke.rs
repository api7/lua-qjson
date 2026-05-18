//! Smoke test for qjson_parse_ex and qjson_options C ABI.

use std::os::raw::c_int;

use qjson::ffi::{qjson_doc, qjson_free, qjson_parse, qjson_parse_ex};
use qjson::options::Options;

#[test]
fn parse_ex_default_options_matches_parse() {
    let buf = b"{\"a\":1}";
    let mut err: c_int = -1;
    let d1: *mut qjson_doc = unsafe { qjson_parse(buf.as_ptr(), buf.len(), &mut err) };
    assert!(!d1.is_null());
    assert_eq!(err, 0);

    let opts = Options { mode: 0, max_depth: 0 };
    let mut err2: c_int = -1;
    let d2: *mut qjson_doc = unsafe { qjson_parse_ex(buf.as_ptr(), buf.len(), &opts, &mut err2) };
    assert!(!d2.is_null());
    assert_eq!(err2, 0);

    unsafe { qjson_free(d1); qjson_free(d2); }
}

#[test]
fn parse_ex_null_opts_uses_defaults() {
    let buf = b"{}";
    let mut err: c_int = -1;
    let d: *mut qjson_doc = unsafe {
        qjson_parse_ex(buf.as_ptr(), buf.len(), std::ptr::null(), &mut err)
    };
    assert!(!d.is_null());
    assert_eq!(err, 0);
    unsafe { qjson_free(d) };
}

#[test]
fn parse_ex_null_err_returns_null_on_bad_buf() {
    let opts = Options { mode: 0, max_depth: 0 };
    let d: *mut qjson_doc = unsafe {
        qjson_parse_ex(std::ptr::null(), 0, &opts, std::ptr::null_mut())
    };
    assert!(d.is_null());
}
