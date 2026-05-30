use std::os::raw::c_int;
use qjson::ffi::*;

fn parse(s: &[u8]) -> *mut qjson_doc {
    let mut err = qjson_error::default();
    let d = unsafe { qjson_parse(s.as_ptr(), s.len(), &mut err) };
    assert!(!d.is_null());
    d
}

#[test]
fn get_i64_basic() {
    let d = parse(b"{\"a\":42}");
    let mut v: i64 = 0;
    let p = b"a";
    let rc = unsafe { qjson_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, 42);
    unsafe { qjson_free(d) };
}

#[test]
fn get_i64_negative() {
    let d = parse(b"{\"a\":-7}");
    let mut v: i64 = 0;
    let p = b"a";
    let rc = unsafe { qjson_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, -7);
    unsafe { qjson_free(d) };
}

#[test]
fn get_i64_overflow() {
    let d = parse(b"{\"a\":99999999999999999999}");
    let mut v: i64 = 0;
    let p = b"a";
    let rc = unsafe { qjson_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 4); // OUT_OF_RANGE
    unsafe { qjson_free(d) };
}

#[test]
fn get_f64_basic() {
    let d = parse(b"{\"a\":1.7}");
    let mut v: f64 = 0.0;
    let p = b"a";
    let rc = unsafe { qjson_get_f64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert!((v - 1.7).abs() < 1e-12);
    unsafe { qjson_free(d) };
}

#[test]
fn get_bool() {
    let d = parse(b"{\"a\":true,\"b\":false}");
    let mut v: c_int = -1;
    let p = b"a";
    let rc = unsafe { qjson_get_bool(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_ne!(v, 0);
    let p = b"b";
    let rc = unsafe { qjson_get_bool(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, 0);
    unsafe { qjson_free(d) };
}

#[test]
fn get_i64_max_and_min() {
    let json = format!("{{\"hi\":{},\"lo\":{}}}", i64::MAX, i64::MIN);
    let d = parse(json.as_bytes());
    let mut v: i64 = 0;
    let p = b"hi";
    let rc = unsafe { qjson_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, i64::MAX);
    let p = b"lo";
    let rc = unsafe { qjson_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, i64::MIN);
    unsafe { qjson_free(d) };
}

#[test]
fn get_i64_just_over_max_overflows() {
    // 9223372036854775808 = i64::MAX + 1
    let d = parse(b"{\"a\":9223372036854775808}");
    let mut v: i64 = 0;
    let p = b"a";
    let rc = unsafe { qjson_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 4); // OUT_OF_RANGE
    unsafe { qjson_free(d) };
}

#[test]
fn get_i64_returns_type_mismatch_for_non_numbers() {
    let d = parse(b"{\"b\":true,\"s\":\"1\",\"n\":null}");
    let mut v: i64 = 0;
    for p in [b"b".as_slice(), b"s".as_slice(), b"n".as_slice()] {
        let rc = unsafe { qjson_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
        assert_eq!(rc, 3); // TYPE_MISMATCH
    }
    unsafe { qjson_free(d) };
}

#[test]
fn get_u64_max() {
    let json = format!("{{\"a\":{}}}", u64::MAX);
    let d = parse(json.as_bytes());
    let mut v: u64 = 0;
    let p = b"a";
    let rc = unsafe { qjson_get_u64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, u64::MAX);
    unsafe { qjson_free(d) };
}

#[test]
fn get_u64_rejects_negative_and_overflow() {
    let d = parse(b"{\"neg\":-1,\"too_big\":18446744073709551616}");
    let mut v: u64 = 0;
    let p = b"neg";
    let rc = unsafe { qjson_get_u64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 4); // OUT_OF_RANGE
    let p = b"too_big";
    let rc = unsafe { qjson_get_u64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 4); // OUT_OF_RANGE
    unsafe { qjson_free(d) };
}

#[test]
fn get_u64_rejects_float_and_non_numbers() {
    let d = parse(b"{\"f\":1.5,\"b\":true,\"s\":\"1\",\"n\":null}");
    let mut v: u64 = 0;
    for p in [b"f".as_slice(), b"b".as_slice(), b"s".as_slice(), b"n".as_slice()] {
        let rc = unsafe { qjson_get_u64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
        assert_eq!(rc, 3); // TYPE_MISMATCH
    }
    unsafe { qjson_free(d) };
}

#[test]
fn cursor_get_u64_max() {
    let json = format!("{{\"root\":{{\"a\":{}}}}}", u64::MAX);
    let d = parse(json.as_bytes());
    let mut cur = qjson_cursor {
        doc: std::ptr::null(),
        idx_start: 0,
        idx_end: 0,
        _reserved0: 0,
        _reserved1: 0,
    };
    let p = b"root";
    let rc = unsafe { qjson_open(d, p.as_ptr() as *const i8, p.len(), &mut cur) };
    assert_eq!(rc, 0);
    let mut v: u64 = 0;
    let p = b"a";
    let rc = unsafe { qjson_cursor_get_u64(&cur, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, u64::MAX);
    unsafe { qjson_free(d) };
}

#[test]
fn get_f64_large_magnitude() {
    let d = parse(b"{\"a\":1.7e308}");
    let mut v: f64 = 0.0;
    let p = b"a";
    let rc = unsafe { qjson_get_f64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert!(v > 1.0e308 && v < f64::INFINITY);
    unsafe { qjson_free(d) };
}

#[test]
fn get_f64_negative_zero_and_exponent() {
    let d = parse(b"{\"a\":-0.0,\"b\":1e-300}");
    let mut v: f64 = 1.0;
    let p = b"a";
    let rc = unsafe { qjson_get_f64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, 0.0);
    let p = b"b";
    let rc = unsafe { qjson_get_f64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_eq!(rc, 0);
    assert!(v > 0.0 && v < 1e-200);
    unsafe { qjson_free(d) };
}

#[test]
fn get_i64_rejects_float_form() {
    let d = parse(b"{\"a\":1.5}");
    let mut v: i64 = 0;
    let p = b"a";
    let rc = unsafe { qjson_get_i64(d, p.as_ptr() as *const i8, p.len(), &mut v) };
    assert_ne!(rc, 0); // any error code is acceptable; not a valid i64
    unsafe { qjson_free(d) };
}
