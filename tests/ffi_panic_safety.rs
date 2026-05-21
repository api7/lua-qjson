#[cfg(feature = "test-panic")]
#[test]
fn panic_does_not_unwind_through_ffi() {
    use qjson::ffi::qjson_test_panic;
    let rc = unsafe { qjson_test_panic() };
    assert_eq!(rc, 8); // QJSON_OOM
}

#[cfg(feature = "test-panic")]
#[test]
fn cursor_field_bytes_panic_returns_oom() {
    // Forge a cursor whose idx_start is well past the end of the indices
    // array. Any internal access via `d.indices[idx_start]` panics on
    // out-of-bounds; the `ffi_catch!` wrapper around qjson_cursor_field_bytes
    // must convert that into QJSON_OOM instead of unwinding across the FFI
    // boundary.
    use std::os::raw::c_int;
    use std::ptr;
    use qjson::ffi::{
        qjson_cursor, qjson_cursor_field_bytes, qjson_free, qjson_open, qjson_parse,
    };

    let json: &[u8] = br#"{"a":1}"#;
    unsafe {
        let mut err: c_int = -1;
        let doc = qjson_parse(json.as_ptr(), json.len(), &mut err);
        assert!(!doc.is_null());
        let mut root: qjson_cursor = std::mem::zeroed();
        let rc = qjson_open(doc, ptr::null(), 0, &mut root);
        assert_eq!(rc, 0);

        // Corrupt the cursor to extend past the end of the indices array.
        // `idx_start` remains valid so the container check passes and the
        // walker enters `walk_children`, which then runs off the end of
        // `doc.indices` and panics. The `ffi_catch!` wrapper around
        // qjson_cursor_field_bytes must convert that panic into QJSON_OOM.
        let mut bad = root;
        bad.idx_end = u32::MAX - 1;

        let mut child: qjson_cursor = std::mem::zeroed();
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjson_cursor_field_bytes(
            &bad,
            b"a".as_ptr() as *const i8,
            1,
            &mut child,
            &mut bs,
            &mut be,
        );
        assert_eq!(rc, 8); // QJSON_OOM

        qjson_free(doc);
    }
}

#[cfg(not(feature = "test-panic"))]
#[test]
fn skip() {
    // Compile-only test; the panic-safety test requires `--features test-panic`.
}
