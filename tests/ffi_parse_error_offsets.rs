use qjson::error::qjson_err;
use qjson::ffi::{
    qjson_error, qjson_free, qjson_parse, qjson_parse_ex,
};
use qjson::options::Options;

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
