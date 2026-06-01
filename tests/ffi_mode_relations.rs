use qjson::error::{QJSON_NO_OFFSET, qjson_err};
use qjson::ffi::{
    qjson_cursor, qjson_cursor_get_i64, qjson_cursor_get_str, qjson_cursor_typeof,
    qjson_doc, qjson_doc_last_error_offset, qjson_error, qjson_free, qjson_get_i64, qjson_get_str,
    qjson_open, qjson_parse_ex, qjson_typeof,
};
use qjson::options::{Options, QJSON_DEFAULT_MAX_DEPTH, QJSON_MODE_EAGER, QJSON_MODE_LAZY};
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int};

fn eager() -> Options {
    Options { mode: QJSON_MODE_EAGER, max_depth: 0 }
}

fn lazy() -> Options {
    Options { mode: QJSON_MODE_LAZY, max_depth: 0 }
}

fn with_depth(mode: u32, max_depth: u32) -> Options {
    Options { mode, max_depth }
}

struct DocGuard(*mut qjson_doc);

impl DocGuard {
    fn as_ptr(&self) -> *mut qjson_doc {
        self.0
    }
}

impl Drop for DocGuard {
    fn drop(&mut self) {
        unsafe {
            qjson_free(self.0);
        }
    }
}

fn parse_ok(buf: &[u8], opts: &Options) -> DocGuard {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(buf.as_ptr(), buf.len(), opts, &mut err) };
    assert!(!doc.is_null(), "parse_ex unexpectedly failed for {:?}: {:?}", buf, err);
    assert_eq!(err.code, qjson_err::QJSON_OK as i32);
    assert_eq!(err.offset, QJSON_NO_OFFSET);
    DocGuard(doc)
}

fn parse_err(buf: &[u8], opts: &Options) -> qjson_error {
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(buf.as_ptr(), buf.len(), opts, &mut err) };
    assert!(doc.is_null(), "parse_ex unexpectedly succeeded for {:?}: {:?}", buf, err);
    err
}

fn open_root_cursor(doc: *mut qjson_doc) -> qjson_cursor {
    let mut cursor = MaybeUninit::<qjson_cursor>::uninit();
    let empty = b"";
    let rc = unsafe { qjson_open(doc, empty.as_ptr() as *const c_char, 0, cursor.as_mut_ptr()) };
    assert_eq!(rc, qjson_err::QJSON_OK as i32);
    unsafe { cursor.assume_init() }
}

fn open_cursor(doc: *mut qjson_doc, path: &[u8]) -> qjson_cursor {
    let mut cursor = MaybeUninit::<qjson_cursor>::uninit();
    let rc = unsafe {
        qjson_open(doc, path.as_ptr() as *const c_char, path.len(), cursor.as_mut_ptr())
    };
    assert_eq!(rc, qjson_err::QJSON_OK as i32);
    unsafe { cursor.assume_init() }
}

fn root_i64_error(doc: *mut qjson_doc, path: &[u8]) -> (i32, usize) {
    let mut out = 0_i64;
    let rc = unsafe { qjson_get_i64(doc, path.as_ptr() as *const c_char, path.len(), &mut out) };
    let offset = unsafe { qjson_doc_last_error_offset(doc) };
    (rc, offset)
}

fn cursor_i64_error(doc: *mut qjson_doc, path: &[u8]) -> (i32, usize) {
    let cursor = open_root_cursor(doc);
    let mut out = 0_i64;
    let rc = unsafe {
        qjson_cursor_get_i64(&cursor, path.as_ptr() as *const c_char, path.len(), &mut out)
    };
    let offset = unsafe { qjson_doc_last_error_offset(doc) };
    (rc, offset)
}

fn root_str_error(doc: *mut qjson_doc, path: &[u8]) -> (i32, usize) {
    let mut out_ptr: *const u8 = std::ptr::null();
    let mut out_len = 0usize;
    let rc = unsafe {
        qjson_get_str(
            doc,
            path.as_ptr() as *const c_char,
            path.len(),
            &mut out_ptr,
            &mut out_len,
        )
    };
    let offset = unsafe { qjson_doc_last_error_offset(doc) };
    (rc, offset)
}

fn cursor_str_error(doc: *mut qjson_doc, path: &[u8]) -> (i32, usize) {
    let cursor = open_root_cursor(doc);
    let mut out_ptr: *const u8 = std::ptr::null();
    let mut out_len = 0usize;
    let rc = unsafe {
        qjson_cursor_get_str(
            &cursor,
            path.as_ptr() as *const c_char,
            path.len(),
            &mut out_ptr,
            &mut out_len,
        )
    };
    let offset = unsafe { qjson_doc_last_error_offset(doc) };
    (rc, offset)
}

fn root_typeof_error(doc: *mut qjson_doc, path: &[u8]) -> (i32, usize) {
    let mut out: c_int = -1;
    let rc = unsafe { qjson_typeof(doc, path.as_ptr() as *const c_char, path.len(), &mut out) };
    let offset = unsafe { qjson_doc_last_error_offset(doc) };
    (rc, offset)
}

fn cursor_typeof_error(doc: *mut qjson_doc, path: &[u8]) -> (i32, usize) {
    let cursor = open_root_cursor(doc);
    let mut out: c_int = -1;
    let rc = unsafe {
        qjson_cursor_typeof(&cursor, path.as_ptr() as *const c_char, path.len(), &mut out)
    };
    let offset = unsafe { qjson_doc_last_error_offset(doc) };
    (rc, offset)
}

fn cursor_typeof_error_from(doc: *mut qjson_doc, cursor: &qjson_cursor, path: &[u8]) -> (i32, usize) {
    let mut out: c_int = -1;
    let rc = unsafe {
        qjson_cursor_typeof(cursor, path.as_ptr() as *const c_char, path.len(), &mut out)
    };
    let offset = unsafe { qjson_doc_last_error_offset(doc) };
    (rc, offset)
}

#[test]
fn eager_accepts_representative_legal_texts_implies_lazy_accepts() {
    let eager_opts = eager();
    let lazy_opts = lazy();
    let cases: &[&[u8]] = &[
        b"null",
        b"true",
        b"false",
        b"-123.45e+6",
        b"\"hello\"",
        b"\"line\\n\\t\\u00e9\"",
        b"[]",
        b"{}",
        b"[1,\"x\",false,null,{\"k\":[2]}]",
        b"{\"a\":{\"b\":[{\"c\":\"d\"}]}}",
    ];
    for case in cases {
        let _eager = parse_ok(case, &eager_opts);
        let _lazy = parse_ok(case, &lazy_opts);
    }
}

enum AccessKind {
    I64,
    Str,
    Typeof,
}

#[test]
fn eager_value_rejections_defer_to_lazy_access_time() {
    struct Case {
        name: &'static str,
        json: &'static [u8],
        path: &'static [u8],
        eager_code: qjson_err,
        eager_offset: usize,
        lazy_code: qjson_err,
        lazy_offset: usize,
        access: AccessKind,
    }

    let cases = [
        Case {
            name: "invalid number",
            json: b"{\"x\":01}",
            path: b"x",
            eager_code: qjson_err::QJSON_INVALID_NUMBER,
            eager_offset: 5,
            lazy_code: qjson_err::QJSON_INVALID_NUMBER,
            lazy_offset: 5,
            access: AccessKind::I64,
        },
        Case {
            name: "raw tab in string",
            json: b"{\"x\":\"a\tb\"}",
            path: b"x",
            eager_code: qjson_err::QJSON_INVALID_STRING,
            eager_offset: 5,
            lazy_code: qjson_err::QJSON_INVALID_STRING,
            lazy_offset: 6,
            access: AccessKind::Str,
        },
        Case {
            name: "invalid utf8 in string",
            json: b"{\"x\":\"\xff\"}",
            path: b"x",
            eager_code: qjson_err::QJSON_INVALID_UTF8,
            eager_offset: 5,
            lazy_code: qjson_err::QJSON_INVALID_UTF8,
            lazy_offset: 6,
            access: AccessKind::Str,
        },
        Case {
            name: "invalid literal",
            json: b"{\"x\":TRUE}",
            path: b"x",
            eager_code: qjson_err::QJSON_PARSE_ERROR,
            eager_offset: 5,
            lazy_code: qjson_err::QJSON_PARSE_ERROR,
            lazy_offset: 5,
            access: AccessKind::Typeof,
        },
    ];

    for case in cases {
        let eager_err = parse_err(case.json, &eager());
        assert_eq!(eager_err.code, case.eager_code as i32, "{}", case.name);
        assert_eq!(eager_err.offset, case.eager_offset, "{}", case.name);

        let lazy_doc = parse_ok(case.json, &lazy());
        let (root_rc, root_offset) = match case.access {
            AccessKind::I64 => root_i64_error(lazy_doc.as_ptr(), case.path),
            AccessKind::Str => root_str_error(lazy_doc.as_ptr(), case.path),
            AccessKind::Typeof => root_typeof_error(lazy_doc.as_ptr(), case.path),
        };
        let (cursor_rc, cursor_offset) = match case.access {
            AccessKind::I64 => cursor_i64_error(lazy_doc.as_ptr(), case.path),
            AccessKind::Str => cursor_str_error(lazy_doc.as_ptr(), case.path),
            AccessKind::Typeof => cursor_typeof_error(lazy_doc.as_ptr(), case.path),
        };

        assert_eq!(root_rc, case.lazy_code as i32, "{}", case.name);
        assert_eq!(cursor_rc, case.lazy_code as i32, "{}", case.name);
        assert_eq!(root_offset, case.lazy_offset, "{}", case.name);
        assert_eq!(cursor_offset, case.lazy_offset, "{}", case.name);
    }
}

#[test]
fn lazy_nested_malformed_container_errors_match_after_root_then_cursor_access() {
    let doc = parse_ok(b"{\"obj\":{\"a\":1 \"b\":2}}", &lazy());

    let root_err = root_typeof_error(doc.as_ptr(), b"obj.b");
    assert_eq!(root_err.0, qjson_err::QJSON_PARSE_ERROR as i32);

    let obj = open_cursor(doc.as_ptr(), b"obj");
    let cursor_err = cursor_typeof_error_from(doc.as_ptr(), &obj, b"b");

    assert_eq!(cursor_err.0, root_err.0);
    // The root path cannot report the inner container that failed during
    // resolution; cursor-relative access can report that container's opener.
    assert_eq!(root_err.1, QJSON_NO_OFFSET);
    assert_eq!(cursor_err.1, 7);
}

fn nested_arrays(depth: usize) -> Vec<u8> {
    let mut buf = vec![b'['; depth];
    buf.extend(std::iter::repeat_n(b']', depth));
    buf
}

#[test]
fn depth_errors_rejected_in_both_modes_for_default_and_small_limits() {
    let default_too_deep = nested_arrays(QJSON_DEFAULT_MAX_DEPTH as usize + 1);
    for mode in [QJSON_MODE_EAGER, QJSON_MODE_LAZY] {
        let err = parse_err(&default_too_deep, &with_depth(mode, 0));
        assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as i32);
        assert_eq!(err.offset, QJSON_DEFAULT_MAX_DEPTH as usize);
    }

    let explicit_too_deep = b"[[[0]]]";
    for mode in [QJSON_MODE_EAGER, QJSON_MODE_LAZY] {
        let err = parse_err(explicit_too_deep, &with_depth(mode, 2));
        assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as i32);
        assert_eq!(err.offset, 2);
    }
}

#[test]
fn max_depth_boundary_matches_in_both_modes() {
    let limit = 8u32;
    let at_limit = nested_arrays(limit as usize);
    let one_past_limit = nested_arrays(limit as usize + 1);

    for mode in [QJSON_MODE_EAGER, QJSON_MODE_LAZY] {
        let _ok = parse_ok(&at_limit, &with_depth(mode, limit));

        let err = parse_err(&one_past_limit, &with_depth(mode, limit));
        assert_eq!(err.code, qjson_err::QJSON_NESTING_TOO_DEEP as i32);
        assert_eq!(err.offset, limit as usize);
    }
}

#[test]
fn trailing_content_is_eager_only() {
    // Intentional boundary: lazy mode skips trailing-content validation.
    let cases: &[(&[u8], usize)] = &[(b"{}garbage", 2), (b"1 2", 2), (b"true false", 5)];

    for (input, eager_offset) in cases {
        let eager_err = parse_err(input, &eager());
        assert_eq!(eager_err.code, qjson_err::QJSON_TRAILING_CONTENT as i32);
        assert_eq!(eager_err.offset, *eager_offset);

        let _lazy_ok = parse_ok(input, &lazy());
    }
}
