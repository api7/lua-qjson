#![cfg(all(target_arch = "x86_64", feature = "avx2"))]

//! AVX2 string-content validation using the PSHUFB nibble-LUT classifier.
//!
//! `classify_str_mask` classifies all 32 bytes in a chunk simultaneously
//! via a 32-byte look-up table queried by `_mm256_shuffle_epi8` (PSHUFB).
//! The LUT produces a byte-class bitmask for each input byte: pure
//! printable ASCII returns zero, while control chars, backslashes, and
//! high-bit bytes set bits that fold into a single `u32` attention mask.
//!
//! Zero-mask chunks are skipped entirely. For non-zero chunks we iterate
//! the set bits and validate each flagged byte in-batch:
//!   - control → INVALID_STRING
//!   - backslash → validate the escape introducer + following byte(s)
//!   - high-bit  → delegate the remainder to the well-tested scalar path
//!
//! Single-char escapes and `\uXXXX` that fit within the current 32-byte
//! chunk are validated inline; escapes straddling a chunk boundary fall
//! through to the scalar path for correctness.

use crate::error::qjson_err;
use crate::validate::classify::classify_str_mask;
use core::arch::x86_64::*;

use super::scalar::validate_span_scalar;

/// Validate `span` using AVX2 to bulk-skip pure-ASCII 32-byte chunks.
pub(crate) fn validate_span_avx2(span: &[u8]) -> Result<(), qjson_err> {
    // SAFETY: dispatcher has verified the AVX2 feature is present.
    unsafe { validate_span_avx2_impl(span) }
}

#[target_feature(enable = "avx2")]
unsafe fn validate_span_avx2_impl(span: &[u8]) -> Result<(), qjson_err> {
    let mut i: usize = 0;
    let n = span.len();

    while i + 32 <= n {
        let chunk = _mm256_loadu_si256(span.as_ptr().add(i) as *const __m256i);
        let mask = classify_str_mask(chunk);

        if mask != 0 {
            let mut m = mask;
            let mut consumed: usize = 0; // bytes from chunk start already handled
            while m != 0 {
                let offset = m.trailing_zeros() as usize;
                m &= m - 1;

                if offset < consumed {
                    continue; // already consumed as part of a prior escape
                }

                let pos = i + offset;
                let b = span[pos];

                if b < 0x20 {
                    return Err(qjson_err::QJSON_INVALID_STRING);
                }

                if b >= 0x80 {
                    return validate_span_scalar(&span[pos..]);
                }

                // b == b'\\' (mask only has bits for ctrl|bs|high)
                if pos + 1 >= n {
                    return Err(qjson_err::QJSON_INVALID_STRING);
                }

                let next = span[pos + 1];
                match next {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {
                        // Escape straddles chunk boundary: delegate to scalar
                        // so consumed tracking doesn't lose sync.
                        if pos + 2 > i + 32 {
                            return validate_span_scalar(&span[pos..]);
                        }
                        consumed = offset + 2;
                    }
                    b'u' => {
                        let hex_start = pos + 2;
                        let hex_end = hex_start + 4;
                        if hex_end > n {
                            return Err(qjson_err::QJSON_INVALID_STRING);
                        }
                        // If the full \uXXXX straddles the chunk boundary,
                        // hand off to scalar.
                        if hex_end > i + 32 {
                            return validate_span_scalar(&span[pos..]);
                        }
                        for &h in &span[hex_start..hex_end] {
                            if !h.is_ascii_hexdigit() {
                                return Err(qjson_err::QJSON_INVALID_STRING);
                            }
                        }
                        consumed = offset + 6;
                    }
                    _ => return Err(qjson_err::QJSON_INVALID_STRING),
                }
            }
        }

        i += 32;
    }

    validate_span_scalar(&span[i..])
}
