//! Wide-object skip-cache test (spec §9.2): 5K keys, repeatedly access random
//! keys via the same cursor and confirm correctness.

use std::os::raw::c_int;
use quickdecode::ffi::*;

fn build_wide(n: usize) -> (String, Vec<String>) {
    let mut s = String::from("{");
    let mut keys = Vec::with_capacity(n);
    for i in 0..n {
        if i > 0 { s.push(','); }
        let k = format!("k{:05}", i);
        s.push('"'); s.push_str(&k); s.push_str("\":");
        s.push_str(&format!("{}", i * 2));
        keys.push(k);
    }
    s.push('}');
    (s, keys)
}

#[test]
fn wide_object_5k_keys_all_resolvable() {
    let n = 5000;
    let (json, keys) = build_wide(n);
    let mut err: c_int = -1;
    let d = unsafe { qjd_parse(json.as_ptr(), json.len(), &mut err) };
    assert!(!d.is_null());

    // Hit a sparse sample, in non-sequential order, twice (second pass exercises
    // the cache-hit path).
    let samples: Vec<usize> = (0..n).step_by(173).collect();
    for &i in &samples {
        let mut v: i64 = -1;
        let k = keys[i].as_bytes();
        let rc = unsafe { qjd_get_i64(d, k.as_ptr() as *const i8, k.len(), &mut v) };
        assert_eq!(rc, 0, "miss on first pass for key {}", keys[i]);
        assert_eq!(v as usize, i * 2);
    }
    for &i in samples.iter().rev() {
        let mut v: i64 = -1;
        let k = keys[i].as_bytes();
        let rc = unsafe { qjd_get_i64(d, k.as_ptr() as *const i8, k.len(), &mut v) };
        assert_eq!(rc, 0, "miss on cache-hit pass for key {}", keys[i]);
        assert_eq!(v as usize, i * 2);
    }

    // Unknown key still returns NOT_FOUND after the cache is populated.
    let bogus = b"definitely_not_a_key";
    let mut v: i64 = 0;
    let rc = unsafe { qjd_get_i64(d, bogus.as_ptr() as *const i8, bogus.len(), &mut v) };
    assert_eq!(rc, 2); // QJD_NOT_FOUND

    unsafe { qjd_free(d) };
}
