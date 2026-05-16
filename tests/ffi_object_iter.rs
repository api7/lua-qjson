use std::os::raw::c_int;
use std::ptr;

use quickdecode::ffi::{
    qjd_cursor, qjd_cursor_object_entry_at, qjd_doc, qjd_free, qjd_open, qjd_parse,
};

unsafe fn open_root(json: &[u8]) -> (*mut qjd_doc, qjd_cursor) {
    let mut err: c_int = -1;
    let doc = qjd_parse(json.as_ptr(), json.len(), &mut err);
    assert!(!doc.is_null());
    let mut cur: qjd_cursor = std::mem::zeroed();
    qjd_open(doc, ptr::null(), 0, &mut cur);
    (doc, cur)
}

unsafe fn entry_at(root: &qjd_cursor, i: usize) -> (String, qjd_cursor) {
    let mut kp: *const u8 = ptr::null();
    let mut kn: usize = 0;
    let mut vc: qjd_cursor = std::mem::zeroed();
    let rc = qjd_cursor_object_entry_at(root, i, &mut kp, &mut kn, &mut vc);
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
        qjd_free(doc);
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
        qjd_free(doc);
    }
}

#[test]
fn out_of_range_returns_not_found() {
    let json = br#"{"a":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut kp: *const u8 = ptr::null();
        let mut kn: usize = 0;
        let mut vc: qjd_cursor = std::mem::zeroed();
        let rc = qjd_cursor_object_entry_at(&root, 5, &mut kp, &mut kn, &mut vc);
        assert_eq!(rc, 2); // QJD_NOT_FOUND
        qjd_free(doc);
    }
}

#[test]
fn array_cursor_returns_type_mismatch() {
    let json = br#"[1,2,3]"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut kp: *const u8 = ptr::null();
        let mut kn: usize = 0;
        let mut vc: qjd_cursor = std::mem::zeroed();
        let rc = qjd_cursor_object_entry_at(&root, 0, &mut kp, &mut kn, &mut vc);
        assert_eq!(rc, 3); // QJD_TYPE_MISMATCH
        qjd_free(doc);
    }
}
