use std::os::raw::c_int;
use qjson::ffi::*;

fn parse(s: &[u8]) -> *mut qjson_doc {
    let mut err = qjson_error::default();
    let d = unsafe { qjson_parse(s.as_ptr(), s.len(), &mut err) };
    assert!(!d.is_null());
    d
}

#[test]
fn typeof_string() {
    let d = parse(b"{\"a\":\"hi\"}");
    let mut t: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjson_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 3); // QJSON_T_STR
    unsafe { qjson_free(d) };
}

#[test]
fn typeof_number() {
    let d = parse(b"{\"a\":42}");
    let mut t: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjson_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 2); // QJSON_T_NUM
    unsafe { qjson_free(d) };
}

#[test]
fn typeof_bool() {
    let d = parse(b"{\"a\":true}");
    let mut t: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjson_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 1);
    unsafe { qjson_free(d) };
}

#[test]
fn typeof_null() {
    let d = parse(b"{\"a\":null}");
    let mut t: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjson_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 0);
    unsafe { qjson_free(d) };
}

#[test]
fn is_null_true() {
    let d = parse(b"{\"a\":null}");
    let mut b: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjson_is_null(d, p.as_ptr() as *const i8, p.len(), &mut b) };
    assert_eq!(rc, 0);
    assert_ne!(b, 0);
    unsafe { qjson_free(d) };
}

#[test]
fn len_object() {
    let d = parse(b"{\"a\":1,\"b\":2,\"c\":3}");
    let mut n: usize = 0;
    let p = b"";
    let rc = unsafe { qjson_len(d, p.as_ptr() as *const i8, p.len(), &mut n) };
    assert_eq!(rc, 0);
    assert_eq!(n, 3);
    unsafe { qjson_free(d) };
}

#[test]
fn len_array() {
    let d = parse(b"[10,20,30,40]");
    let mut n: usize = 0;
    let p = b"";
    let rc = unsafe { qjson_len(d, p.as_ptr() as *const i8, p.len(), &mut n) };
    assert_eq!(rc, 0);
    assert_eq!(n, 4);
    unsafe { qjson_free(d) };
}

#[test]
fn typeof_not_found() {
    let d = parse(b"{\"a\":1}");
    let mut t: c_int = -1;
    let p = b"b";
    let rc = unsafe { qjson_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 2); // NOT_FOUND
    unsafe { qjson_free(d) };
}

#[test]
fn len_empty_object() {
    let d = parse(b"{}");
    let mut n: usize = 0;
    let p = b"";
    let rc = unsafe { qjson_len(d, p.as_ptr() as *const i8, p.len(), &mut n) };
    assert_eq!(rc, 0);
    assert_eq!(n, 0);
    unsafe { qjson_free(d) };
}

#[test]
fn len_empty_array() {
    let d = parse(b"[]");
    let mut n: usize = 0;
    let p = b"";
    let rc = unsafe { qjson_len(d, p.as_ptr() as *const i8, p.len(), &mut n) };
    assert_eq!(rc, 0);
    assert_eq!(n, 0);
    unsafe { qjson_free(d) };
}

#[test]
fn len_single_scalar_array() {
    let d = parse(b"[5]");
    let mut n: usize = 0;
    let p = b"";
    let rc = unsafe { qjson_len(d, p.as_ptr() as *const i8, p.len(), &mut n) };
    assert_eq!(rc, 0);
    assert_eq!(n, 1);
    unsafe { qjson_free(d) };
}

#[test]
fn len_single_scalar_object() {
    let d = parse(b"{\"a\":1}");
    let mut n: usize = 0;
    let p = b"";
    let rc = unsafe { qjson_len(d, p.as_ptr() as *const i8, p.len(), &mut n) };
    assert_eq!(rc, 0);
    assert_eq!(n, 1);
    unsafe { qjson_free(d) };
}

#[test]
fn root_scalar_typeof_and_getters_work_with_empty_path() {
    let empty = b"";

    let d = parse(b"42");
    let mut t: c_int = -1;
    let rc = unsafe { qjson_typeof(d, empty.as_ptr() as *const i8, 0, &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 2);
    let mut v: f64 = 0.0;
    let rc = unsafe { qjson_get_f64(d, empty.as_ptr() as *const i8, 0, &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, 42.0);
    unsafe { qjson_free(d) };

    let d = parse(b"false");
    let mut b: c_int = -1;
    let rc = unsafe { qjson_get_bool(d, empty.as_ptr() as *const i8, 0, &mut b) };
    assert_eq!(rc, 0);
    assert_eq!(b, 0);
    unsafe { qjson_free(d) };

    let d = parse(b"null");
    let mut is_null: c_int = 0;
    let rc = unsafe { qjson_is_null(d, empty.as_ptr() as *const i8, 0, &mut is_null) };
    assert_eq!(rc, 0);
    assert_eq!(is_null, 1);
    unsafe { qjson_free(d) };
}

#[test]
fn root_scalar_len_returns_type_mismatch() {
    let d = parse(b"42");
    let empty = b"";
    let mut len = 0usize;
    let rc = unsafe { qjson_len(d, empty.as_ptr() as *const i8, 0, &mut len) };
    assert_eq!(rc, 3);
    unsafe { qjson_free(d) };
}
