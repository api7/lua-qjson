use std::ffi::CStr;
use std::os::raw::c_int;

use quickdecode::ffi::{qjd_doc, qjd_free, qjd_parse, qjd_strerror};

#[test]
fn parse_and_free_roundtrip() {
    let json = b"{\"a\":1}";
    let mut err: c_int = -1;
    let doc: *mut qjd_doc = unsafe { qjd_parse(json.as_ptr(), json.len(), &mut err) };
    assert!(!doc.is_null());
    assert_eq!(err, 0);
    unsafe { qjd_free(doc); }
}

#[test]
fn parse_error_returns_null() {
    let bad = b"{";
    let mut err: c_int = -1;
    let doc = unsafe { qjd_parse(bad.as_ptr(), bad.len(), &mut err) };
    assert!(doc.is_null());
    assert_eq!(err, 1); // QJD_PARSE_ERROR
}

#[test]
fn parse_null_buffer_returns_invalid_arg() {
    let mut err: c_int = -1;
    let doc = unsafe { qjd_parse(std::ptr::null(), 0, &mut err) };
    assert!(doc.is_null());
    assert_eq!(err, 7); // QJD_INVALID_ARG
}

#[test]
fn free_null_is_safe() {
    unsafe { qjd_free(std::ptr::null_mut()); }
}

#[test]
fn strerror_returns_non_empty() {
    for code in 0..=8 {
        let p = unsafe { qjd_strerror(code) };
        assert!(!p.is_null());
        let s = unsafe { CStr::from_ptr(p) }.to_str().unwrap();
        assert!(!s.is_empty(), "code {}", code);
    }
}
