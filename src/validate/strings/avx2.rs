#![cfg(all(target_arch = "x86_64", feature = "avx2"))]

//! AVX2 ASCII fast path for string-content validation.
//!
//! For each 32-byte chunk, compute a "needs-attention" mask covering bytes
//! that are either control chars (< 0x20), backslashes, or high-bit bytes.
//! If the mask is all-zero the chunk is pure printable ASCII (no escapes,
//! no UTF-8, no control) and can be skipped entirely.
//!
//! On the first non-zero chunk we hand off to the scalar state machine for
//! the remainder of the span — we don't try to bit-scan inside the chunk.
//! The fast-path payoff comes from cleanly skipping long ASCII prefixes;
//! the scalar tail handles correctness without needing SIMD escape logic.

use crate::error::qjson_err;
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

    // ASCII bytes that need scalar attention have:
    //   - top bit set                  → byte >= 0x80
    //   - value < 0x20                 → control char
    //   - value == 0x5C ('\\')         → escape introducer
    //
    // Detection via three SIMD compares OR'd together.
    let backslash = _mm256_set1_epi8(b'\\' as i8);
    // For "< 0x20" we use a signed unsigned trick: compare against 0x1F via
    // unsigned MAX. _mm256_cmpgt_epi8 is signed, but bytes <0x20 are also
    // <0x20 as signed positive values, so signed cmpgt works here for the
    // 0x00..=0x1F range (none of which has the high bit set).
    let ctrl_thresh = _mm256_set1_epi8(0x20_i8);

    while i + 32 <= n {
        let chunk = _mm256_loadu_si256(span.as_ptr().add(i) as *const __m256i);

        // high bit set?
        let high  = _mm256_movemask_epi8(chunk) as u32;
        // byte == '\\' ?
        let bs    = _mm256_movemask_epi8(_mm256_cmpeq_epi8(chunk, backslash)) as u32;
        // byte < 0x20 ?  (signed cmpgt: ctrl_thresh > chunk for 0x00..=0x1F bytes)
        let ctrl  = _mm256_movemask_epi8(_mm256_cmpgt_epi8(ctrl_thresh, chunk)) as u32;

        let interesting = high | bs | ctrl;
        if interesting != 0 {
            // Hand off to the scalar state machine starting at the first
            // interesting byte in this chunk. We don't try to validate any
            // already-cleared bytes — those are pure printable ASCII and
            // self-terminating so it's safe to resume there.
            let offset = interesting.trailing_zeros() as usize;
            return validate_span_scalar(&span[i + offset..]);
        }

        i += 32;
    }

    validate_span_scalar(&span[i..])
}
