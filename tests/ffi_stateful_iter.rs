use std::os::raw::c_int;
use std::ptr;

use qjson::ffi::{
    qjson_cursor, qjson_cursor_get_i64, qjson_cursor_typeof, qjson_doc, qjson_free,
    qjson_iter, qjson_iter_init, qjson_iter_next, qjson_open, qjson_parse,
};

unsafe fn open_root(json: &[u8]) -> (*mut qjson_doc, qjson_cursor) {
    let mut err: c_int = -1;
    let doc = qjson_parse(json.as_ptr(), json.len(), &mut err);
    assert!(!doc.is_null());
    assert_eq!(err, 0);

    let mut root: qjson_cursor = std::mem::zeroed();
    let rc = qjson_open(doc, ptr::null(), 0, &mut root);
    assert_eq!(rc, 0);
    (doc, root)
}

unsafe fn init_iter(cur: &qjson_cursor) -> qjson_iter {
    let mut it: qjson_iter = std::mem::zeroed();
    let rc = qjson_iter_init(cur, &mut it);
    assert_eq!(rc, 0);
    it
}

unsafe fn next(it: &mut qjson_iter) -> Option<(String, qjson_cursor)> {
    let mut key_ptr: *const u8 = ptr::null();
    let mut key_len: usize = 0;
    let mut value: qjson_cursor = std::mem::zeroed();
    let rc = qjson_iter_next(it, &mut key_ptr, &mut key_len, &mut value);
    if rc == 2 {
        return None;
    }
    assert_eq!(rc, 0);
    let key = std::slice::from_raw_parts(key_ptr, key_len);
    Some((String::from_utf8(key.to_vec()).unwrap(), value))
}

#[test]
fn stateful_iter_empty_object_exhausts_immediately() {
    unsafe {
        let (doc, root) = open_root(br#"{}"#);
        let mut it = init_iter(&root);
        assert!(next(&mut it).is_none());
        assert!(next(&mut it).is_none());
        qjson_free(doc);
    }
}

#[test]
fn stateful_iter_returns_keys_and_value_cursors_in_source_order() {
    unsafe {
        let (doc, root) = open_root(br#"{"a":1,"b":{"x":2},"c":[3]}"#);
        let mut it = init_iter(&root);

        let (k0, v0) = next(&mut it).unwrap();
        let mut n = 0i64;
        assert_eq!(k0, "a");
        assert_eq!(qjson_cursor_get_i64(&v0, ptr::null(), 0, &mut n), 0);
        assert_eq!(n, 1);

        let (k1, v1) = next(&mut it).unwrap();
        let mut t = -1;
        assert_eq!(k1, "b");
        assert_eq!(qjson_cursor_typeof(&v1, ptr::null(), 0, &mut t), 0);
        assert_eq!(t, 5); // QJSON_T_OBJ

        let (k2, v2) = next(&mut it).unwrap();
        assert_eq!(k2, "c");
        assert_eq!(qjson_cursor_typeof(&v2, ptr::null(), 0, &mut t), 0);
        assert_eq!(t, 4); // QJSON_T_ARR

        assert!(next(&mut it).is_none());
        qjson_free(doc);
    }
}

#[test]
fn multiple_iterators_keep_independent_positions() {
    unsafe {
        let (doc, root) = open_root(br#"{"outer":{"a":1,"b":2},"other":{"c":3}}"#);
        let mut root_it = init_iter(&root);

        let (outer_key, outer_cur) = next(&mut root_it).unwrap();
        assert_eq!(outer_key, "outer");
        let mut outer_it = init_iter(&outer_cur);

        let (other_key, _other_cur) = next(&mut root_it).unwrap();
        assert_eq!(other_key, "other");

        let (a_key, a_cur) = next(&mut outer_it).unwrap();
        let mut n = 0i64;
        assert_eq!(a_key, "a");
        assert_eq!(qjson_cursor_get_i64(&a_cur, ptr::null(), 0, &mut n), 0);
        assert_eq!(n, 1);

        let (b_key, b_cur) = next(&mut outer_it).unwrap();
        assert_eq!(b_key, "b");
        assert_eq!(qjson_cursor_get_i64(&b_cur, ptr::null(), 0, &mut n), 0);
        assert_eq!(n, 2);

        assert!(next(&mut outer_it).is_none());
        assert!(next(&mut root_it).is_none());
        qjson_free(doc);
    }
}

#[test]
fn iter_init_rejects_non_object_cursor() {
    unsafe {
        let (doc, root) = open_root(br#"[1,2,3]"#);
        let mut it: qjson_iter = std::mem::zeroed();
        let rc = qjson_iter_init(&root, &mut it);
        assert_eq!(rc, 3); // QJSON_TYPE_MISMATCH
        qjson_free(doc);
    }
}

