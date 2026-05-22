#![cfg(all(target_arch = "x86_64", feature = "avx2"))]

//! AVX2 three-tier string-content validator.
//!
//! Dispatch:
//!   Tier 1 – pure printable ASCII chunk (high | ctrl | bs == 0, prev ended ASCII-safe):
//!            skip wholesale, no SIMD validation needed.
//!   Tier 2 – pure UTF-8 chunk (high != 0, ctrl | bs == 0):
//!            run lookup4 AVX2 validator; continue SIMD loop.
//!   Tier 3 – control byte or backslash present:
//!            finalize lookup4 carry, then hand off to scalar state machine
//!            from the first interesting byte.
//!
//! Cross-chunk UTF-8 carry state:
//!   `prev_input` (256-bit) is the last processed chunk. It is shifted by 1, 2,
//!   and 3 byte positions into `prev1`, `prev2`, `prev3` inside `lookup4_chunk`.
//!   `err_acc` ORs per-chunk errors; checked at Tier 3 handoff and end of loop.
//!   `prev_ended_ascii` gates Tier 1: a chunk whose last byte has the high bit
//!   set cannot be followed by a Tier 1 skip — the interlock forces Tier 2/3
//!   so the cross-chunk multi-byte carry is validated.
//!
//! The lookup4 algorithm is transcribed from simdjson's
//! utf8_lookup4_algorithm.h (Lemire & Keiser, 2020).

use crate::error::qjson_err;
use core::arch::x86_64::*;

// ── lookup4 UTF-8 validator constants (from simdjson) ───────────────────────
//
// Three 16-byte lookup tables encode UTF-8 byte-pair validity.  Each byte
// of a chunk is classified by the table lookup; the result of ANDing
// byte_1_high & byte_1_low & byte_2_high yields the per-byte error flags
// for check_special_cases.  Combined with the check_multibyte_lengths
// result via XOR, any non-zero byte ⇒ invalid UTF-8.
//
// DO NOT re-derive — values transcribed verbatim from simdjson source.

const TOO_SHORT:      u8 = 1 << 0; // lead byte followed by lead/ASCII
const TOO_LONG:       u8 = 1 << 1; // ASCII followed by continuation
const OVERLONG_3:     u8 = 1 << 2; // E0 80..9F
const SURROGATE:      u8 = 1 << 4; // ED A0..BF
const OVERLONG_2:     u8 = 1 << 5; // C0/C1 80..BF
const TWO_CONTS:      u8 = 1 << 7; // 10______ 10______
const TOO_LARGE:      u8 = 1 << 3; // > U+10FFFF
const TOO_LARGE_1000: u8 = 1 << 6;
const OVERLONG_4:     u8 = 1 << 6; // F0 80..8F
const CARRY:          u8 = TOO_SHORT | TOO_LONG | TWO_CONTS;

/// High nibble of prev1 (byte i−1): `byte_1_high` in simdjson terminology.
#[rustfmt::skip]
const BYTE1_HIGH: [u8; 16] = [
    // 0_______ = ASCII in byte 1
    TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG,
    TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG,
    // 10______ = continuation in byte 1
    TWO_CONTS, TWO_CONTS, TWO_CONTS, TWO_CONTS,
    // 1100____ = 2-byte lead (incl. C0/C1 → OVERLONG_2)
    TOO_SHORT | OVERLONG_2,
    // 1101____ = 2-byte lead
    TOO_SHORT,
    // 1110____ = 3-byte lead (incl. E0 → OVERLONG_3, ED → SURROGATE)
    TOO_SHORT | OVERLONG_3 | SURROGATE,
    // 1111____ = 4-byte lead (incl. F0 → OVERLONG_4, F4+ → TOO_LARGE)
    TOO_SHORT | TOO_LARGE | TOO_LARGE_1000 | OVERLONG_4,
];

/// Low nibble of prev1: `byte_1_low` in simdjson terminology.
#[rustfmt::skip]
const BYTE1_LOW: [u8; 16] = [
    // ____0000
    CARRY | OVERLONG_3 | OVERLONG_2 | OVERLONG_4,
    // ____0001
    CARRY | OVERLONG_2,
    // ____0010, ____0011
    CARRY,
    CARRY,
    // ____0100
    CARRY | TOO_LARGE,
    // ____0101
    CARRY | TOO_LARGE | TOO_LARGE_1000,
    // ____011_
    CARRY | TOO_LARGE | TOO_LARGE_1000,
    CARRY | TOO_LARGE | TOO_LARGE_1000,
    // ____1___
    CARRY | TOO_LARGE | TOO_LARGE_1000,
    CARRY | TOO_LARGE | TOO_LARGE_1000,
    CARRY | TOO_LARGE | TOO_LARGE_1000,
    CARRY | TOO_LARGE | TOO_LARGE_1000,
    CARRY | TOO_LARGE | TOO_LARGE_1000,
    // ____1101
    CARRY | TOO_LARGE | TOO_LARGE_1000 | SURROGATE,
    CARRY | TOO_LARGE | TOO_LARGE_1000,
    CARRY | TOO_LARGE | TOO_LARGE_1000,
];

/// High nibble of current byte (byte i): `byte_2_high` in simdjson terminology.
///
/// This is the lookup for the *current* byte (not the prev1 byte).
/// - ASCII bytes (nibbles 0-7): `TOO_SHORT`
/// - Continuation 0x80-0x8F (nibble 8): `TOO_LONG | OVERLONG_2 | TWO_CONTS | OVERLONG_3 | TOO_LARGE_1000 | OVERLONG_4`
/// - Continuation 0x90-0x9F (nibble 9): `TOO_LONG | OVERLONG_2 | TWO_CONTS | OVERLONG_3 | TOO_LARGE`
/// - Continuation 0xA0-0xBF (nibbles A-B): `TOO_LONG | OVERLONG_2 | TWO_CONTS | SURROGATE | TOO_LARGE`
/// - Lead bytes 0xC0-0xFF (nibbles C-F): `TOO_SHORT`
#[rustfmt::skip]
const BYTE2_HIGH: [u8; 16] = [
    // ________ 0_______ = ASCII in byte 2
    TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT,
    TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT,
    // ________ 1000____ = continuation in byte 2
    TOO_LONG | OVERLONG_2 | TWO_CONTS | OVERLONG_3 | TOO_LARGE_1000 | OVERLONG_4,
    // ________ 1001____
    TOO_LONG | OVERLONG_2 | TWO_CONTS | OVERLONG_3 | TOO_LARGE,
    // ________ 101_____
    TOO_LONG | OVERLONG_2 | TWO_CONTS | SURROGATE   | TOO_LARGE,
    TOO_LONG | OVERLONG_2 | TWO_CONTS | SURROGATE   | TOO_LARGE,
    // ________ 11______ = lead byte in byte 2
    TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT,
];

/// Compute the lookup4 error mask for a 32-byte chunk.
///
/// Implements `check_special_cases` XOR `check_multibyte_lengths & 0x80`
/// from simdjson, adapted for AVX2 (256-bit = 2 × 128-bit lanes).
///
/// `prev_input` is updated to the current chunk on return (carry for next call).
/// Returns a 256-bit mask: any non-zero byte ⇒ UTF-8 error at that position.
#[inline]
#[target_feature(enable = "avx2")]
unsafe fn lookup4_chunk(chunk: __m256i, prev_input: &mut __m256i) -> __m256i {
    // ─── build prev1 / prev2 / prev3 ────────────────────────────────────────
    // `_mm256_permute2x128_si256::<0x21>(*prev_input, chunk)` splices the
    // upper 128-bit lane of prev_input with the lower lane of chunk, producing
    // a 256-bit intermediate that feeds `_mm256_alignr_epi8` (which operates
    // per 128-bit lane).
    let cross = _mm256_permute2x128_si256::<0x21>(*prev_input, chunk);

    // prev1[i] = chunk[i-1], with prev_input's last byte at position 0.
    let prev1 = _mm256_alignr_epi8::<15>(chunk, cross);
    // prev2[i] = chunk[i-2]
    let prev2 = _mm256_alignr_epi8::<14>(chunk, cross);
    // prev3[i] = chunk[i-3]
    let prev3 = _mm256_alignr_epi8::<13>(chunk, cross);

    // ─── check_special_cases ────────────────────────────────────────────────
    let nibble_mask = _mm256_set1_epi8(0x0F);

    let hi1 = _mm256_and_si256(_mm256_srli_epi16(prev1, 4), nibble_mask);
    let lo1 = _mm256_and_si256(prev1, nibble_mask);
    let hi2 = _mm256_and_si256(_mm256_srli_epi16(chunk, 4), nibble_mask);

    let byte1_high_tbl = _mm256_broadcastsi128_si256(
        _mm_loadu_si128(BYTE1_HIGH.as_ptr() as *const __m128i));
    let byte1_low_tbl  = _mm256_broadcastsi128_si256(
        _mm_loadu_si128(BYTE1_LOW.as_ptr() as *const __m128i));
    let byte2_high_tbl = _mm256_broadcastsi128_si256(
        _mm_loadu_si128(BYTE2_HIGH.as_ptr() as *const __m128i));

    let sc = _mm256_and_si256(
        _mm256_and_si256(
            _mm256_shuffle_epi8(byte1_high_tbl, hi1),
            _mm256_shuffle_epi8(byte1_low_tbl,  lo1),
        ),
        _mm256_shuffle_epi8(byte2_high_tbl, hi2),
    );

    // ─── check_multibyte_lengths ────────────────────────────────────────────
    // must_be_2_3_continuation(prev2, prev3):
    //   is_third_byte  = saturating_sub(prev2, 0xE0 - 0x80) i.e. saturating_sub(prev2, 0x60)
    //   is_fourth_byte = saturating_sub(prev3, 0xF0 - 0x80) i.e. saturating_sub(prev3, 0x70)
    // Only bytes >= 0xE0 / >= 0xF0 respectively produce non-zero results.
    let is_third  = _mm256_subs_epu8(prev2, _mm256_set1_epi8(0x60_u8 as i8));
    let is_fourth = _mm256_subs_epu8(prev3, _mm256_set1_epi8(0x70_u8 as i8));
    let must23 = _mm256_or_si256(is_third, is_fourth);
    // & 0x80: only keep the high bit (any non-zero saturated value sets bit 7 after & 0x80)
    let must23_80 = _mm256_and_si256(must23, _mm256_set1_epi8(0x80_u8 as i8));

    // Update carry for next chunk.
    *prev_input = chunk;

    // Final error mask: XOR cancels out the expected bits.
    _mm256_xor_si256(must23_80, sc)
}

/// Validate `span` using AVX2 three-tier dispatch.
#[doc(hidden)]
pub fn validate_span_avx2(span: &[u8]) -> Result<(), qjson_err> {
    // SAFETY: dispatcher has verified the AVX2 feature is present.
    unsafe { validate_span_avx2_impl(span) }
}

#[target_feature(enable = "avx2")]
unsafe fn validate_span_avx2_impl(span: &[u8]) -> Result<(), qjson_err> {
    let mut i: usize = 0;
    let n = span.len();

    let mut prev_input = _mm256_setzero_si256();
    let mut err_acc    = _mm256_setzero_si256();
    let mut prev_ended_ascii = true;

    while i + 32 <= n {
        let chunk = _mm256_loadu_si256(span.as_ptr().add(i) as *const __m256i);

        let high  = _mm256_movemask_epi8(chunk) as u32;
        let ctrl  = _mm256_movemask_epi8(_mm256_cmpgt_epi8(
                        _mm256_set1_epi8(0x20),
                        chunk,
                    )) as u32;
        let bs    = _mm256_movemask_epi8(_mm256_cmpeq_epi8(
                        chunk,
                        _mm256_set1_epi8(b'\\' as i8),
                    )) as u32;

        // Tier 1: pure printable ASCII, and previous chunk also ended ASCII-safe.
        if (high | ctrl | bs) == 0 && prev_ended_ascii {
            prev_input = chunk;
            // prev_ended_ascii stays true (all bytes < 0x80)
            i += 32;
            continue;
        }

        // Tier 3: control byte or backslash → do NOT run lookup4 on this
        // chunk (the scalar path handles it correctly). First flush any UTF-8
        // errors accumulated from prior Tier 2 chunks, then hand off to
        // scalar starting at the appropriate byte.
        if (ctrl | bs) != 0 {
            if _mm256_testz_si256(err_acc, err_acc) == 0 {
                return Err(qjson_err::QJSON_INVALID_UTF8);
            }
            // If the previous chunk ended with a multi-byte lead, we need to
            // include those unfinished bytes in the scalar input so it can
            // validate the full sequence (including the continuation/error).
            if !prev_ended_ascii {
                let last3 = [
                    _mm256_extract_epi8::<29>(prev_input) as u8,
                    _mm256_extract_epi8::<30>(prev_input) as u8,
                    _mm256_extract_epi8::<31>(prev_input) as u8,
                ];
                let mut prefix_len = 0usize;
                if (0xC2..=0xF4).contains(&last3[2]) {
                    prefix_len = 1;
                } else if (0xC2..=0xF4).contains(&last3[1]) {
                    prefix_len = 2;
                } else if (0xC2..=0xF4).contains(&last3[0]) {
                    prefix_len = 3;
                }
                if prefix_len > 0 {
                    let mut combined = Vec::with_capacity(prefix_len + (n - i));
                    combined.extend_from_slice(&last3[3 - prefix_len..]);
                    combined.extend_from_slice(&span[i..]);
                    return super::scalar::validate_span_scalar(&combined);
                }
            }
            // If there are no high-bit bytes in this chunk, bytes before the
            // first ctrl/bs are pure ASCII and need no validation — skip them.
            // If there ARE high-bit bytes, scalar must start from the beginning
            // of the chunk to correctly validate UTF-8 sequences.
            let start = if high == 0 {
                i + (ctrl | bs).trailing_zeros() as usize
            } else {
                i
            };
            return super::scalar::validate_span_scalar(&span[start..]);
        }

        // Tier 2: pure UTF-8 (no control, no backslash) → run lookup4.
        err_acc = _mm256_or_si256(err_acc, lookup4_chunk(chunk, &mut prev_input));
        prev_ended_ascii = (span[i + 31] & 0x80) == 0;
        i += 32;
    }

    // After main loop: if err_acc accumulated any UTF-8 errors, fail before tail.
    if _mm256_testz_si256(err_acc, err_acc) == 0 {
        return Err(qjson_err::QJSON_INVALID_UTF8);
    }

    // Standard tail: if prev_ended_ascii, scalar starts fresh at i.
    // If !prev_ended_ascii, the previous chunk may have left an unfinished
    // multi-byte lead; reconstruct a buffer containing those bytes plus the
    // tail and validate as a unit.
    if !prev_ended_ascii {
        let last3 = [
            _mm256_extract_epi8::<29>(prev_input) as u8,
            _mm256_extract_epi8::<30>(prev_input) as u8,
            _mm256_extract_epi8::<31>(prev_input) as u8,
        ];
        let mut prefix_len = 0usize;
        if (0xC2..=0xF4).contains(&last3[2]) {
            prefix_len = 1;
        } else if (0xC2..=0xF4).contains(&last3[1]) {
            prefix_len = 2;
        } else if (0xC2..=0xF4).contains(&last3[0]) {
            prefix_len = 3;
        }
        if prefix_len > 0 {
            let mut combined = Vec::with_capacity(prefix_len + (n - i));
            combined.extend_from_slice(&last3[3 - prefix_len..]);
            combined.extend_from_slice(&span[i..]);
            return super::scalar::validate_span_scalar(&combined);
        }
    }

    super::scalar::validate_span_scalar(&span[i..])
}
