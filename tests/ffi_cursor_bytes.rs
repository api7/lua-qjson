use std::os::raw::c_int;
use std::ptr;

use quickdecode::ffi::{
    qjd_cursor, qjd_cursor_bytes, qjd_cursor_field, qjd_doc, qjd_free, qjd_open, qjd_parse,
};

unsafe fn open_root(json: &[u8]) -> (*mut qjd_doc, qjd_cursor) {
    let mut err: c_int = -1;
    let doc = qjd_parse(json.as_ptr(), json.len(), &mut err);
    assert!(!doc.is_null());
    let mut cur: qjd_cursor = std::mem::zeroed();
    let rc = qjd_open(doc, ptr::null(), 0, &mut cur);
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
        let rc = qjd_cursor_bytes(&cur, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], json.as_ref());
        qjd_free(doc);
    }
}

#[test]
fn bytes_of_string_value_is_quoted_span() {
    let json = br#"{"k":"hello"}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjd_cursor = std::mem::zeroed();
        let rc = qjd_cursor_field(&root, b"k".as_ptr() as *const i8, 1, &mut child);
        assert_eq!(rc, 0);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjd_cursor_bytes(&child, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], br#""hello""#);
        qjd_free(doc);
    }
}

#[test]
fn bytes_of_number_value_strips_separators() {
    let json = br#"{"k": 42 ,"x":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjd_cursor = std::mem::zeroed();
        let rc = qjd_cursor_field(&root, b"k".as_ptr() as *const i8, 1, &mut child);
        assert_eq!(rc, 0);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjd_cursor_bytes(&child, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], b"42");
        qjd_free(doc);
    }
}

#[test]
fn bytes_with_null_out_pointer_returns_invalid_arg() {
    let json = br#"{"a":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let rc = qjd_cursor_bytes(&root, ptr::null_mut(), ptr::null_mut());
        assert_eq!(rc, 7); // QJD_INVALID_ARG
        qjd_free(doc);
    }
}
