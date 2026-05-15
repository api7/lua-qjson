use quickdecode::ffi::qjd_test_panic;

#[test]
fn panic_does_not_unwind_through_ffi() {
    let rc = unsafe { qjd_test_panic() };
    assert_eq!(rc, 8); // QJD_OOM
}
