use qjson::error::qjson_err;
use qjson::ffi::{qjson_error, qjson_free, qjson_parse_ex};
use qjson::options::{Options, QJSON_MODE_EAGER};

const NO_OFFSET: usize = usize::MAX;

#[derive(Clone, Copy)]
struct OkCase {
    name: &'static str,
    input: &'static [u8],
}

#[derive(Clone, Copy)]
struct ErrorCase {
    name: &'static str,
    input: &'static [u8],
    code: qjson_err,
    offset: usize,
}

fn eager_options() -> Options {
    Options {
        mode: QJSON_MODE_EAGER,
        max_depth: 0,
    }
}

fn parse_eager_ok(input: &[u8]) {
    let opts = eager_options();
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(input.as_ptr(), input.len(), &opts, &mut err) };
    assert!(!doc.is_null(), "unexpected parse failure for {input:?}: {err:?}");
    assert_eq!(
        err.code,
        qjson_err::QJSON_OK as i32,
        "expected QJSON_OK for {input:?}"
    );
    assert_eq!(
        err.offset,
        NO_OFFSET,
        "expected no offset sentinel for {input:?}"
    );
    unsafe { qjson_free(doc) };
}

fn parse_eager_error(input: &[u8]) -> qjson_error {
    let opts = eager_options();
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(input.as_ptr(), input.len(), &opts, &mut err) };
    assert!(doc.is_null(), "unexpected parse success for {input:?}");
    err
}

fn assert_ok_cases(cases: &[OkCase]) {
    for case in cases {
        assert!(!case.name.is_empty(), "ok case name must not be empty");
        parse_eager_ok(case.input);
    }
}

fn assert_error_cases(cases: &[ErrorCase]) {
    for case in cases {
        let err = parse_eager_error(case.input);
        assert_eq!(
            err.code,
            case.code as i32,
            "{}: wrong error code for {:?}",
            case.name,
            case.input
        );
        assert_eq!(
            err.offset,
            case.offset,
            "{}: wrong error offset for {:?}",
            case.name,
            case.input
        );
    }
}

#[test]
fn eager_numbers_accept_valid_grammar() {
    let cases = [
        OkCase {
            name: "zero",
            input: b"[0]",
        },
        OkCase {
            name: "negative zero",
            input: b"[-0]",
        },
        OkCase {
            name: "integer",
            input: b"[42]",
        },
        OkCase {
            name: "fraction",
            input: b"[3.1415]",
        },
        OkCase {
            name: "positive exponent",
            input: b"[1e+10]",
        },
        OkCase {
            name: "negative exponent",
            input: b"[1e-10]",
        },
        OkCase {
            name: "fraction exponent",
            input: b"[9.5E2]",
        },
    ];
    assert_ok_cases(&cases);
}

#[test]
fn eager_numbers_accept_very_long_number() {
    let mut input = Vec::with_capacity(260);
    input.push(b'[');
    input.extend_from_slice(&[b'9'; 256]);
    input.push(b']');
    parse_eager_ok(&input);
}

#[test]
fn eager_numbers_reject_invalid_grammar_with_offsets() {
    let cases = [
        ErrorCase {
            name: "leading zero",
            input: b"[01]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "leading plus",
            input: b"[+1]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "leading dot",
            input: b"[.5]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "trailing decimal point",
            input: b"[1.]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "decimal without fraction digits before exponent",
            input: b"[1.e2]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "missing exponent digits",
            input: b"[1e]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "missing exponent digits after plus sign",
            input: b"[1e+]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "missing exponent digits after minus sign",
            input: b"[1e-]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "nan",
            input: b"[NaN]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "infinity",
            input: b"[Infinity]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "negative infinity",
            input: b"[-Infinity]",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 1,
        },
        ErrorCase {
            name: "object value invalid after decimal",
            input: b"{\"n\":1.}",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 5,
        },
        ErrorCase {
            name: "object value invalid after exponent",
            input: b"{\"n\":1e+}",
            code: qjson_err::QJSON_INVALID_NUMBER,
            offset: 5,
        },
    ];
    assert_error_cases(&cases);
}

#[test]
fn eager_strings_accept_valid_grammar() {
    let cases = [
        OkCase {
            name: "plain ascii",
            input: b"[\"hello\"]",
        },
        OkCase {
            name: "escaped controls",
            input: b"[\"line\\nfeed\\t\"]",
        },
        OkCase {
            name: "escaped unicode",
            input: b"[\"caf\\u00e9\"]",
        },
        OkCase {
            name: "surrogate pair",
            input: b"[\"\\uD83D\\uDE00\"]",
        },
    ];
    assert_ok_cases(&cases);
}

#[test]
fn eager_strings_reject_invalid_grammar_with_offsets() {
    let cases = [
        ErrorCase {
            name: "raw tab",
            input: b"[\"a\tb\"]",
            code: qjson_err::QJSON_INVALID_STRING,
            offset: 1,
        },
        ErrorCase {
            name: "raw nul",
            input: b"[\"a\x00b\"]",
            code: qjson_err::QJSON_INVALID_STRING,
            offset: 1,
        },
        ErrorCase {
            name: "invalid escape",
            input: b"[\"\\q\"]",
            code: qjson_err::QJSON_INVALID_STRING,
            offset: 1,
        },
        ErrorCase {
            name: "truncated unicode escape",
            input: b"[\"\\u12\"]",
            code: qjson_err::QJSON_INVALID_STRING,
            offset: 1,
        },
        ErrorCase {
            name: "lone high surrogate escape",
            input: b"[\"\\uD800\"]",
            code: qjson_err::QJSON_INVALID_STRING,
            offset: 1,
        },
        ErrorCase {
            name: "invalid utf8 bytes",
            input: &[b'[', b'"', 0xC0, 0xC0, b'"', b']'],
            code: qjson_err::QJSON_INVALID_UTF8,
            offset: 1,
        },
    ];
    assert_error_cases(&cases);
}

#[test]
fn eager_literals_accept_valid_grammar() {
    let cases = [
        OkCase {
            name: "true",
            input: b"true",
        },
        OkCase {
            name: "false",
            input: b"false",
        },
        OkCase {
            name: "null",
            input: b"null",
        },
        OkCase {
            name: "literals in array",
            input: b"[true,false,null]",
        },
    ];
    assert_ok_cases(&cases);
}

#[test]
fn eager_literals_reject_invalid_grammar_with_offsets() {
    let cases = [
        ErrorCase {
            name: "truncated true",
            input: b"tru",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 0,
        },
        ErrorCase {
            name: "truncated false",
            input: b"fals",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 0,
        },
        ErrorCase {
            name: "truncated null",
            input: b"nul",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 0,
        },
        ErrorCase {
            name: "misspelled true",
            input: b"ture",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 0,
        },
        ErrorCase {
            name: "misspelled false",
            input: b"flase",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 0,
        },
        ErrorCase {
            name: "misspelled null",
            input: b"nlul",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 0,
        },
        ErrorCase {
            name: "array interrupted literal",
            input: b"[tru]",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 1,
        },
        ErrorCase {
            name: "object interrupted literal",
            input: b"{\"x\":fals}",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 5,
        },
    ];
    assert_error_cases(&cases);
}

#[test]
fn eager_rejects_trailing_content_with_exact_offsets() {
    let cases = [
        ErrorCase {
            name: "container trailing garbage",
            input: b"{}garbage",
            code: qjson_err::QJSON_TRAILING_CONTENT,
            offset: 2,
        },
        ErrorCase {
            name: "array followed by scalar",
            input: b"[] 0",
            code: qjson_err::QJSON_TRAILING_CONTENT,
            offset: 3,
        },
        ErrorCase {
            name: "multiple numeric roots",
            input: b"1 2",
            code: qjson_err::QJSON_TRAILING_CONTENT,
            offset: 2,
        },
        ErrorCase {
            name: "multiple literal roots",
            input: b"true false",
            code: qjson_err::QJSON_TRAILING_CONTENT,
            offset: 5,
        },
    ];
    assert_error_cases(&cases);
}

#[test]
fn eager_parse_error_offsets_cover_key_and_element_boundaries() {
    let cases = [
        ErrorCase {
            name: "missing colon after object key",
            input: b"{\"a\"}",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 4,
        },
        ErrorCase {
            name: "scalar after object key without colon",
            input: b"{\"a\" 1}",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 5,
        },
        ErrorCase {
            name: "missing comma after array element",
            input: b"[\"x\" \"y\"]",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 5,
        },
        ErrorCase {
            name: "array trailing comma",
            input: b"[1,]",
            code: qjson_err::QJSON_PARSE_ERROR,
            offset: 3,
        },
    ];
    assert_error_cases(&cases);
}
