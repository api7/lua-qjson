//! String-content validation: control chars and UTF-8.

use crate::error::qjd_err;

/// Verify that the raw span (excluding surrounding quotes) contains no
/// unescaped control characters (0x00..=0x1F), is valid UTF-8, and that
/// every backslash escape sequence is RFC 8259 §7 compliant.
pub(crate) fn validate_string_span(span: &[u8]) -> Result<(), qjd_err> {
    // UTF-8 validation first (includes multi-byte content validation).
    // Backslash escapes are ASCII, so validating the unexpanded span gives
    // the correct answer for the UTF-8 structure of non-escape bytes.
    if std::str::from_utf8(span).is_err() {
        return Err(qjd_err::QJD_INVALID_UTF8);
    }

    // Walk the span validating control chars and escape sequences.
    let mut i = 0;
    while i < span.len() {
        let b = span[i];
        // RFC 8259 §7: control characters must be escaped.
        if b < 0x20 {
            return Err(qjd_err::QJD_INVALID_STRING);
        }
        if b == b'\\' {
            i += 1;
            if i >= span.len() {
                return Err(qjd_err::QJD_INVALID_STRING);
            }
            match span[i] {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                b'u' => {
                    // Must be followed by exactly 4 hex digits.
                    if i + 4 >= span.len() {
                        return Err(qjd_err::QJD_INVALID_STRING);
                    }
                    for &h in &span[i + 1..=i + 4] {
                        if !h.is_ascii_hexdigit() {
                            return Err(qjd_err::QJD_INVALID_STRING);
                        }
                    }
                    i += 4; // consumed 4 hex digits; loop adds 1 more
                }
                _ => return Err(qjd_err::QJD_INVALID_STRING),
            }
        }
        i += 1;
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
