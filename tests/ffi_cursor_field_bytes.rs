//! Integration tests for `qjson_cursor_field_bytes` — a fused field-lookup
//! plus byte-range query used by the lazy splice patch path.

use std::os::raw::c_int;
use std::ptr;

use qjson::error::qjson_err;
use qjson::ffi::{
    qjson_cursor, qjson_cursor_field_bytes, qjson_doc, qjson_free, qjson_open, qjson_parse,
};

unsafe fn open_root(json: &[u8]) -> (*mut qjson_doc, qjson_cursor) {
    let mut err: c_int = -1;
    let doc = qjson_parse(json.as_ptr(), json.len(), &mut err);
    assert!(!doc.is_null(), "parse failed: rc={}", err);
    let mut cur: qjson_cursor = std::mem::zeroed();
    let rc = qjson_open(doc, ptr::null(), 0, &mut cur);
    assert_eq!(rc, 0);
    (doc, cur)
}

#[test]
fn field_bytes_existing_key_scalar_value() {
    let json = br#"{"a":1,"b":"x"}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &root,
            b"a".as_ptr() as *const i8,
            1,
            &mut child,
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, qjson_err::QJSON_OK as c_int);
        assert_eq!(&json[bs..be], b"1");
        // child cursor is now positioned at the value; doc field must match
        assert_eq!(child.doc, doc as *const qjson_doc);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_string_value_with_escape_keeps_quotes() {
    // Syntactic span — must include the surrounding quotes, not the decoded
    // bytes. The "\\n" inside is two source bytes (backslash + n).
    let json = br#"{"k":"a\nb"}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &root,
            b"k".as_ptr() as *const i8,
            1,
            &mut child,
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, qjson_err::QJSON_OK as c_int);
        assert_eq!(&json[bs..be], br#""a\nb""#);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_nested_container_spans_entire_value() {
    let json = br#"{"k":{"x":1}}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &root,
            b"k".as_ptr() as *const i8,
            1,
            &mut child,
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, qjson_err::QJSON_OK as c_int);
        assert_eq!(&json[bs..be], br#"{"x":1}"#);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_nested_array_value_spans_entire_value() {
    let json = br#"{"k":[1,2,3]}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &root,
            b"k".as_ptr() as *const i8,
            1,
            &mut child,
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, qjson_err::QJSON_OK as c_int);
        assert_eq!(&json[bs..be], b"[1,2,3]");
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_missing_key_returns_not_found() {
    let json = br#"{"a":1,"b":2}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let mut bs: usize = 999;
        let mut be: usize = 999;
        let rc = qjson_cursor_field_bytes(
            &root,
            b"missing".as_ptr() as *const i8,
            7,
            &mut child,
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, qjson_err::QJSON_NOT_FOUND as c_int);
        // On NOT_FOUND we make no guarantee about the out parameters.
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_on_array_cursor_returns_type_mismatch() {
    // Mirrors qjson_cursor_field behavior — calling it on a non-object
    // cursor returns QJSON_TYPE_MISMATCH.
    let json = br#"[1,2,3]"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &root,
            b"a".as_ptr() as *const i8,
            1,
            &mut child,
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, qjson_err::QJSON_TYPE_MISMATCH as c_int);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_null_value_out_still_writes_byte_range() {
    let json = br#"{"k":42}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &root,
            b"k".as_ptr() as *const i8,
            1,
            ptr::null_mut(),
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, qjson_err::QJSON_OK as c_int);
        assert_eq!(&json[bs..be], b"42");
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_duplicate_keys_returns_first_occurrence() {
    let json = br#"{"a":1,"a":2}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &root,
            b"a".as_ptr() as *const i8,
            1,
            &mut child,
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, qjson_err::QJSON_OK as c_int);
        assert_eq!(&json[bs..be], b"1");
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_null_outputs_for_byte_range_return_invalid_arg() {
    let json = br#"{"a":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let rc = qjson_cursor_field_bytes(
            &root,
            b"a".as_ptr() as *const i8,
            1,
            &mut child,
            ptr::null_mut(),
            ptr::null_mut(),
        );
        assert_eq!(rc, qjson_err::QJSON_INVALID_ARG as c_int);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_scalar_strips_surrounding_whitespace() {
    let json = br#"{"k": 42 ,"x":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &root,
            b"k".as_ptr() as *const i8,
            1,
            &mut child,
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, qjson_err::QJSON_OK as c_int);
        assert_eq!(&json[bs..be], b"42");
        qjson_free(doc);
    }
}
