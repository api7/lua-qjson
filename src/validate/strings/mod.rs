//! String-content validation: control chars, escape grammar, and UTF-8.
//!
//! Single-pass validator with an optional SIMD ASCII fast path. The public
//! entry point [`validate_string_span`] dispatches once via `OnceCell` to
//! the best available implementation:
//!
//!   - x86_64 + AVX2: 32-byte chunk skip → scalar tail.
//!   - aarch64 NEON:  16-byte chunk skip → scalar tail.
//!   - Otherwise:     pure scalar state machine.
//!
//! All paths return identical error codes for any input; the SIMD layers
//! only accelerate the "this chunk is pure printable ASCII" common case.

mod scalar;
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
mod avx2;
#[cfg(target_arch = "aarch64")]
mod neon;

use crate::error::qjd_err;
use once_cell::sync::OnceCell;

type ValidateFn = fn(&[u8]) -> Result<(), qjd_err>;
static VALIDATE_FN: OnceCell<ValidateFn> = OnceCell::new();

/// Verify that the raw span (excluding surrounding quotes) contains no
/// unescaped control characters (0x00..=0x1F), every backslash escape is
/// RFC 8259 §7 compliant, and the byte sequence is valid UTF-8 per RFC 3629.
pub(crate) fn validate_string_span(span: &[u8]) -> Result<(), qjd_err> {
    let f = *VALIDATE_FN.get_or_init(|| {
        #[cfg(all(target_arch = "x86_64", feature = "avx2"))]
        {
            if std::is_x86_feature_detected!("avx2") {
                return avx2::validate_span_avx2 as ValidateFn;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            return neon::validate_span_neon as ValidateFn;
        }
        #[allow(unreachable_code)]
        {
            scalar::validate_span_scalar as ValidateFn
        }
    });
    f(span)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Pinned baseline contract (DO NOT MODIFY) ─────────────────────────
    // These 8 tests reproduce the original 3-pass validator's externally
    // observable behavior and pin it down. The single-pass refactor must
    // not change any of these outcomes.

    #[test] fn ascii_ok()         { assert!(validate_string_span(b"hello").is_ok()); }
    #[test] fn utf8_ok()          { assert!(validate_string_span("中文".as_bytes()).is_ok()); }
    #[test] fn escapes_ok()       { assert!(validate_string_span(b"a\\nb\\u00e9").is_ok()); }
    #[test] fn tab_raw_bad()      { assert_eq!(validate_string_span(b"a\tb").unwrap_err(), qjd_err::QJD_INVALID_STRING); }
    #[test] fn null_raw_bad()     { assert_eq!(validate_string_span(b"a\x00b").unwrap_err(), qjd_err::QJD_INVALID_STRING); }
    #[test] fn newline_raw_bad()  { assert_eq!(validate_string_span(b"a\nb").unwrap_err(), qjd_err::QJD_INVALID_STRING); }
    #[test] fn del_0x7f_ok()      { assert!(validate_string_span(b"a\x7fb").is_ok()); } // RFC 8259 does NOT forbid 0x7F
    #[test] fn invalid_utf8_bad() { assert_eq!(validate_string_span(&[0xC0, 0xC0]).unwrap_err(), qjd_err::QJD_INVALID_UTF8); }

    // ── Single-pass / SIMD edge cases ────────────────────────────────────

    #[test]
    fn empty_span_ok() {
        assert!(validate_string_span(b"").is_ok());
    }

    #[test]
    fn long_ascii_ok() {
        // > 64 bytes hits the SIMD fast path multiple times.
        let s = vec![b'x'; 256];
        assert!(validate_string_span(&s).is_ok());
    }

    #[test]
    fn long_ascii_with_trailing_tab_bad() {
        // Long ASCII run skipped by SIMD, then a control byte in the tail.
        let mut s = vec![b'x'; 200];
        s.push(b'\t');
        assert_eq!(validate_string_span(&s).unwrap_err(), qjd_err::QJD_INVALID_STRING);
    }

    #[test]
    fn utf8_at_simd_chunk_boundary() {
        // 31 ASCII bytes + 2-byte UTF-8 (é = 0xC3 0xA9). On AVX2 the first
        // 32-byte chunk has a high-bit byte at lane 31 → forces scalar tail
        // starting at position 31, which must handle the 2-byte sequence.
        let mut s = vec![b'x'; 31];
        s.extend_from_slice("é".as_bytes());
        assert!(validate_string_span(&s).is_ok());
    }

    #[test]
    fn backslash_escape_at_simd_chunk_boundary() {
        // 31 ASCII + `\n` straddles AVX2 chunk boundary at byte 31.
        let mut s = vec![b'x'; 31];
        s.push(b'\\');
        s.push(b'n');
        assert!(validate_string_span(&s).is_ok());
    }

    #[test]
    fn backslash_at_chunk_boundary_with_bad_followup() {
        // Backslash lands as the last byte of a 32-byte chunk; the next byte
        // is an invalid escape introducer. Tail must reject.
        let mut s = vec![b'x'; 31];
        s.push(b'\\');
        s.push(b'q');
        assert_eq!(validate_string_span(&s).unwrap_err(), qjd_err::QJD_INVALID_STRING);
    }

    #[test]
    fn truncated_u_escape_at_end() {
        // `\uXX` with only 2 hex digits — RFC requires exactly 4.
        assert_eq!(validate_string_span(b"\\uAB").unwrap_err(), qjd_err::QJD_INVALID_STRING);
        assert_eq!(validate_string_span(b"\\uABC").unwrap_err(), qjd_err::QJD_INVALID_STRING);
        // Bare `\u` at end.
        assert_eq!(validate_string_span(b"\\u").unwrap_err(), qjd_err::QJD_INVALID_STRING);
    }

    #[test]
    fn dangling_backslash_at_end() {
        assert_eq!(validate_string_span(b"abc\\").unwrap_err(), qjd_err::QJD_INVALID_STRING);
    }

    #[test]
    fn unknown_escape_introducer() {
        // `\a`, `\q`, etc. are not valid RFC 8259 escapes.
        assert_eq!(validate_string_span(b"\\a").unwrap_err(), qjd_err::QJD_INVALID_STRING);
        assert_eq!(validate_string_span(b"\\q").unwrap_err(), qjd_err::QJD_INVALID_STRING);
        assert_eq!(validate_string_span(b"\\x41").unwrap_err(), qjd_err::QJD_INVALID_STRING);
    }

    #[test]
    fn u_escape_non_hex_bad() {
        assert_eq!(validate_string_span(b"\\u00ZZ").unwrap_err(), qjd_err::QJD_INVALID_STRING);
        assert_eq!(validate_string_span(b"\\uGHIJ").unwrap_err(), qjd_err::QJD_INVALID_STRING);
    }

    #[test]
    fn overlong_utf8_rejected() {
        // C0 80 would encode U+0000 in 2 bytes (overlong) — RFC 3629 forbids.
        assert_eq!(validate_string_span(&[0xC0, 0x80]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
        // E0 80 80 would encode U+0000 in 3 bytes (overlong).
        assert_eq!(validate_string_span(&[0xE0, 0x80, 0x80]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
        // F0 80 80 80 would encode U+0000 in 4 bytes (overlong).
        assert_eq!(validate_string_span(&[0xF0, 0x80, 0x80, 0x80]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
    }

    #[test]
    fn surrogate_in_utf8_rejected() {
        // ED A0 80 = U+D800, the start of the high-surrogate range.
        assert_eq!(validate_string_span(&[0xED, 0xA0, 0x80]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
        // ED BF BF = U+DFFF, the end of the low-surrogate range.
        assert_eq!(validate_string_span(&[0xED, 0xBF, 0xBF]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
    }

    #[test]
    fn lone_continuation_byte_rejected() {
        assert_eq!(validate_string_span(&[0x80]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
        assert_eq!(validate_string_span(&[b'a', 0xBF, b'b']).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
    }

    #[test]
    fn four_byte_emoji_ok() {
        // U+1F600 grinning face = F0 9F 98 80.
        assert!(validate_string_span(&[0xF0, 0x9F, 0x98, 0x80]).is_ok());
    }

    #[test]
    fn truncated_utf8_sequence_rejected() {
        // 2-byte lead with no continuation.
        assert_eq!(validate_string_span(&[0xC3]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
        // 3-byte lead with only one continuation.
        assert_eq!(validate_string_span(&[0xE4, 0xB8]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
        // 4-byte lead with only two continuations.
        assert_eq!(validate_string_span(&[0xF0, 0x9F, 0x98]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
    }

    #[test]
    fn utf8_out_of_range_rejected() {
        // F5..FF are not valid lead bytes (would encode > U+10FFFF).
        assert_eq!(validate_string_span(&[0xF5, 0x80, 0x80, 0x80]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
        assert_eq!(validate_string_span(&[0xFF]).unwrap_err(), qjd_err::QJD_INVALID_UTF8);
    }
}
