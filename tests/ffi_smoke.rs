use std::ffi::CStr;

use qjson::ffi::{
    qjson_cursor, qjson_cursor_get_i64, qjson_doc, qjson_free, qjson_get_i64, qjson_open,
    qjson_error, qjson_parse, qjson_strerror,
};

#[test]
fn parse_and_free_roundtrip() {
    let json = b"{\"a\":1}";
    let mut err = qjson_error::default();
    let doc: *mut qjson_doc = unsafe { qjson_parse(json.as_ptr(), json.len(), &mut err) };
    assert!(!doc.is_null());
    assert_eq!(err.code, 0);
    unsafe { qjson_free(doc); }
}

#[test]
fn root_scalar_is_accessible_through_ffi() {
    let json = b"4";
    let mut err = qjson_error::default();
    let doc: *mut qjson_doc = unsafe { qjson_parse(json.as_ptr(), json.len(), &mut err) };
    assert!(!doc.is_null());
    assert_eq!(err.code, 0);

    let mut direct = 0i64;
    let rc = unsafe { qjson_get_i64(doc, std::ptr::null(), 0, &mut direct) };
    assert_eq!(rc, 0);
    assert_eq!(direct, 4);

    let mut cur = qjson_cursor {
        doc: std::ptr::null(),
        idx_start: 0,
        idx_end: 0,
        _reserved0: 0,
        _reserved1: 0,
    };
    let rc = unsafe { qjson_open(doc, std::ptr::null(), 0, &mut cur) };
    assert_eq!(rc, 0);

    let mut via_cursor = 0i64;
    let rc = unsafe { qjson_cursor_get_i64(&cur, std::ptr::null(), 0, &mut via_cursor) };
    assert_eq!(rc, 0);
    assert_eq!(via_cursor, 4);

    unsafe { qjson_free(doc); }
}

#[test]
fn parse_error_returns_null() {
    let bad = b"{";
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse(bad.as_ptr(), bad.len(), &mut err) };
    assert!(doc.is_null());
    assert_eq!(err.code, 1); // QJSON_PARSE_ERROR
}

#[test]
fn parse_null_buffer_returns_invalid_arg() {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse(std::ptr::null(), 0, &mut err) };
    assert!(doc.is_null());
    assert_eq!(err.code, 7); // QJSON_INVALID_ARG
}

#[test]
fn free_null_is_safe() {
    unsafe { qjson_free(std::ptr::null_mut()); }
}

#[test]
fn strerror_returns_non_empty() {
    for code in 0..=8 {
        let p = unsafe { qjson_strerror(code) };
        assert!(!p.is_null());
        let s = unsafe { CStr::from_ptr(p) }.to_str().unwrap();
        assert!(!s.is_empty(), "code {}", code);
    }
}
