#![cfg(target_arch = "aarch64")]

//! NEON ASCII fast path for string-content validation.
//!
//! For each 16-byte chunk, compute a single "needs-attention" mask covering
//! bytes that are control chars (< 0x20), backslashes, or high-bit bytes.
//! If the chunk is pure printable ASCII the mask is all-zero and the chunk
//! can be skipped entirely. The first non-zero chunk hands off to the
//! scalar state machine, which handles correctness for the remainder.

use crate::error::qjd_err;
use core::arch::aarch64::*;

use super::scalar::validate_span_scalar;

/// Validate `span` using NEON to bulk-skip pure-ASCII 16-byte chunks.
pub(crate) fn validate_span_neon(span: &[u8]) -> Result<(), qjd_err> {
    // SAFETY: aarch64 NEON is always available on aarch64 (it is part of
    // the AArch64 base ISA), so no runtime feature check is required.
    unsafe { validate_span_neon_impl(span) }
}

#[target_feature(enable = "neon")]
unsafe fn validate_span_neon_impl(span: &[u8]) -> Result<(), qjd_err> {
    let mut i: usize = 0;
    let n = span.len();

    let backslash = vdupq_n_u8(b'\\');
    let ctrl_top  = vdupq_n_u8(0x20);

    while i + 16 <= n {
        let chunk = vld1q_u8(span.as_ptr().add(i));

        // byte >= 0x80 ?  high bit set
        let high = vcgeq_u8(chunk, vdupq_n_u8(0x80));
        // byte == '\\' ?
        let bs   = vceqq_u8(chunk, backslash);
        // byte <  0x20 ?
        let ctrl = vcltq_u8(chunk, ctrl_top);

        let interesting = vorrq_u8(vorrq_u8(high, bs), ctrl);

        // Reduce 16 lanes → single u64 to test for any non-zero byte.
        // vmaxvq_u8 returns 0 iff every lane is 0.
        if vmaxvq_u8(interesting) != 0 {
            // First interesting byte: find via lane index.
            // Build 0xFF/0x00 per-lane mask already in `interesting`; convert
            // each lane to its index-or-MAX via a small scalar loop. A 16-lane
            // ctz would be tidier but isn't critical here — interesting chunks
            // are the slow case anyway.
            for lane in 0..16usize {
                if span[i + lane] >= 0x80
                    || span[i + lane] == b'\\'
                    || span[i + lane] < 0x20
                {
                    return validate_span_scalar(&span[i + lane..]);
                }
            }
            // Unreachable: vmaxvq_u8 said at least one lane is non-zero.
            unreachable!();
        }

        i += 16;
    }

    validate_span_scalar(&span[i..])
}
