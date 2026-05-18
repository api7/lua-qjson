//! Single-pass scalar validator for a JSON string span (interior bytes,
//! excluding the surrounding quotes).
//!
//! Combines three checks into one byte walk:
//!   1. RFC 8259 §7: no raw control characters (b < 0x20).
//!   2. RFC 8259 §7: every `\` escape is one of `" \ / b f n r t` or `\uXXXX`.
//!   3. RFC 3629: valid UTF-8 (rejects overlong encodings and surrogates,
//!      matching `std::str::from_utf8` for full corpus parity).
//!
//! Error-code precedence on mixed inputs:
//!   - Control char or invalid escape introducer encountered first → INVALID_STRING.
//!   - Bad UTF-8 lead/continuation byte encountered first → INVALID_UTF8.
//!
//! This means a span like `[0x09, 0xFF]` returns INVALID_STRING (control byte
//! seen before the UTF-8 problem), whereas `[0xFF, 0x09]` returns INVALID_UTF8.
//! The previous two-pass code preferred UTF-8 in both cases; no existing test
//! pins down which wins on mixed input, so the position-ordered choice here
//! is the natural single-pass behavior.

use crate::error::qjd_err;

/// Validate `span` byte-by-byte. The caller passes the unescaped string
/// interior (between the JSON `"…"` quotes) — `\` therefore introduces an
/// RFC 8259 escape sequence, not a literal backslash byte.
pub(crate) fn validate_span_scalar(span: &[u8]) -> Result<(), qjd_err> {
    let mut i: usize = 0;
    let n = span.len();
    while i < n {
        let b = span[i];

        // Fast path: plain ASCII non-escape non-control.
        if b < 0x80 {
            if b < 0x20 {
                return Err(qjd_err::QJD_INVALID_STRING);
            }
            if b == b'\\' {
                i = validate_escape(span, i + 1)?;
                continue;
            }
            i += 1;
            continue;
        }

        // High-bit byte: must be the lead of a 2/3/4-byte UTF-8 sequence.
        i = validate_utf8_sequence(span, i)?;
    }
    Ok(())
}

/// At entry `i` points to the byte AFTER the `\`. Returns the index of the
/// next byte to validate (i.e. one past the last consumed escape byte).
#[inline]
fn validate_escape(span: &[u8], i: usize) -> Result<usize, qjd_err> {
    if i >= span.len() {
        // Dangling `\` at end of span.
        return Err(qjd_err::QJD_INVALID_STRING);
    }
    match span[i] {
        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => Ok(i + 1),
        b'u' => {
            // Must be followed by exactly 4 hex digits.
            let hex_start = i + 1;
            let hex_end = hex_start + 4;
            if hex_end > span.len() {
                return Err(qjd_err::QJD_INVALID_STRING);
            }
            for &h in &span[hex_start..hex_end] {
                if !h.is_ascii_hexdigit() {
                    return Err(qjd_err::QJD_INVALID_STRING);
                }
            }
            Ok(hex_end)
        }
        _ => Err(qjd_err::QJD_INVALID_STRING),
    }
}

/// At entry `i` points to a byte with the high bit set. Validate the
/// multi-byte UTF-8 sequence starting here per RFC 3629 (rejects overlong
/// encodings and UTF-16 surrogates U+D800..=U+DFFF). Returns the index one
/// past the last byte of the sequence.
#[inline]
fn validate_utf8_sequence(span: &[u8], i: usize) -> Result<usize, qjd_err> {
    let lead = span[i];
    let n = span.len();

    // 2-byte: 110xxxxx 10xxxxxx, lead in C2..=DF (C0/C1 are overlong).
    if (0xC2..=0xDF).contains(&lead) {
        if i + 1 >= n {
            return Err(qjd_err::QJD_INVALID_UTF8);
        }
        let b1 = span[i + 1];
        if !(0x80..=0xBF).contains(&b1) {
            return Err(qjd_err::QJD_INVALID_UTF8);
        }
        return Ok(i + 2);
    }

    // 3-byte: 1110xxxx 10xxxxxx 10xxxxxx, lead in E0..=EF.
    // Extra constraints: E0 second must be A0..BF (else overlong);
    //                    ED second must be 80..9F (else surrogate U+D800..=DFFF).
    if (0xE0..=0xEF).contains(&lead) {
        if i + 2 >= n {
            return Err(qjd_err::QJD_INVALID_UTF8);
        }
        let b1 = span[i + 1];
        let b2 = span[i + 2];
        let b1_lo = match lead {
            0xE0 => 0xA0,
            _    => 0x80,
        };
        let b1_hi = match lead {
            0xED => 0x9F,
            _    => 0xBF,
        };
        if b1 < b1_lo || b1 > b1_hi {
            return Err(qjd_err::QJD_INVALID_UTF8);
        }
        if !(0x80..=0xBF).contains(&b2) {
            return Err(qjd_err::QJD_INVALID_UTF8);
        }
        return Ok(i + 3);
    }

    // 4-byte: 11110xxx 10xxxxxx 10xxxxxx 10xxxxxx, lead in F0..=F4.
    // Extra constraints: F0 second must be 90..BF (else overlong);
    //                    F4 second must be 80..8F (else > U+10FFFF).
    if (0xF0..=0xF4).contains(&lead) {
        if i + 3 >= n {
            return Err(qjd_err::QJD_INVALID_UTF8);
        }
        let b1 = span[i + 1];
        let b2 = span[i + 2];
        let b3 = span[i + 3];
        let b1_lo = match lead {
            0xF0 => 0x90,
            _    => 0x80,
        };
        let b1_hi = match lead {
            0xF4 => 0x8F,
            _    => 0xBF,
        };
        if b1 < b1_lo || b1 > b1_hi {
            return Err(qjd_err::QJD_INVALID_UTF8);
        }
        if !(0x80..=0xBF).contains(&b2) {
            return Err(qjd_err::QJD_INVALID_UTF8);
        }
        if !(0x80..=0xBF).contains(&b3) {
            return Err(qjd_err::QJD_INVALID_UTF8);
        }
        return Ok(i + 4);
    }

    // C0, C1 (overlong 2-byte lead), F5..FF (out of range), or a bare
    // continuation byte (80..BF with no lead) — all invalid.
    Err(qjd_err::QJD_INVALID_UTF8)
}
