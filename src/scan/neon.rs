#![cfg(target_arch = "aarch64")]

use super::Scanner;
use core::arch::aarch64::*;

pub struct NeonScanner;

const TAG_QUOTE: u8 = 0x01;
const TAG_COMMA: u8 = 0x02;
const TAG_COLON: u8 = 0x04;
const TAG_OPEN_BRACKET: u8 = 0x08;
const TAG_CLOSE_BRACKET: u8 = 0x10;
const TAG_OPEN_BRACE: u8 = 0x20;
const TAG_CLOSE_BRACE: u8 = 0x40;
const TAG_STRUCTURAL: u8 = TAG_QUOTE
    | TAG_COMMA
    | TAG_COLON
    | TAG_OPEN_BRACKET
    | TAG_CLOSE_BRACKET
    | TAG_OPEN_BRACE
    | TAG_CLOSE_BRACE;
const TAG_BACKSLASH: u8 = 0x80;

#[rustfmt::skip]
const HI_LUT: [u8; 16] = [
    0x00, 0x00, TAG_QUOTE | TAG_COMMA, TAG_COLON,
    0x00, TAG_OPEN_BRACKET | TAG_CLOSE_BRACKET | TAG_BACKSLASH, 0x00, TAG_OPEN_BRACE | TAG_CLOSE_BRACE,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
];

#[rustfmt::skip]
const LO_LUT: [u8; 16] = [
    0x00, 0x00, TAG_QUOTE, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, TAG_COLON, TAG_OPEN_BRACKET | TAG_OPEN_BRACE,
    TAG_COMMA | TAG_BACKSLASH, TAG_CLOSE_BRACKET | TAG_CLOSE_BRACE, 0x00, 0x00,
];

impl Scanner for NeonScanner {
    fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize> {
        if buf.is_empty() {
            return Ok(());
        }
        out.reserve(buf.len() / 6);
        // SAFETY: caller (dispatcher in mod.rs) verified `aes` feature is
        // present at runtime via `is_aarch64_feature_detected!("aes")`.
        unsafe { scan_neon_impl(buf, out) }
    }
}

/// Simulate `_mm_movemask_epi8` for a 128-bit NEON register.
/// Returns a u16 where bit i is the high bit of lane i.
/// The input lanes are expected to be 0xFF (match) or 0x00 (no match).
#[inline(always)]
unsafe fn movemask16(v: uint8x16_t) -> u16 {
    // Weight each byte by its bit position within its half-register.
    // Lanes 0..7 use weights 1,2,4,8,16,32,64,128 (low byte of result).
    // Lanes 8..15 use the same weights but are pairsum'd into the high byte.
    const LANE_BITS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
    let lane_mask = vld1q_u8(LANE_BITS.as_ptr());
    // Extract high bit of each lane (all-FF → all-FF, all-00 → all-00).
    let hi = vshrq_n_s8(vreinterpretq_s8_u8(v), 7);
    let weighted = vandq_u8(vreinterpretq_u8_s8(hi), lane_mask);
    // Pairwise sum to condense 16 bytes → 8 u16 → 4 u32 → 2 u64.
    let s16 = vpaddlq_u8(weighted);
    let s32 = vpaddlq_u16(s16);
    let s64 = vpaddlq_u32(s32);
    let lo = vgetq_lane_u64(s64, 0) as u16;
    let hi = vgetq_lane_u64(s64, 1) as u16;
    lo | (hi << 8)
}

#[cfg(test)]
fn scalar_nibble_tag(byte: u8) -> u8 {
    HI_LUT[(byte >> 4) as usize] & LO_LUT[(byte & 0x0f) as usize]
}

#[inline(always)]
unsafe fn classify16(bytes: uint8x16_t, hi_lut: uint8x16_t, lo_lut: uint8x16_t) -> uint8x16_t {
    let lo = vandq_u8(bytes, vdupq_n_u8(0x0f));
    let hi = vshrq_n_u8(bytes, 4);
    vandq_u8(vqtbl1q_u8(hi_lut, hi), vqtbl1q_u8(lo_lut, lo))
}

#[inline(always)]
unsafe fn tag_mask16(tag: uint8x16_t, bits: u8) -> u16 {
    movemask16(vtstq_u8(tag, vdupq_n_u8(bits)))
}

#[inline(always)]
unsafe fn byte_mask16(bytes: uint8x16_t, needle: u8) -> u16 {
    movemask16(vceqq_u8(bytes, vdupq_n_u8(needle)))
}

#[inline(always)]
unsafe fn byte_mask64(
    c0: uint8x16_t,
    c1: uint8x16_t,
    c2: uint8x16_t,
    c3: uint8x16_t,
    needle: u8,
) -> u64 {
    (byte_mask16(c0, needle) as u64)
        | ((byte_mask16(c1, needle) as u64) << 16)
        | ((byte_mask16(c2, needle) as u64) << 32)
        | ((byte_mask16(c3, needle) as u64) << 48)
}

#[inline(always)]
unsafe fn classify_tags64(
    c0: uint8x16_t,
    c1: uint8x16_t,
    c2: uint8x16_t,
    c3: uint8x16_t,
) -> (uint8x16_t, uint8x16_t, uint8x16_t, uint8x16_t) {
    let hi_lut = vld1q_u8(HI_LUT.as_ptr());
    let lo_lut = vld1q_u8(LO_LUT.as_ptr());

    (
        classify16(c0, hi_lut, lo_lut),
        classify16(c1, hi_lut, lo_lut),
        classify16(c2, hi_lut, lo_lut),
        classify16(c3, hi_lut, lo_lut),
    )
}

#[inline(always)]
unsafe fn tag_mask64(
    t0: uint8x16_t,
    t1: uint8x16_t,
    t2: uint8x16_t,
    t3: uint8x16_t,
    bits: u8,
) -> u64 {
    (tag_mask16(t0, bits) as u64)
        | ((tag_mask16(t1, bits) as u64) << 16)
        | ((tag_mask16(t2, bits) as u64) << 32)
        | ((tag_mask16(t3, bits) as u64) << 48)
}

/// Prefix-XOR via PMULL (carry-less multiply by all-ones) to produce an
/// inside-string mask from the real-quote positions.
/// Returns `(inside_mask, new_in_string_state)` where state is 0 or 1.
#[target_feature(enable = "neon,aes")]
unsafe fn inside_string_neon(real_quote: u64, prev_in_string: u64) -> (u64, u64) {
    // vmull_p64(a, u64::MAX) = prefix XOR of bits in `a`.
    let result = vmull_p64(real_quote, u64::MAX);
    // Extract low 64 bits of the 128-bit poly result.
    let result_v: uint64x2_t = vreinterpretq_u64_p128(result);
    let mut mask = vgetq_lane_u64(result_v, 0);
    if prev_in_string != 0 {
        mask = !mask;
    }
    let new_state = (mask >> 63) & 1;
    (mask, new_state)
}

#[target_feature(enable = "neon,aes")]
unsafe fn scan_neon_impl(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize> {
    let mut i = 0usize;
    let mut bs_carry: u64 = 0;
    let mut in_string: u64 = 0;

    while i + 64 <= buf.len() {
        let c0 = vld1q_u8(buf.as_ptr().add(i));
        let c1 = vld1q_u8(buf.as_ptr().add(i + 16));
        let c2 = vld1q_u8(buf.as_ptr().add(i + 32));
        let c3 = vld1q_u8(buf.as_ptr().add(i + 48));

        // In-string fast probe: while already in a string, avoid the full
        // nibble-LUT classification unless this block contains quote/backslash.
        if in_string != 0 {
            let quote_probe = byte_mask64(c0, c1, c2, c3, b'"');
            let backslash_probe = byte_mask64(c0, c1, c2, c3, b'\\');
            if (quote_probe | backslash_probe) == 0 {
                bs_carry = 0;
                i += 64;
                continue;
            }
        }

        let (t0, t1, t2, t3) = classify_tags64(c0, c1, c2, c3);
        let backslash = tag_mask64(t0, t1, t2, t3, TAG_BACKSLASH);
        let quote = tag_mask64(t0, t1, t2, t3, TAG_QUOTE);

        let escaped = super::find_escape_mask_with_carry(backslash, &mut bs_carry);
        let real_quote = quote & !escaped;
        let (inside, new_in_string) = inside_string_neon(real_quote, in_string);
        in_string = new_in_string;

        let struct_mask = tag_mask64(t0, t1, t2, t3, TAG_STRUCTURAL);
        let final_mask = (struct_mask & !inside) | real_quote;
        super::emit_bits(final_mask, i as u32, out);
        i += 64;
    }

    // Tail (<64 bytes): hand off to scalar emit, carrying in_string / bs_carry state.
    if i < buf.len() {
        let scalar_start = if in_string != 0 && bs_carry != 0 {
            i + 1
        } else {
            i
        };
        super::scalar::scan_emit_resume(buf, scalar_start, in_string != 0, out)?;
    } else if in_string != 0 {
        return Err(buf.len());
    }

    super::validate_brackets(buf, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{scalar::ScalarScanner, Scanner};

    #[test]
    fn nibble_lut_classifies_all_256_bytes_correctly() {
        for byte in 0u8..=u8::MAX {
            let tag = scalar_nibble_tag(byte);
            assert_eq!(
                tag & TAG_STRUCTURAL != 0,
                matches!(byte, b'{' | b'}' | b'[' | b']' | b':' | b',' | b'"'),
                "structural classification mismatch for byte {byte:#04x}",
            );
            assert_eq!(
                tag & TAG_QUOTE != 0,
                byte == b'"',
                "quote classification mismatch for byte {byte:#04x}",
            );
            assert_eq!(
                tag & TAG_BACKSLASH != 0,
                byte == b'\\',
                "backslash classification mismatch for byte {byte:#04x}",
            );

            if tag != 0 {
                assert_eq!(
                    tag.count_ones(),
                    1,
                    "classified byte {byte:#04x} should map to exactly one tag bit",
                );
            }
        }
    }

    fn host_supports_neon_aes() -> bool {
        std::arch::is_aarch64_feature_detected!("aes")
    }

    fn parity(input: &[u8]) {
        let mut a = Vec::new();
        let mut b = Vec::new();
        let ra = ScalarScanner::scan(input, &mut a);
        let rb = NeonScanner::scan(input, &mut b);
        assert_eq!(
            ra,
            rb,
            "result mismatch on {:?}",
            std::str::from_utf8(input).unwrap_or("(non-utf8)")
        );
        assert_eq!(
            a,
            b,
            "indices mismatch on {:?}",
            std::str::from_utf8(input).unwrap_or("(non-utf8)")
        );
    }

    #[test]
    fn no_strings_matches_scalar() {
        if !host_supports_neon_aes() {
            return;
        }
        parity(b"{}");
        parity(b"[]");
        parity(b"[{}]");
        parity(b"[[[]]]");
        parity(b"[1,2,3,4,5,6,7,8,9,0]");
        parity(b"[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]");
    }

    #[test]
    fn within_chunk_strings_match_scalar() {
        if !host_supports_neon_aes() {
            return;
        }
        parity(b"{\"a\":\"hello\"}");
        parity(b"{\"a\":\"he\\nlo\"}");
        parity(b"{\"a\":\"he\\\"lo\"}");
        parity(b"[\"x\",\"y\",\"z\"]");
    }

    #[test]
    fn chunked_path_with_string() {
        if !host_supports_neon_aes() {
            return;
        }
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"k\":\"");
        buf.resize(62, b'a');
        buf.extend_from_slice(b"\"}");
        assert_eq!(buf.len(), 64);
        parity(&buf);
    }

    #[test]
    fn chunked_path_with_escapes() {
        if !host_supports_neon_aes() {
            return;
        }
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"k\":\"aa\\\"bb\\\\cc");
        while buf.len() < 62 {
            buf.push(b'x');
        }
        buf.push(b'"');
        buf.push(b'}');
        assert_eq!(buf.len(), 64);
        parity(&buf);
    }

    #[test]
    fn long_string_engages_skip_fastpath() {
        if !host_supports_neon_aes() {
            return;
        }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"k\":\"");
        buf.resize(buf.len() + 1_048_576, b'a');
        buf.extend_from_slice(b"\"}");
        while buf.len() % 64 != 0 {
            buf.push(b' ');
        }
        parity(&buf);
    }

    #[test]
    fn backslash_at_chunk_boundary() {
        if !host_supports_neon_aes() {
            return;
        }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"key\":\"");
        while buf.len() < 63 {
            buf.push(b'x');
        }
        buf.push(b'\\');
        buf.push(b'"');
        buf.push(b'y');
        buf.push(b'"');
        buf.push(b'}');
        parity(&buf);
    }

    #[test]
    fn unaligned_tail_parity() {
        if !host_supports_neon_aes() {
            return;
        }
        for tail_len in [1usize, 5, 17, 33, 63] {
            let mut buf = Vec::new();
            buf.extend_from_slice(b"{\"key\":\"");
            while buf.len() < 60 {
                buf.push(b'x');
            }
            buf.extend_from_slice(b"abc\"}");
            let target = 64 + tail_len;
            while buf.len() < target {
                buf.push(b' ');
            }
            assert_eq!(buf.len(), target, "test setup");
            parity(&buf);
        }
    }

    #[test]
    fn string_crosses_neon_boundary() {
        if !host_supports_neon_aes() {
            return;
        }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"k\":\"");
        while buf.len() < 80 {
            buf.push(b'a');
        }
        buf.push(b'"');
        buf.push(b'}');
        parity(&buf);
    }

    #[test]
    fn pclmul_inside_string_correct() {
        if !host_supports_neon_aes() {
            return;
        }
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"a\":\"foo\",\"b\":\"bar\"}");
        while buf.len() < 64 {
            buf.push(b' ');
        }
        assert_eq!(buf.len(), 64);
        parity(&buf);

        let mut buf2 = Vec::with_capacity(64);
        buf2.extend_from_slice(b"[\"a\",\"b\",\"c\",\"d\",\"e\"]");
        while buf2.len() < 64 {
            buf2.push(b' ');
        }
        parity(&buf2);

        let mut buf3 = Vec::with_capacity(64);
        buf3.extend_from_slice(b"{\"a\":\"\\\\\\\\\\\"\"}");
        while buf3.len() < 64 {
            buf3.push(b' ');
        }
        parity(&buf3);
    }

    #[test]
    fn invalid_bracket_detected() {
        if !host_supports_neon_aes() {
            return;
        }
        // Mismatch detected in scalar tail (short input)
        assert!(NeonScanner::scan(b"{]", &mut Vec::new()).is_err());
        assert!(NeonScanner::scan(b"[}", &mut Vec::new()).is_err());
        assert!(NeonScanner::scan(b"{\"a\":\"foo\"", &mut Vec::new()).is_err());
    }
}
