use std::os::raw::c_int;
use quickdecode::ffi::*;

fn parse(s: &[u8]) -> *mut qjd_doc {
    let mut err: c_int = -1;
    let d = unsafe { qjd_parse(s.as_ptr(), s.len(), &mut err) };
    assert!(!d.is_null());
    d
}

#[test]
fn get_str_simple() {
    let d = parse(b"{\"a\":\"hello\"}");
    let mut p: *const u8 = std::ptr::null();
    let mut n: usize = 0;
    let path = b"a";
    let rc = unsafe { qjd_get_str(d, path.as_ptr() as *const i8, path.len(), &mut p, &mut n) };
    assert_eq!(rc, 0);
    let s = unsafe { std::slice::from_raw_parts(p, n) };
    assert_eq!(s, b"hello");
    unsafe { qjd_free(d) };
}

#[test]
fn get_str_with_escape() {
    let d = parse(b"{\"a\":\"he\\nlo\"}");
    let mut p: *const u8 = std::ptr::null();
    let mut n: usize = 0;
    let path = b"a";
    let rc = unsafe { qjd_get_str(d, path.as_ptr() as *const i8, path.len(), &mut p, &mut n) };
    assert_eq!(rc, 0);
    let s = unsafe { std::slice::from_raw_parts(p, n) };
    assert_eq!(s, b"he\nlo");
    unsafe { qjd_free(d) };
}

#[test]
fn get_str_type_mismatch() {
    let d = parse(b"{\"a\":42}");
    let mut p: *const u8 = std::ptr::null();
    let mut n: usize = 0;
    let path = b"a";
    let rc = unsafe { qjd_get_str(d, path.as_ptr() as *const i8, path.len(), &mut p, &mut n) };
    assert_eq!(rc, 3); // TYPE_MISMATCH
    unsafe { qjd_free(d) };
}
