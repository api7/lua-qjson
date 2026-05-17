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

use quickdecode::doc::Document;
use quickdecode::options::{Options, QJD_MODE_EAGER, QJD_MODE_LAZY};

fn eager() -> Options { Options { mode: QJD_MODE_EAGER, max_depth: 0 } }
fn lazy()  -> Options { Options { mode: QJD_MODE_LAZY,  max_depth: 0 } }

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
/// Usage: `assert_rejects_eager!("01", QJD_INVALID_NUMBER);`
#[macro_export]
macro_rules! assert_rejects_eager {
    ($input:expr, $expected_err:ident) => {{
        use quickdecode::error::qjd_err;
        let buf: &[u8] = $input.as_ref();
        let expected = qjd_err::$expected_err;
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
#[should_panic(expected = "expected QJD_INVALID_NUMBER")]
fn macro_rejects_wrong_error_code() {
    // Sanity: passing the wrong expected variant must panic.
    // `{` is rejected as QJD_PARSE_ERROR, NOT QJD_INVALID_NUMBER.
    // With the buggy macro, this test would NOT panic (false positive
    // — the macro would silently bind whatever Err came back).
    assert_rejects_eager!("{", QJD_INVALID_NUMBER);
}

// ── Phase 3: nesting depth ───────────────────────────────────

#[test]
fn rejects_deeply_nested_at_default_limit() {
    use quickdecode::error::qjd_err;
    let mut buf = String::new();
    for _ in 0..1100 { buf.push('['); }
    for _ in 0..1100 { buf.push(']'); }
    match Document::parse_with_options(buf.as_bytes(), &eager()) {
        Err(qjd_err::QJD_NESTING_TOO_DEEP) => {}
        other => panic!("expected QJD_NESTING_TOO_DEEP, got {:?}", other.err()),
    }
}

#[test]
fn lazy_mode_also_enforces_max_depth() {
    use quickdecode::error::qjd_err;
    let mut buf = String::new();
    for _ in 0..1100 { buf.push('['); }
    for _ in 0..1100 { buf.push(']'); }
    assert_eq!(
        Document::parse_with_options(buf.as_bytes(), &lazy()).err().unwrap(),
        qjd_err::QJD_NESTING_TOO_DEEP,
    );
}

#[test]
fn accepts_nested_at_configured_limit() {
    let mut buf = String::new();
    for _ in 0..256 { buf.push('['); }
    for _ in 0..256 { buf.push(']'); }
    let opts = Options { mode: QJD_MODE_EAGER, max_depth: 256 };
    assert!(Document::parse_with_options(buf.as_bytes(), &opts).is_ok());
}

#[test]
fn rejects_when_one_past_configured_limit() {
    let mut buf = String::new();
    for _ in 0..33 { buf.push('['); }
    for _ in 0..33 { buf.push(']'); }
    let opts = Options { mode: QJD_MODE_EAGER, max_depth: 32 };
    assert!(Document::parse_with_options(buf.as_bytes(), &opts).is_err());
}

// ── Phase 6: trailing content ────────────────────────────────

#[test]
fn eager_rejects_trailing_content() {
    use quickdecode::error::qjd_err;
    assert_eq!(
        Document::parse_with_options(b"{}garbage", &eager()).err().unwrap(),
        qjd_err::QJD_TRAILING_CONTENT,
    );
}

#[test]
fn eager_rejects_multiple_root_values() {
    use quickdecode::error::qjd_err;
    assert_eq!(
        Document::parse_with_options(b"1 2", &eager()).err().unwrap(),
        qjd_err::QJD_TRAILING_CONTENT,
    );
    assert_eq!(
        Document::parse_with_options(b"true false", &eager()).err().unwrap(),
        qjd_err::QJD_TRAILING_CONTENT,
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
