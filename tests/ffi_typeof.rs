use std::os::raw::c_int;
use quickdecode::ffi::*;

fn parse(s: &[u8]) -> *mut qjd_doc {
    let mut err: c_int = -1;
    let d = unsafe { qjd_parse(s.as_ptr(), s.len(), &mut err) };
    assert!(!d.is_null());
    d
}

#[test]
fn typeof_string() {
    let d = parse(b"{\"a\":\"hi\"}");
    let mut t: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjd_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 3); // QJD_T_STR
    unsafe { qjd_free(d) };
}

#[test]
fn typeof_number() {
    let d = parse(b"{\"a\":42}");
    let mut t: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjd_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 2); // QJD_T_NUM
    unsafe { qjd_free(d) };
}

#[test]
fn typeof_bool() {
    let d = parse(b"{\"a\":true}");
    let mut t: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjd_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 1);
    unsafe { qjd_free(d) };
}

#[test]
fn typeof_null() {
    let d = parse(b"{\"a\":null}");
    let mut t: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjd_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 0);
    unsafe { qjd_free(d) };
}

#[test]
fn is_null_true() {
    let d = parse(b"{\"a\":null}");
    let mut b: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjd_is_null(d, p.as_ptr() as *const i8, p.len(), &mut b) };
    assert_eq!(rc, 0);
    assert_ne!(b, 0);
    unsafe { qjd_free(d) };
}

#[test]
fn len_object() {
    let d = parse(b"{\"a\":1,\"b\":2,\"c\":3}");
    let mut n: usize = 0;
    let p = b"";
    let rc = unsafe { qjd_len(d, p.as_ptr() as *const i8, p.len(), &mut n) };
    assert_eq!(rc, 0);
    assert_eq!(n, 3);
    unsafe { qjd_free(d) };
}

#[test]
fn len_array() {
    let d = parse(b"[10,20,30,40]");
    let mut n: usize = 0;
    let p = b"";
    let rc = unsafe { qjd_len(d, p.as_ptr() as *const i8, p.len(), &mut n) };
    assert_eq!(rc, 0);
    assert_eq!(n, 4);
    unsafe { qjd_free(d) };
}

#[test]
fn typeof_not_found() {
    let d = parse(b"{\"a\":1}");
    let mut t: c_int = -1;
    let p = b"b";
    let rc = unsafe { qjd_typeof(d, p.as_ptr() as *const i8, p.len(), &mut t) };
    assert_eq!(rc, 2); // NOT_FOUND
    unsafe { qjd_free(d) };
}
