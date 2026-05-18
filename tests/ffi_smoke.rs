use std::ffi::CStr;
use std::os::raw::c_int;

use qjson::ffi::{qjson_doc, qjson_free, qjson_parse, qjson_strerror};

#[test]
fn parse_and_free_roundtrip() {
    let json = b"{\"a\":1}";
    let mut err: c_int = -1;
    let doc: *mut qjson_doc = unsafe { qjson_parse(json.as_ptr(), json.len(), &mut err) };
    assert!(!doc.is_null());
    assert_eq!(err, 0);
    unsafe { qjson_free(doc); }
}

#[test]
fn parse_error_returns_null() {
    let bad = b"{";
    let mut err: c_int = -1;
    let doc = unsafe { qjson_parse(bad.as_ptr(), bad.len(), &mut err) };
    assert!(doc.is_null());
    assert_eq!(err, 1); // QJSON_PARSE_ERROR
}

#[test]
fn parse_null_buffer_returns_invalid_arg() {
    let mut err: c_int = -1;
    let doc = unsafe { qjson_parse(std::ptr::null(), 0, &mut err) };
    assert!(doc.is_null());
    assert_eq!(err, 7); // QJSON_INVALID_ARG
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
