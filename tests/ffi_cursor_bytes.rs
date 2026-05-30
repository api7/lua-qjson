use std::os::raw::c_int;
use std::ptr;

use qjson::error::qjson_err;
use qjson::ffi::{
    qjson_cursor, qjson_cursor_bytes, qjson_cursor_field, qjson_doc, qjson_error, qjson_free,
    qjson_open, qjson_parse,
};

unsafe fn open_root(json: &[u8]) -> (*mut qjson_doc, qjson_cursor) {
    let mut err = qjson_error::default();
    let doc = qjson_parse(json.as_ptr(), json.len(), &mut err);
    assert!(!doc.is_null());
    let mut cur: qjson_cursor = std::mem::zeroed();
    let rc = qjson_open(doc, ptr::null(), 0, &mut cur);
    assert_eq!(rc, 0);
    (doc, cur)
}

#[test]
fn bytes_of_root_object_covers_full_json() {
    let json = br#"{"a":1,"b":[2,3]}"#;
    unsafe {
        let (doc, cur) = open_root(json);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_bytes(&cur, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], json.as_ref());
        qjson_free(doc);
    }
}

#[test]
fn bytes_of_string_value_is_quoted_span() {
    let json = br#"{"k":"hello"}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let rc = qjson_cursor_field(&root, b"k".as_ptr() as *const i8, 1, &mut child);
        assert_eq!(rc, 0);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_bytes(&child, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], br#""hello""#);
        qjson_free(doc);
    }
}

#[test]
fn bytes_of_number_value_strips_separators() {
    let json = br#"{"k": 42 ,"x":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjson_cursor = std::mem::zeroed();
        let rc = qjson_cursor_field(&root, b"k".as_ptr() as *const i8, 1, &mut child);
        assert_eq!(rc, 0);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_bytes(&child, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], b"42");
        qjson_free(doc);
    }
}

#[test]
fn bytes_with_null_out_pointer_returns_invalid_arg() {
    let json = br#"{"a":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let rc = qjson_cursor_bytes(&root, ptr::null_mut(), ptr::null_mut());
        assert_eq!(rc, qjson_err::QJSON_INVALID_ARG as c_int);
        qjson_free(doc);
    }
}

#[test]
fn bytes_of_root_array_covers_full_json() {
    let json = br#"[1,"two",true]"#;
    unsafe {
        let (doc, cur) = open_root(json);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_bytes(&cur, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], json.as_ref());
        qjson_free(doc);
    }
}
