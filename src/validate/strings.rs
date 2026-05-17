//! String-content validation: control chars and UTF-8.

use crate::error::qjd_err;

/// Verify that the raw span (excluding surrounding quotes) contains no
/// unescaped control characters (0x00..=0x1F) and is valid UTF-8.
pub(crate) fn validate_string_span(span: &[u8]) -> Result<(), qjd_err> {
    // Control chars are forbidden inside a JSON string per RFC 8259 §7.
    // Cheap pass first: bytewise check.
    if span.iter().any(|&b| b < 0x20) {
        return Err(qjd_err::QJD_INVALID_STRING);
    }
    // UTF-8 validation. Backslash escapes are not yet expanded; the byte
    // immediately after `\` may legally be any escape introducer
    // (`"`, `\`, `/`, `b`, `f`, `n`, `r`, `t`, `u`), all of which are ASCII.
    // So validating the raw span (with backslashes still in place) gives
    // the same answer as validating the escape-decoded result.
    if std::str::from_utf8(span).is_err() {
        return Err(qjd_err::QJD_INVALID_UTF8);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn ascii_ok()         { assert!(validate_string_span(b"hello").is_ok()); }
    #[test] fn utf8_ok()          { assert!(validate_string_span("中文".as_bytes()).is_ok()); }
    #[test] fn escapes_ok()       { assert!(validate_string_span(b"a\\nb\\u00e9").is_ok()); }
    #[test] fn tab_raw_bad()      { assert_eq!(validate_string_span(b"a\tb").unwrap_err(), qjd_err::QJD_INVALID_STRING); }
    #[test] fn null_raw_bad()     { assert_eq!(validate_string_span(b"a\x00b").unwrap_err(), qjd_err::QJD_INVALID_STRING); }
    #[test] fn newline_raw_bad()  { assert_eq!(validate_string_span(b"a\nb").unwrap_err(), qjd_err::QJD_INVALID_STRING); }
    #[test] fn del_0x7f_ok()      { assert!(validate_string_span(b"a\x7fb").is_ok()); } // RFC 8259 does NOT forbid 0x7F
    #[test] fn invalid_utf8_bad() { assert_eq!(validate_string_span(&[0xC0, 0xC0]).unwrap_err(), qjd_err::QJD_INVALID_UTF8); }
}
