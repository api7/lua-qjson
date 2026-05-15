use std::os::raw::c_int;
use quickdecode::ffi::*;

fn parse(s: &[u8]) -> *mut qjd_doc {
    let mut err: c_int = -1;
    let d = unsafe { qjd_parse(s.as_ptr(), s.len(), &mut err) };
    assert!(!d.is_null());
    d
}

#[test]
fn get_i64_basic() {
    let d = parse(b"{\"a\":42}");
    let mut v: i64 = 0;
    let p = b"a";
    let rc = unsafe { qjd_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, 42);
    unsafe { qjd_free(d) };
}

#[test]
fn get_i64_negative() {
    let d = parse(b"{\"a\":-7}");
    let mut v: i64 = 0;
    let p = b"a";
    unsafe { qjd_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(v, -7);
    unsafe { qjd_free(d) };
}

#[test]
fn get_i64_overflow() {
    let d = parse(b"{\"a\":99999999999999999999}");
    let mut v: i64 = 0;
    let p = b"a";
    let rc = unsafe { qjd_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 4); // OUT_OF_RANGE
    unsafe { qjd_free(d) };
}

#[test]
fn get_f64_basic() {
    let d = parse(b"{\"a\":3.14}");
    let mut v: f64 = 0.0;
    let p = b"a";
    unsafe { qjd_get_f64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert!((v - 3.14).abs() < 1e-12);
    unsafe { qjd_free(d) };
}

#[test]
fn get_bool() {
    let d = parse(b"{\"a\":true,\"b\":false}");
    let mut v: c_int = -1;
    let p = b"a";
    unsafe { qjd_get_bool(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_ne!(v, 0);
    let p = b"b";
    unsafe { qjd_get_bool(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(v, 0);
    unsafe { qjd_free(d) };
}
