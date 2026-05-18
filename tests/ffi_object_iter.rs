use std::os::raw::c_int;
use std::ptr;

use qjson::ffi::{
    qjson_cursor, qjson_cursor_object_entry_at, qjson_doc, qjson_free, qjson_open, qjson_parse,
};

unsafe fn open_root(json: &[u8]) -> (*mut qjson_doc, qjson_cursor) {
    let mut err: c_int = -1;
    let doc = qjson_parse(json.as_ptr(), json.len(), &mut err);
    assert!(!doc.is_null());
    let mut cur: qjson_cursor = std::mem::zeroed();
    qjson_open(doc, ptr::null(), 0, &mut cur);
    (doc, cur)
}

unsafe fn entry_at(root: &qjson_cursor, i: usize) -> (String, qjson_cursor) {
    let mut kp: *const u8 = ptr::null();
    let mut kn: usize = 0;
    let mut vc: qjson_cursor = std::mem::zeroed();
    let rc = qjson_cursor_object_entry_at(root, i, &mut kp, &mut kn, &mut vc);
    assert_eq!(rc, 0, "entry_at({}) failed with rc={}", i, rc);
    let key = std::slice::from_raw_parts(kp, kn);
    (String::from_utf8(key.to_vec()).unwrap(), vc)
}

#[test]
fn three_keys_in_order() {
    let json = br#"{"a":1,"b":"x","c":[2,3]}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let (k0, _) = entry_at(&root, 0);
        let (k1, _) = entry_at(&root, 1);
        let (k2, _) = entry_at(&root, 2);
        assert_eq!(k0, "a");
        assert_eq!(k1, "b");
        assert_eq!(k2, "c");
        qjson_free(doc);
    }
}

#[test]
fn key_with_escape_decodes() {
    // The key `"a\nb"` (3 chars: a, newline, b) — verifies the FFI runs the
    // string-decode scratch path rather than handing back raw escaped bytes.
    let json = b"{\"a\\nb\":1}";
    unsafe {
        let (doc, root) = open_root(json);
        let (k0, _) = entry_at(&root, 0);
        assert_eq!(k0, "a\nb");
        qjson_free(doc);
    }
}

#[test]
fn out_of_range_returns_not_found() {
    let json = br#"{"a":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut kp: *const u8 = ptr::null();
        let mut kn: usize = 0;
        let mut vc: qjson_cursor = std::mem::zeroed();
        let rc = qjson_cursor_object_entry_at(&root, 5, &mut kp, &mut kn, &mut vc);
        assert_eq!(rc, 2); // QJSON_NOT_FOUND
        qjson_free(doc);
    }
}

#[test]
fn array_cursor_returns_type_mismatch() {
    let json = br#"[1,2,3]"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut kp: *const u8 = ptr::null();
        let mut kn: usize = 0;
        let mut vc: qjson_cursor = std::mem::zeroed();
        let rc = qjson_cursor_object_entry_at(&root, 0, &mut kp, &mut kn, &mut vc);
        assert_eq!(rc, 3); // QJSON_TYPE_MISMATCH
        qjson_free(doc);
    }
}
