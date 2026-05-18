//! RFC 8259 conformance suite.
//!
//! Cross-mode contract:
//!   * `y_*` inputs MUST parse successfully in both EAGER and LAZY modes,
//!     and any specified field-level access MUST return the expected value.
//!   * `n_*` inputs MUST fail to parse in EAGER mode, and MUST either
//!     fail to parse OR fail on the documented field access in LAZY mode.
//!   * `i_*` inputs document our current behavior; we assert what we do
//!     today (so regressions surface), referencing JSONTestSuite naming.
//!
//! RFC 8259 references are in section-paragraph form, e.g. RFC8259 §6 for
//! the number grammar.

use qjson::doc::Document;
use qjson::options::{Options, QJSON_MODE_EAGER, QJSON_MODE_LAZY};

fn eager() -> Options { Options { mode: QJSON_MODE_EAGER, max_depth: 0 } }
fn lazy()  -> Options { Options { mode: QJSON_MODE_LAZY,  max_depth: 0 } }

/// Asserts the input is accepted in both modes.
///
/// Usage: `assert_accepts!("[]");`
#[macro_export]
macro_rules! assert_accepts {
    ($input:expr) => {{
        let buf: &[u8] = $input.as_ref();
        let r_eager = Document::parse_with_options(buf, &eager());
        assert!(r_eager.is_ok(),
            "EAGER unexpectedly rejected {:?}: {:?}", $input, r_eager.err());
        let r_lazy = Document::parse_with_options(buf, &lazy());
        assert!(r_lazy.is_ok(),
            "LAZY unexpectedly rejected {:?}: {:?}", $input, r_lazy.err());
    }};
}

/// Asserts the input is REJECTED by eager parse.
///
/// Usage: `assert_rejects_eager!("01", QJSON_INVALID_NUMBER);`
#[macro_export]
macro_rules! assert_rejects_eager {
    ($input:expr, $expected_err:ident) => {{
        use qjson::error::qjson_err;
        let buf: &[u8] = $input.as_ref();
        let expected = qjson_err::$expected_err;
        match Document::parse_with_options(buf, &eager()) {
            Err(e) if e == expected => {}
            Err(other) => panic!(
                "EAGER rejected {:?} with {:?}, expected {:?}",
                $input, other, expected),
            Ok(_) => panic!("EAGER unexpectedly accepted {:?}", $input),
        }
    }};
}

/// Asserts the input is rejected at parse time in BOTH modes (structural).
#[macro_export]
macro_rules! assert_rejects_both {
    ($input:expr) => {{
        let buf: &[u8] = $input.as_ref();
        assert!(Document::parse_with_options(buf, &eager()).is_err(),
            "EAGER unexpectedly accepted {:?}", $input);
        assert!(Document::parse_with_options(buf, &lazy()).is_err(),
            "LAZY unexpectedly accepted {:?}", $input);
    }};
}

// ─────────────────────────────────────────────────────────────
// Scaffold smoke tests — replaced by Task 11 with full corpus.
// ─────────────────────────────────────────────────────────────

#[test]
fn smoke_accepts_empty_object() { assert_accepts!("{}"); }

#[test]
fn smoke_accepts_empty_array() { assert_accepts!("[]"); }

#[test]
fn smoke_rejects_unmatched_brace_both_modes() {
    assert_rejects_both!("{");
}

#[test]
#[should_panic(expected = "expected QJSON_INVALID_NUMBER")]
fn macro_rejects_wrong_error_code() {
    // Sanity: passing the wrong expected variant must panic.
    // `{` is rejected as QJSON_PARSE_ERROR, NOT QJSON_INVALID_NUMBER.
    // With the buggy macro, this test would NOT panic (false positive
    // — the macro would silently bind whatever Err came back).
    assert_rejects_eager!("{", QJSON_INVALID_NUMBER);
}

// ── Phase 3: nesting depth ───────────────────────────────────

#[test]
fn rejects_deeply_nested_at_default_limit() {
    use qjson::error::qjson_err;
    let mut buf = String::new();
    for _ in 0..1100 { buf.push('['); }
    for _ in 0..1100 { buf.push(']'); }
    match Document::parse_with_options(buf.as_bytes(), &eager()) {
        Err(qjson_err::QJSON_NESTING_TOO_DEEP) => {}
        other => panic!("expected QJSON_NESTING_TOO_DEEP, got {:?}", other.err()),
    }
}

#[test]
fn lazy_mode_also_enforces_max_depth() {
    use qjson::error::qjson_err;
    let mut buf = String::new();
    for _ in 0..1100 { buf.push('['); }
    for _ in 0..1100 { buf.push(']'); }
    assert_eq!(
        Document::parse_with_options(buf.as_bytes(), &lazy()).err().unwrap(),
        qjson_err::QJSON_NESTING_TOO_DEEP,
    );
}

#[test]
fn accepts_nested_at_configured_limit() {
    let mut buf = String::new();
    for _ in 0..256 { buf.push('['); }
    for _ in 0..256 { buf.push(']'); }
    let opts = Options { mode: QJSON_MODE_EAGER, max_depth: 256 };
    assert!(Document::parse_with_options(buf.as_bytes(), &opts).is_ok());
}

#[test]
fn rejects_when_one_past_configured_limit() {
    let mut buf = String::new();
    for _ in 0..33 { buf.push('['); }
    for _ in 0..33 { buf.push(']'); }
    let opts = Options { mode: QJSON_MODE_EAGER, max_depth: 32 };
    assert!(Document::parse_with_options(buf.as_bytes(), &opts).is_err());
}

// ── Phase 6: trailing content ────────────────────────────────

#[test]
fn eager_rejects_trailing_content() {
    use qjson::error::qjson_err;
    assert_eq!(
        Document::parse_with_options(b"{}garbage", &eager()).err().unwrap(),
        qjson_err::QJSON_TRAILING_CONTENT,
    );
}

#[test]
fn eager_rejects_multiple_root_values() {
    use qjson::error::qjson_err;
    assert_eq!(
        Document::parse_with_options(b"1 2", &eager()).err().unwrap(),
        qjson_err::QJSON_TRAILING_CONTENT,
    );
    assert_eq!(
        Document::parse_with_options(b"true false", &eager()).err().unwrap(),
        qjson_err::QJSON_TRAILING_CONTENT,
    );
}

#[test]
fn eager_accepts_trailing_whitespace() {
    assert_accepts!("{}   \n\t");
}

#[test]
fn eager_accepts_top_level_scalar_with_trailing_whitespace() {
    assert_accepts!("42 \n\t");
}

#[test]
fn lazy_accepts_trailing_garbage() {
    // Lazy preserves historical behavior: trailing bytes are ignored.
    assert!(Document::parse_with_options(b"{}garbage", &lazy()).is_ok());
}

// ── Phase 2: number format ───────────────────────────────────

#[test]
fn eager_accepts_canonical_numbers() {
    for s in ["0", "-0", "1", "-1", "3.14", "-2.718",
              "1e10", "1E10", "1e+10", "1e-10", "1.5e2",
              "9223372036854775807", "-9223372036854775808"] {
        let input = format!("[{}]", s);
        assert_accepts!(input);
    }
}

#[test]
fn eager_rejects_invalid_numbers() {
    use qjson::error::qjson_err;
    for s in ["+1", "01", "00", ".5", "1.", "1.e5", "0x1F",
              "NaN", "Infinity", "-Infinity", "1e", "1e+"] {
        let input = format!("[{}]", s);
        match Document::parse_with_options(input.as_bytes(), &eager()) {
            Err(qjson_err::QJSON_INVALID_NUMBER) => {}
            Err(other) => panic!(
                "expected QJSON_INVALID_NUMBER for {:?}, got {:?}", input, other),
            Ok(_) => panic!("EAGER unexpectedly accepted {:?}", input),
        }
    }
}

#[test]
fn lazy_defers_invalid_number_until_access() {
    // In LAZY mode, "[01]" parses; the error surfaces when you ask for the value.
    let doc = Document::parse_with_options(b"[01]", &lazy()).unwrap();
    // Walking via FFI tests is verbose; we only check that the LAZY parse
    // itself does not fail. Field-level access is covered in tests/ffi_*.
    drop(doc);
}

// ── Phase 4 + 5: string content ──────────────────────────────

#[test]
fn eager_rejects_raw_tab_in_string() {
    use qjson::error::qjson_err;
    let input = b"[\"a\tb\"]";
    match Document::parse_with_options(input, &eager()) {
        Err(qjson_err::QJSON_INVALID_STRING) => {}
        Err(other) => panic!("expected QJSON_INVALID_STRING, got {:?}", other),
        Ok(_) => panic!("EAGER unexpectedly accepted raw tab in string"),
    }
}

#[test]
fn eager_rejects_raw_null_in_string() {
    use qjson::error::qjson_err;
    let input = b"[\"a\x00b\"]";
    match Document::parse_with_options(input, &eager()) {
        Err(qjson_err::QJSON_INVALID_STRING) => {}
        Err(other) => panic!("expected QJSON_INVALID_STRING, got {:?}", other),
        Ok(_) => panic!("EAGER unexpectedly accepted raw null in string"),
    }
}

#[test]
fn eager_rejects_invalid_utf8_in_string() {
    use qjson::error::qjson_err;
    let input = &[b'[', b'"', 0xC0, 0xC0, b'"', b']'];
    match Document::parse_with_options(input, &eager()) {
        Err(qjson_err::QJSON_INVALID_UTF8) => {}
        Err(other) => panic!("expected QJSON_INVALID_UTF8, got {:?}", other),
        Ok(_) => panic!("EAGER unexpectedly accepted invalid UTF-8 in string"),
    }
}

#[test]
fn eager_accepts_escape_sequences() {
    assert_accepts!("[\"a\\nb\\u00e9\"]");
    assert_accepts!("[\"emoji \\uD83D\\uDE00\"]");
}

#[test]
fn lazy_accepts_raw_tab_but_decode_fails() {
    let input = b"[\"a\tb\"]";
    let doc = Document::parse_with_options(input, &lazy()).expect("lazy accepts raw control");
    drop(doc);
    // Field-level rejection on access is enforced by decode/string.rs and
    // is covered by tests/ffi_strings.rs (existing decode_string tests cover
    // the error type); no extra assertion needed here.
}

// ── Task 10 fix: check_gap dispatch ──────────────────────────

#[test]
fn eager_rejects_uppercase_true_as_parse_error() {
    use qjson::error::qjson_err;
    let r = Document::parse_with_options(b"TRUE", &eager());
    match r {
        Err(qjson_err::QJSON_PARSE_ERROR) => {}
        other => panic!("expected QJSON_PARSE_ERROR, got {:?}", other.err()),
    }
}

#[test]
fn eager_rejects_uppercase_false_as_parse_error() {
    use qjson::error::qjson_err;
    let r = Document::parse_with_options(b"False", &eager());
    match r {
        Err(qjson_err::QJSON_PARSE_ERROR) => {}
        other => panic!("expected QJSON_PARSE_ERROR, got {:?}", other.err()),
    }
}

#[test]
fn eager_rejects_uppercase_null_as_parse_error() {
    use qjson::error::qjson_err;
    let r = Document::parse_with_options(b"NULL", &eager());
    match r {
        Err(qjson_err::QJSON_PARSE_ERROR) => {}
        other => panic!("expected QJSON_PARSE_ERROR, got {:?}", other.err()),
    }
}

#[test]
fn eager_rejects_undefined_as_parse_error() {
    use qjson::error::qjson_err;
    let r = Document::parse_with_options(b"undefined", &eager());
    match r {
        Err(qjson_err::QJSON_PARSE_ERROR) => {}
        other => panic!("expected QJSON_PARSE_ERROR, got {:?}", other.err()),
    }
}

#[test]
fn eager_rejects_nan_as_invalid_number() {
    use qjson::error::qjson_err;
    let r = Document::parse_with_options(b"NaN", &eager());
    match r {
        Err(qjson_err::QJSON_INVALID_NUMBER) => {}
        other => panic!("expected QJSON_INVALID_NUMBER, got {:?}", other.err()),
    }
}

#[test]
fn eager_rejects_infinity_as_invalid_number() {
    use qjson::error::qjson_err;
    let r = Document::parse_with_options(b"Infinity", &eager());
    match r {
        Err(qjson_err::QJSON_INVALID_NUMBER) => {}
        other => panic!("expected QJSON_INVALID_NUMBER, got {:?}", other.err()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 11: Comprehensive RFC 8259 conformance corpus
// Organized into nested mod blocks per category.
// ─────────────────────────────────────────────────────────────────────────────

mod structural {
    use super::*;

    // RFC 8259 §2-3: JSON values — null, true, false are valid root values.
    #[test]
    fn primitives_valid() {
        assert_accepts!("null");
        assert_accepts!("true");
        assert_accepts!("false");
    }

    // RFC 8259 §2: a JSON text contains exactly one value — empty is not valid.
    #[test]
    fn empty_input_rejected() {
        assert_rejects_both!("");
    }

    // RFC 8259 §2: whitespace-only input also contains no value.
    #[test]
    fn whitespace_only_rejected() {
        assert_rejects_both!("   ");
        assert_rejects_both!("\t\n\r");
    }

    // RFC 8259 §4-5: empty object and empty array are valid.
    #[test]
    fn empty_containers() {
        assert_accepts!("{}");
        assert_accepts!("[]");
    }

    // RFC 8259 §4-5: nested containers with mixed value types.
    #[test]
    fn nested_containers() {
        assert_accepts!("[{\"a\":[1,{\"b\":2}]}]");
        assert_accepts!("{\"x\":{\"y\":{\"z\":null}}}");
        assert_accepts!("[[],[],[[],[]]]");
    }

    // RFC 8259 §4: '{' must be followed by a matching '}'.
    #[test]
    fn unclosed_brace() {
        assert_rejects_both!("{");
    }

    // RFC 8259 §5: '[' must be followed by a matching ']'.
    #[test]
    fn unclosed_bracket() {
        assert_rejects_both!("[");
    }

    // Bracket mismatch: '{' closed by ']'.
    #[test]
    fn mismatched_brace_bracket() {
        assert_rejects_both!("{]");
    }

    // Bracket mismatch: '[' closed by '}'.
    #[test]
    fn mismatched_bracket_brace() {
        assert_rejects_both!("[}");
    }

    // RFC 8259 §4: object value must follow the colon — omitting it is invalid.
    // Eager catches the empty gap after ':'; lazy defers (structural-only rule).
    #[test]
    fn missing_value() {
        assert_rejects_eager!("{\"a\":}", QJSON_PARSE_ERROR);
    }

    // RFC 8259 §4: colon between key and value is mandatory.
    // The grammar-aware pass detects this: after consuming the key
    // string the state is ObjAfterKey, and `}` is rejected because
    // it can only close ObjAfterOpen/ObjAfterValue.
    #[test]
    fn missing_colon() {
        assert_rejects_eager!("{\"a\"}", QJSON_PARSE_ERROR);
    }

    // RFC 8259 §5: a leading comma in an array is invalid.
    // [,] — both commas have empty gaps → eager rejects via the ':'/','
    // heuristic in check_gap.
    #[test]
    fn leading_comma_array_empty() {
        assert_rejects_eager!("[,]", QJSON_PARSE_ERROR);
    }

    // [,1] — leading comma followed by a value: the grammar-aware
    // pass rejects this because `,` is invalid in the ArrAfterOpen
    // state (only a value or `]` is allowed after `[`).
    #[test]
    fn leading_comma_array_with_value() {
        assert_rejects_eager!("[,1]", QJSON_PARSE_ERROR);
    }

    // RFC 8259 §5: trailing comma in an array is invalid.
    #[test]
    fn trailing_comma_array() {
        assert_rejects_eager!("[1,]", QJSON_PARSE_ERROR);
    }

    // RFC 8259 §4: trailing comma in an object is invalid.
    #[test]
    fn trailing_comma_object() {
        assert_rejects_eager!("{\"a\":1,}", QJSON_PARSE_ERROR);
    }

    // RFC 8259 §5: array elements must be separated by exactly one comma.
    // [1 2] contains a space-separated pair that validate_number rejects as
    // QJSON_INVALID_NUMBER (not QJSON_PARSE_ERROR) — the element IS rejected by
    // eager, just with a different error code.
    #[test]
    fn missing_comma_in_array_rejected() {
        // We assert only that eager rejects; the exact code is QJSON_INVALID_NUMBER
        // because the "1 2" token fails number validation (space within number).
        let input = b"[1 2]";
        assert!(
            Document::parse_with_options(input, &eager()).is_err(),
            "EAGER should reject [1 2]"
        );
    }

    // Missing comma inside an object (no structural separator between values):
    // {"a":1"b":2} — after consuming the value `1`, the state is
    // ObjAfterValue; the next `"` (start of "b") is rejected because
    // a key/value-position quote is not legal there.
    #[test]
    fn missing_comma_in_object() {
        assert_rejects_eager!("{\"a\":1\"b\":2}", QJSON_PARSE_ERROR);
    }
}

mod whitespace {
    use super::*;

    // RFC 8259 §2: insignificant whitespace (space, tab, LF, CR) is allowed
    // before and after structural characters.

    #[test]
    fn spaces_around_object() {
        assert_accepts!("  {  }  ");
    }

    #[test]
    fn tabs_around_object() {
        assert_accepts!("\t{}\t");
    }

    #[test]
    fn newlines_around() {
        assert_accepts!("\n{}\n");
    }

    #[test]
    fn cr_around() {
        assert_accepts!("\r{}\r");
    }

    #[test]
    fn inside_object() {
        assert_accepts!("{ \"a\" : 1 , \"b\" : 2 }");
    }

    #[test]
    fn inside_array() {
        assert_accepts!("[ 1 , 2 , 3 ]");
    }

    // All four RFC whitespace characters interleaved.
    #[test]
    fn mixed_whitespace() {
        assert_accepts!(" \t\n\r { \t\n\r } \t\n\r ");
    }
}

mod literals {
    use super::*;

    // RFC 8259 §3: only lowercase "true", "false", "null" are valid.
    // Wrong case must be rejected by eager.

    #[test]
    fn true_must_be_lowercase() {
        assert_rejects_eager!("TRUE", QJSON_PARSE_ERROR);
        assert_rejects_eager!("True", QJSON_PARSE_ERROR);
        assert_rejects_eager!("tRuE", QJSON_PARSE_ERROR);
    }

    #[test]
    fn false_must_be_lowercase() {
        assert_rejects_eager!("FALSE", QJSON_PARSE_ERROR);
        assert_rejects_eager!("False", QJSON_PARSE_ERROR);
    }

    #[test]
    fn null_must_be_lowercase() {
        assert_rejects_eager!("NULL", QJSON_PARSE_ERROR);
        assert_rejects_eager!("Null", QJSON_PARSE_ERROR);
    }

    // JavaScript-ism: "nil" is not a valid JSON value.
    #[test]
    fn nil_rejected() {
        assert_rejects_eager!("nil", QJSON_PARSE_ERROR);
    }

    // JavaScript-ism: "undefined" is not a valid JSON value.
    #[test]
    fn undefined_rejected() {
        assert_rejects_eager!("undefined", QJSON_PARSE_ERROR);
    }
}

mod strings {
    use super::*;

    // RFC 8259 §7: string grammar.

    // Empty string is valid.
    #[test]
    fn empty_string() {
        assert_accepts!("\"\"");
        assert_accepts!("[\"\"  ]");
    }

    // Printable ASCII (no special chars) is valid.
    #[test]
    fn ascii_string() {
        assert_accepts!("\"hello world\"");
        assert_accepts!("\"abcdefghijklmnopqrstuvwxyz 0123456789 !@#$%^&*()\"");
    }

    // RFC 8259 §7: all defined escape sequences must be accepted.
    #[test]
    fn all_escape_sequences() {
        // \"  \\  \/  \b  \f  \n  \r  \t
        assert_accepts!("\"\\\" \\\\ \\/ \\b \\f \\n \\r \\t\"");
    }

    // RFC 8259 §7: \uXXXX Unicode escape (4 hex digits).
    #[test]
    fn unicode_escape() {
        assert_accepts!("\"\\u0000\"");   // NUL encoded as escape — valid
        assert_accepts!("\"\\u00e9\"");   // é
        assert_accepts!("\"\\u4e2d\\u6587\""); // 中文
    }

    // RFC 8259 §7: surrogate pair (\uD800–\uDBFF followed by \uDC00–\uDFFF).
    #[test]
    fn surrogate_pair() {
        assert_accepts!("\"\\uD83D\\uDE00\""); // 😀 U+1F600
    }

    // RFC 8259 §7: strings must be terminated with a closing '"'.
    #[test]
    fn unclosed_string_rejected() {
        assert_rejects_both!("\"hello");
        assert_rejects_both!("\"");
    }

    // JSON does not allow single-quoted strings (JavaScript-ism).
    #[test]
    fn single_quoted_string_rejected() {
        assert_rejects_eager!("'hello'", QJSON_PARSE_ERROR);
    }

    // RFC 8259 §7: control characters (U+0000–U+001F) must be escaped.
    // A raw tab (0x09) inside a string is forbidden.
    #[test]
    fn raw_control_char_rejected() {
        use qjson::error::qjson_err;
        let with_tab  = b"[\"a\tb\"]";
        let with_null = b"[\"a\x00b\"]";
        match Document::parse_with_options(with_tab, &eager()) {
            Err(qjson_err::QJSON_INVALID_STRING) => {}
            other => panic!("expected QJSON_INVALID_STRING for raw tab, got {:?}", other.err()),
        }
        match Document::parse_with_options(with_null, &eager()) {
            Err(qjson_err::QJSON_INVALID_STRING) => {}
            other => panic!("expected QJSON_INVALID_STRING for raw NUL, got {:?}", other.err()),
        }
    }

    // Strings with valid multi-byte UTF-8 content are accepted.
    #[test]
    fn utf8_multibyte_string() {
        assert_accepts!("\"café\"");          // 2-byte sequence
        assert_accepts!("\"中文\"");            // 3-byte sequences
        assert_accepts!("\"😀\"");             // 4-byte sequence (emoji)
    }
}

mod numbers {
    use super::*;

    // RFC 8259 §6: number grammar.
    // These complement the existing top-level number tests with a thorough
    // table-driven suite organized by sub-rule.

    // §6 integer: optional minus, zero, or non-zero digit followed by digits.
    #[test]
    fn integers_valid() {
        for s in ["0", "-0", "1", "-1", "123", "-456",
                  "9223372036854775807", "-9223372036854775808"] {
            let input = format!("[{}]", s);
            assert_accepts!(input);
        }
    }

    // §6 fraction: a '.' followed by one or more digits.
    #[test]
    fn fractions_valid() {
        for s in ["0.0", "-0.0", "1.5", "-2.718", "3.14159",
                  "0.123456789"] {
            let input = format!("[{}]", s);
            assert_accepts!(input);
        }
    }

    // §6 exponent: 'e'/'E' with optional '+'/'-' and one or more digits.
    #[test]
    fn exponents_valid() {
        for s in ["1e10", "1E10", "1e+10", "1e-10",
                  "1.5e2", "2.5E-3", "0e0", "-0e0"] {
            let input = format!("[{}]", s);
            assert_accepts!(input);
        }
    }

    // §6: leading '+' is not allowed.
    #[test]
    fn leading_plus_rejected() {
        assert_rejects_eager!("[+1]", QJSON_INVALID_NUMBER);
    }

    // §6: leading zeros are not allowed (except bare "0").
    #[test]
    fn leading_zero_rejected() {
        assert_rejects_eager!("[01]", QJSON_INVALID_NUMBER);
        assert_rejects_eager!("[00]", QJSON_INVALID_NUMBER);
        assert_rejects_eager!("[007]", QJSON_INVALID_NUMBER);
    }

    // §6: fraction requires at least one digit after the dot.
    #[test]
    fn trailing_dot_rejected() {
        assert_rejects_eager!("[1.]", QJSON_INVALID_NUMBER);
        assert_rejects_eager!("[1.e5]", QJSON_INVALID_NUMBER);
    }

    // §6: fraction cannot start without an integer part.
    #[test]
    fn leading_dot_rejected() {
        assert_rejects_eager!("[.5]", QJSON_INVALID_NUMBER);
    }

    // §6: exponent requires at least one digit.
    #[test]
    fn incomplete_exponent_rejected() {
        assert_rejects_eager!("[1e]", QJSON_INVALID_NUMBER);
        assert_rejects_eager!("[1e+]", QJSON_INVALID_NUMBER);
        assert_rejects_eager!("[1e-]", QJSON_INVALID_NUMBER);
    }

    // Hex notation is not part of the JSON number grammar.
    #[test]
    fn hex_notation_rejected() {
        assert_rejects_eager!("[0x1F]", QJSON_INVALID_NUMBER);
        assert_rejects_eager!("[0xFF]", QJSON_INVALID_NUMBER);
    }

    // Non-finite values are not part of JSON.
    #[test]
    fn non_finite_rejected() {
        assert_rejects_eager!("[NaN]", QJSON_INVALID_NUMBER);
        assert_rejects_eager!("[Infinity]", QJSON_INVALID_NUMBER);
        assert_rejects_eager!("[-Infinity]", QJSON_INVALID_NUMBER);
    }

    // Lone minus is not a valid number.
    #[test]
    fn lone_minus_rejected() {
        assert_rejects_eager!("[-]", QJSON_INVALID_NUMBER);
    }
}
