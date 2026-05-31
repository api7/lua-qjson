use qjson::error::qjson_err;
use qjson::ffi::{
    qjson_cursor, qjson_doc, qjson_doc_last_error_offset, qjson_error, qjson_format_error,
    qjson_free, qjson_get_i64, qjson_get_str, qjson_open, qjson_parse, qjson_parse_ex,
    qjson_cursor_get_i64, qjson_cursor_get_str,
};
use qjson::options::Options;
use std::ffi::CStr;
use std::os::raw::c_char;

fn parse_error(buf: &[u8]) -> qjson_error {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse(buf.as_ptr(), buf.len(), &mut err) };
    assert!(doc.is_null(), "parse unexpectedly succeeded for {:?}", buf);
    err
}

fn parse_ex_error(buf: &[u8], opts: &Options) -> qjson_error {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(buf.as_ptr(), buf.len(), opts, &mut err) };
    assert!(doc.is_null(), "parse_ex unexpectedly succeeded for {:?}", buf);
    err
}

fn parse_ok(buf: &[u8]) -> qjson_error {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse(buf.as_ptr(), buf.len(), &mut err) };
    assert!(!doc.is_null(), "parse unexpectedly failed with {:?}", err);
    unsafe { qjson_free(doc); }
    err
}

fn parse_doc_ok(buf: &[u8]) -> *mut qjson_doc {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse(buf.as_ptr(), buf.len(), &mut err) };
    assert!(!doc.is_null(), "parse unexpectedly failed with {:?}", err);
    doc
}

fn format_error_message(code: qjson_err, offset: usize, extra: usize, buf: &[u8]) -> String {
    let mut out = vec![0u8; 512];
    let written = unsafe {
        qjson_format_error(
            code as i32,
            offset,
            extra,
            buf.as_ptr() as *const c_char,
            buf.len(),
            out.as_mut_ptr() as *mut c_char,
            out.len(),
        )
    };
    assert!(written + 1 < out.len(), "output buffer was too small");
    let msg = CStr::from_bytes_until_nul(&out).expect("missing NUL terminator");
    let msg = msg.to_str().expect("non-utf8 message").to_owned();
    assert_eq!(written, msg.len());
    msg
}

#[test]
fn success_writes_ok_with_no_offset() {
    let err = parse_ok(br#"{"a":1}"#);
    assert_eq!(err.code, qjson_err::QJSON_OK as i32);
    assert_eq!(err.offset, usize::MAX);
}

#[test]
fn empty_input_has_no_position() {
    let err = parse_error(b"");
    assert_eq!(err.code, qjson_err::QJSON_PARSE_ERROR as i32);
    assert_eq!(err.offset, usize::MAX);
}

#[test]
fn truncated_container_reports_end_offset() {
    let err = parse_error(b"{");
    assert_eq!(err.code, qjson_err::QJSON_PARSE_ERROR as i32);
    assert_eq!(err.offset, 1);
}

#[test]
fn unclosed_eager_container_reports_first_bad_structural_before_eof() {
    let err = parse_error(br#"{"a":1,,"#);
    assert_eq!(err.code, qjson_err::QJSON_PARSE_ERROR as i32);
    assert_eq!(err.offset, 7);

    let msg = format_error_message(
        qjson_err::QJSON_PARSE_ERROR,
        err.offset,
        0,
        br#"{"a":1,,"#,
    );
    assert_eq!(msg, "parse error at byte 7: unexpected ',', expected value");
}

#[test]
fn mismatched_bracket_reports_rejected_byte() {
    let err = parse_error(b"[}");
    assert_eq!(err.code, qjson_err::QJSON_PARSE_ERROR as i32);
    assert_eq!(err.offset, 1);
}

#[test]
fn invalid_number_reports_token_start() {
    let err = parse_error(b"[01]");
    assert_eq!(err.code, qjson_err::QJSON_INVALID_NUMBER as i32);
    assert_eq!(err.offset, 1);
}

#[test]
fn invalid_utf8_string_reports_string_token_start() {
    let err = parse_error(b"{\"a\":\"\xff\"}");
    assert_eq!(err.code, qjson_err::QJSON_INVALID_UTF8 as i32);
    assert_eq!(err.offset, 5);
}

#[test]
fn trailing_content_reports_first_trailing_byte() {
    let err = parse_error(b"{}garbage");
    assert_eq!(err.code, qjson_err::QJSON_TRAILING_CONTENT as i32);
    assert_eq!(err.offset, 2);
}

#[test]
fn eager_depth_reports_opening_byte_that_exceeds_limit() {
    let opts = Options { mode: 0, max_depth: 2 };
    let err = parse_ex_error(b"[[[0]]]", &opts);
    assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as i32);
    assert_eq!(err.offset, 2);
}

#[test]
fn eager_unclosed_depth_reports_opening_byte_that_exceeds_limit() {
    let mut buf = vec![b'['; 1025];
    let err = parse_error(&buf);
    assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as i32);
    assert_eq!(err.offset, 1024);

    buf.truncate(3);
    let opts = Options { mode: 1, max_depth: 2 };
    let err = parse_ex_error(&buf, &opts);
    assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as i32);
    assert_eq!(err.offset, 2);
}

#[test]
fn lazy_depth_reports_opening_byte_that_exceeds_limit() {
    let opts = Options { mode: 1, max_depth: 2 };
    let err = parse_ex_error(b"[[[0]]]", &opts);
    assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as i32);
    assert_eq!(err.offset, 2);
}

#[test]
fn invalid_arg_has_no_position() {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse(std::ptr::null(), 0, &mut err) };
    assert!(doc.is_null());
    assert_eq!(err.code, qjson_err::QJSON_INVALID_ARG as i32);
    assert_eq!(err.offset, usize::MAX);
}

#[test]
fn format_error_parse_and_snippet_messages() {
    let msg = format_error_message(qjson_err::QJSON_PARSE_ERROR, 1, 0, b"[}");
    assert_eq!(msg, "parse error at byte 1: unexpected '}', expected value");

    let msg = format_error_message(qjson_err::QJSON_INVALID_NUMBER, 1, 0, b"[01]");
    assert_eq!(msg, "invalid number '01' at byte 1");

    let msg = format_error_message(qjson_err::QJSON_TRAILING_CONTENT, 2, 0, b"{}garbage");
    assert_eq!(msg, "trailing content 'garbage' after root value at byte 2");

    let msg = format_error_message(qjson_err::QJSON_NESTING_TOO_DEEP, 2, 7, b"[[[0]]]");
    assert_eq!(msg, "nesting too deep at byte 2 (max 7)");
}

#[test]
fn format_error_type_mismatch_messages() {
    let doc = br#"{"user":{"age":42}}"#;
    let msg = format_error_message(qjson_err::QJSON_TYPE_MISMATCH, 15, 3, doc);
    assert_eq!(msg, "type mismatch: expected string, got number at byte 15");

    let msg = format_error_message(qjson_err::QJSON_TYPE_MISMATCH, usize::MAX, 3, doc);
    assert_eq!(msg, "type mismatch");

    let msg = format_error_message(qjson_err::QJSON_NOT_FOUND, usize::MAX, 0, doc);
    assert_eq!(msg, "path not found");
}

#[test]
fn format_error_respects_buffer_contract() {
    let json = br#"{"x":[}"#;
    let needed = unsafe {
        qjson_format_error(
            qjson_err::QJSON_PARSE_ERROR as i32,
            5,
            0,
            json.as_ptr() as *const c_char,
            json.len(),
            std::ptr::null_mut(),
            0,
        )
    };
    assert!(needed > 0);

    let mut too_small = vec![0xABu8; needed];
    let before = too_small.clone();
    let rc = unsafe {
        qjson_format_error(
            qjson_err::QJSON_PARSE_ERROR as i32,
            5,
            0,
            json.as_ptr() as *const c_char,
            json.len(),
            too_small.as_mut_ptr() as *mut c_char,
            too_small.len(),
        )
    };
    assert_eq!(rc, needed);
    assert_eq!(too_small, before, "buffer must remain untouched when too small");

    let mut out = vec![0u8; needed + 1];
    let rc = unsafe {
        qjson_format_error(
            qjson_err::QJSON_PARSE_ERROR as i32,
            5,
            0,
            json.as_ptr() as *const c_char,
            json.len(),
            out.as_mut_ptr() as *mut c_char,
            out.len(),
        )
    };
    assert_eq!(rc, needed);
    assert_eq!(out[needed], 0, "message must be NUL-terminated");
}

#[test]
fn doc_last_error_offset_tracks_access_failures_and_resets_on_success() {
    let doc = parse_doc_ok(br#"{"user":{"age":42,"big":9223372036854775808}}"#);
    assert_eq!(unsafe { qjson_doc_last_error_offset(doc) }, usize::MAX);

    let mut str_ptr: *const u8 = std::ptr::null();
    let mut str_len: usize = 0;
    let age_path = b"user.age";
    let rc = unsafe {
        qjson_get_str(
            doc,
            age_path.as_ptr() as *const c_char,
            age_path.len(),
            &mut str_ptr,
            &mut str_len,
        )
    };
    assert_eq!(rc, qjson_err::QJSON_TYPE_MISMATCH as i32);
    assert_eq!(unsafe { qjson_doc_last_error_offset(doc) }, 15);

    let mut i64_out = 0_i64;
    let rc = unsafe {
        qjson_get_i64(
            doc,
            age_path.as_ptr() as *const c_char,
            age_path.len(),
            &mut i64_out,
        )
    };
    assert_eq!(rc, qjson_err::QJSON_OK as i32);
    assert_eq!(i64_out, 42);
    assert_eq!(unsafe { qjson_doc_last_error_offset(doc) }, usize::MAX);

    let big_path = b"user.big";
    let rc = unsafe {
        qjson_get_i64(
            doc,
            big_path.as_ptr() as *const c_char,
            big_path.len(),
            &mut i64_out,
        )
    };
    assert_eq!(rc, qjson_err::QJSON_OUT_OF_RANGE as i32);
    assert_eq!(unsafe { qjson_doc_last_error_offset(doc) }, 24);

    let mut user_cur = qjson_cursor {
        doc: std::ptr::null(),
        idx_start: 0,
        idx_end: 0,
        _reserved0: 0,
        _reserved1: 0,
    };
    let user_path = b"user";
    let rc = unsafe {
        qjson_open(
            doc,
            user_path.as_ptr() as *const c_char,
            user_path.len(),
            &mut user_cur,
        )
    };
    assert_eq!(rc, qjson_err::QJSON_OK as i32);

    let leaf_path = b"age";
    let rc = unsafe {
        qjson_cursor_get_str(
            &user_cur,
            leaf_path.as_ptr() as *const c_char,
            leaf_path.len(),
            &mut str_ptr,
            &mut str_len,
        )
    };
    assert_eq!(rc, qjson_err::QJSON_TYPE_MISMATCH as i32);
    assert_eq!(unsafe { qjson_doc_last_error_offset(doc) }, 15);

    let rc = unsafe {
        qjson_cursor_get_i64(
            &user_cur,
            leaf_path.as_ptr() as *const c_char,
            leaf_path.len(),
            &mut i64_out,
        )
    };
    assert_eq!(rc, qjson_err::QJSON_OK as i32);
    assert_eq!(i64_out, 42);
    assert_eq!(unsafe { qjson_doc_last_error_offset(doc) }, usize::MAX);

    unsafe { qjson_free(doc) };
}

#[test]
fn lazy_string_decode_error_reports_string_content_offset() {
    let opts = Options { mode: 1, max_depth: 0 };
    let doc = {
        let mut err = qjson_error::default();
        let json = br#"{"s":"\001"}"#;
        let doc = unsafe { qjson_parse_ex(json.as_ptr(), json.len(), &opts, &mut err) };
        assert!(!doc.is_null(), "parse_ex unexpectedly failed with {:?}", err);
        doc
    };

    let mut str_ptr: *const u8 = std::ptr::null();
    let mut str_len: usize = 0;
    let path = b"s";
    let rc = unsafe {
        qjson_get_str(
            doc,
            path.as_ptr() as *const c_char,
            path.len(),
            &mut str_ptr,
            &mut str_len,
        )
    };
    assert_eq!(rc, qjson_err::QJSON_INVALID_STRING as i32);
    assert_eq!(unsafe { qjson_doc_last_error_offset(doc) }, 6);

    unsafe { qjson_free(doc) };
}
