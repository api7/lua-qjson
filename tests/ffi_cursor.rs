use std::os::raw::c_int;
use qjson::ffi::*;

fn parse(s: &[u8]) -> *mut qjson_doc {
    let mut err: c_int = -1;
    let d = unsafe { qjson_parse(s.as_ptr(), s.len(), &mut err) };
    assert!(!d.is_null());
    d
}

#[test]
fn open_object_then_get_field() {
    let d = parse(b"{\"body\":{\"model\":\"gpt\",\"temperature\":0.5}}");
    let mut c = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let p = b"body";
    let rc = unsafe { qjson_open(d, p.as_ptr() as *const i8, p.len(), c.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let c = unsafe { c.assume_init() };

    let mut pp: *const u8 = std::ptr::null();
    let mut nn: usize = 0;
    let k = b"model";
    let rc = unsafe { qjson_cursor_get_str(&c, k.as_ptr() as *const i8, k.len(), &mut pp, &mut nn) };
    assert_eq!(rc, 0);
    let s = unsafe { std::slice::from_raw_parts(pp, nn) };
    assert_eq!(s, b"gpt");

    let mut f: f64 = 0.0;
    let k = b"temperature";
    let rc = unsafe { qjson_cursor_get_f64(&c, k.as_ptr() as *const i8, k.len(), &mut f) };
    assert_eq!(rc, 0);
    assert!((f - 0.5).abs() < 1e-12);

    unsafe { qjson_free(d) };
}

#[test]
fn cursor_index_array() {
    let d = parse(b"[\"a\",\"b\",\"c\"]");
    let mut c = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let p = b"";
    let rc = unsafe { qjson_open(d, p.as_ptr() as *const i8, 0, c.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let c = unsafe { c.assume_init() };

    let mut sub = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let rc = unsafe { qjson_cursor_index(&c, 1, sub.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let sub = unsafe { sub.assume_init() };

    let mut pp: *const u8 = std::ptr::null();
    let mut nn: usize = 0;
    let empty = b"";
    let rc = unsafe { qjson_cursor_get_str(&sub, empty.as_ptr() as *const i8, 0, &mut pp, &mut nn) };
    assert_eq!(rc, 0);
    assert_eq!(unsafe { std::slice::from_raw_parts(pp, nn) }, b"b");

    unsafe { qjson_free(d) };
}

#[test]
fn cursor_field_with_dotted_key() {
    let d = parse(b"{\"a.b\":42}");
    let mut c = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let p = b"";
    let rc = unsafe { qjson_open(d, p.as_ptr() as *const i8, 0, c.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let c = unsafe { c.assume_init() };

    let mut sub = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let key = b"a.b";
    let rc = unsafe { qjson_cursor_field(&c, key.as_ptr() as *const i8, key.len(), sub.as_mut_ptr()) };
    assert_eq!(rc, 0);

    let sub = unsafe { sub.assume_init() };
    let mut v: i64 = 0;
    let empty = b"";
    let rc = unsafe { qjson_cursor_get_i64(&sub, empty.as_ptr() as *const i8, 0, &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, 42);

    unsafe { qjson_free(d) };
}

// Regression: walk_children must visit the trailing scalar (last element has no
// structural marker of its own — `indices[end]` is the parent closer).
// Before the fix, `while i < end` stopped one step early and index 2 returned
// QJSON_NOT_FOUND for a 3-element all-scalar array.
#[test]
fn walk_children_trailing_scalar_integer() {
    let d = parse(b"[10,20,30]");
    let mut c = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let empty = b"";
    let rc = unsafe { qjson_open(d, empty.as_ptr() as *const i8, 0, c.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let c = unsafe { c.assume_init() };

    // Index 2 is the trailing element `30`.
    let mut sub = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let rc = unsafe { qjson_cursor_index(&c, 2, sub.as_mut_ptr()) };
    assert_eq!(rc, 0, "qjson_cursor_index([2]) must succeed");
    let sub = unsafe { sub.assume_init() };

    let mut v: i64 = 0;
    let rc = unsafe { qjson_cursor_get_i64(&sub, empty.as_ptr() as *const i8, 0, &mut v) };
    assert_eq!(rc, 0, "qjson_cursor_get_i64 on trailing element must succeed");
    assert_eq!(v, 30);

    unsafe { qjson_free(d) };
}

// Regression: trailing scalar with non-numeric type — ensures walk_children
// visits it and type information is correct (not silently skipped).
#[test]
fn walk_children_trailing_scalar_bool() {
    let d = parse(b"[1,\"x\",true]");
    let mut c = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let empty = b"";
    let rc = unsafe { qjson_open(d, empty.as_ptr() as *const i8, 0, c.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let c = unsafe { c.assume_init() };

    // Index 2 is the trailing element `true`.
    let mut sub = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let rc = unsafe { qjson_cursor_index(&c, 2, sub.as_mut_ptr()) };
    assert_eq!(rc, 0, "qjson_cursor_index([2]) must succeed");
    let sub = unsafe { sub.assume_init() };

    let mut b: c_int = -1;
    let rc = unsafe { qjson_cursor_get_bool(&sub, empty.as_ptr() as *const i8, 0, &mut b) };
    assert_eq!(rc, 0, "qjson_cursor_get_bool on trailing `true` must succeed");
    assert_eq!(b, 1);

    unsafe { qjson_free(d) };
}

#[test]
fn open_root_number_cursor_gets_f64() {
    let d = parse(b"42");
    let mut c = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let empty = b"";
    let rc = unsafe { qjson_open(d, empty.as_ptr() as *const i8, 0, c.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let c = unsafe { c.assume_init() };

    let mut t: c_int = -1;
    let rc = unsafe { qjson_cursor_typeof(&c, empty.as_ptr() as *const i8, 0, &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 2);

    let mut v: f64 = 0.0;
    let rc = unsafe { qjson_cursor_get_f64(&c, empty.as_ptr() as *const i8, 0, &mut v) };
    assert_eq!(rc, 0);
    assert_eq!(v, 42.0);

    unsafe { qjson_free(d) };
}

#[test]
fn open_root_bool_and_null_cursors() {
    let empty = b"";

    let d = parse(b"true");
    let mut c = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let rc = unsafe { qjson_open(d, empty.as_ptr() as *const i8, 0, c.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let c = unsafe { c.assume_init() };

    let mut b: c_int = -1;
    let rc = unsafe { qjson_cursor_get_bool(&c, empty.as_ptr() as *const i8, 0, &mut b) };
    assert_eq!(rc, 0);
    assert_eq!(b, 1);
    unsafe { qjson_free(d) };

    let d = parse(b"null");
    let mut c = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let rc = unsafe { qjson_open(d, empty.as_ptr() as *const i8, 0, c.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let c = unsafe { c.assume_init() };

    let mut t: c_int = -1;
    let rc = unsafe { qjson_cursor_typeof(&c, empty.as_ptr() as *const i8, 0, &mut t) };
    assert_eq!(rc, 0);
    assert_eq!(t, 0);

    unsafe { qjson_free(d) };
}

#[test]
fn root_scalar_cursor_container_ops_return_type_mismatch() {
    let d = parse(b"42");
    let mut c = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let empty = b"";
    let rc = unsafe { qjson_open(d, empty.as_ptr() as *const i8, 0, c.as_mut_ptr()) };
    assert_eq!(rc, 0);
    let c = unsafe { c.assume_init() };

    let mut len = 0usize;
    let rc = unsafe { qjson_cursor_len(&c, empty.as_ptr() as *const i8, 0, &mut len) };
    assert_eq!(rc, 3);

    let mut key_ptr: *const u8 = std::ptr::null();
    let mut key_len = 0usize;
    let mut child: qjson_cursor = unsafe { std::mem::zeroed() };
    let rc = unsafe { qjson_cursor_object_entry_at(&c, 0, &mut key_ptr, &mut key_len, &mut child) };
    assert_eq!(rc, 3);

    unsafe { qjson_free(d) };
}
