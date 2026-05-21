use std::os::raw::c_int;
use std::ptr;

use qjson::error::qjson_err;
use qjson::ffi::{
    qjson_cursor, qjson_cursor_bytes, qjson_cursor_field, qjson_cursor_field_bytes, qjson_doc,
    qjson_free, qjson_open, qjson_parse,
};

unsafe fn open_root(json: &[u8]) -> (*mut qjson_doc, qjson_cursor) {
    let mut err: c_int = -1;
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

// ---------------------------------------------------------------------------
// qjson_cursor_field_bytes: the splice path's load-bearing FFI symbol.

unsafe fn field_bytes(root: &qjson_cursor, key: &[u8]) -> (c_int, usize, usize) {
    let mut bs: usize = 0;
    let mut be: usize = 0;
    let rc = qjson_cursor_field_bytes(root, key.as_ptr() as *const i8, key.len(), &mut bs, &mut be);
    (rc, bs, be)
}

#[test]
fn field_bytes_string_value_returns_quoted_span() {
    let json = br#"{"k":"hello"}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let (rc, bs, be) = field_bytes(&root, b"k");
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], br#""hello""#);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_number_value_strips_separators() {
    let json = br#"{"k": 42 ,"x":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let (rc, bs, be) = field_bytes(&root, b"k");
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], b"42");
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_object_value_covers_braces() {
    let json = br#"{"k":{"a":1,"b":[2,3]},"x":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let (rc, bs, be) = field_bytes(&root, b"k");
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], br#"{"a":1,"b":[2,3]}"#);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_array_value_covers_brackets() {
    let json = br#"{"k":[1,"two",true],"x":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let (rc, bs, be) = field_bytes(&root, b"k");
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], br#"[1,"two",true]"#);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_missing_key_returns_not_found() {
    let json = br#"{"a":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let (rc, _, _) = field_bytes(&root, b"missing");
        assert_eq!(rc, qjson_err::QJSON_NOT_FOUND as c_int);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_non_object_cursor_returns_type_mismatch() {
    // root is an array — qjson_cursor_field_bytes should refuse to index by key.
    let json = br#"[1,2,3]"#;
    unsafe {
        let (doc, root) = open_root(json);
        let (rc, _, _) = field_bytes(&root, b"k");
        assert_eq!(rc, qjson_err::QJSON_TYPE_MISMATCH as c_int);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_null_out_pointer_returns_invalid_arg() {
    let json = br#"{"a":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let key = b"a";
        // Both out pointers null
        let rc = qjson_cursor_field_bytes(
            &root, key.as_ptr() as *const i8, key.len(), ptr::null_mut(), ptr::null_mut(),
        );
        assert_eq!(rc, qjson_err::QJSON_INVALID_ARG as c_int);
        // Only value_end null
        let mut bs: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &root, key.as_ptr() as *const i8, key.len(), &mut bs, ptr::null_mut(),
        );
        assert_eq!(rc, qjson_err::QJSON_INVALID_ARG as c_int);
        qjson_free(doc);
    }
}

#[test]
fn field_bytes_round_trip_splice() {
    // The use case the FFI exists for: extract a field's span, replace it
    // with a different JSON literal, re-parse and check the resulting field.
    let json = br#"{"a":1,"b":"old","c":3}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let (rc, bs, be) = field_bytes(&root, b"b");
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], br#""old""#);
        qjson_free(doc);

        // Splice + reparse
        let mut spliced = Vec::new();
        spliced.extend_from_slice(&json[..bs]);
        spliced.extend_from_slice(br#"["new",42]"#);
        spliced.extend_from_slice(&json[be..]);

        let (doc2, root2) = open_root(&spliced);
        let (rc2, bs2, be2) = field_bytes(&root2, b"b");
        assert_eq!(rc2, 0);
        assert_eq!(&spliced[bs2..be2], br#"["new",42]"#);
        // Unchanged neighbours
        let (_, bsa, bea) = field_bytes(&root2, b"a");
        assert_eq!(&spliced[bsa..bea], b"1");
        let (_, bsc, bec) = field_bytes(&root2, b"c");
        assert_eq!(&spliced[bsc..bec], b"3");
        qjson_free(doc2);
    }
}
