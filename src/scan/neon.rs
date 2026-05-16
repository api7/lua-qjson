#![cfg(target_arch = "aarch64")]

use core::arch::aarch64::*;
use super::Scanner;

pub struct NeonScanner;

impl Scanner for NeonScanner {
    fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize> {
        if buf.is_empty() { return Ok(()); }
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

/// Build a u64 mask where bit i is set if byte i (across c0..c3) equals `byte`.
#[inline(always)]
unsafe fn byte_mask64(c0: uint8x16_t, c1: uint8x16_t, c2: uint8x16_t, c3: uint8x16_t, byte: u8) -> u64 {
    let v = vdupq_n_u8(byte);
    let m0 = movemask16(vceqq_u8(c0, v)) as u64;
    let m1 = movemask16(vceqq_u8(c1, v)) as u64;
    let m2 = movemask16(vceqq_u8(c2, v)) as u64;
    let m3 = movemask16(vceqq_u8(c3, v)) as u64;
    m0 | (m1 << 16) | (m2 << 32) | (m3 << 48)
}

/// Build a u64 mask where bit i is set if byte i is one of: { } [ ] : , "
#[inline(always)]
unsafe fn structural_mask64(c0: uint8x16_t, c1: uint8x16_t, c2: uint8x16_t, c3: uint8x16_t) -> u64 {
    let chars: [u8; 7] = [b'{', b'}', b'[', b']', b':', b',', b'"'];
    let mut m0: u16 = 0;
    let mut m1: u16 = 0;
    let mut m2: u16 = 0;
    let mut m3: u16 = 0;
    for c in chars {
        let v = vdupq_n_u8(c);
        m0 |= movemask16(vceqq_u8(c0, v));
        m1 |= movemask16(vceqq_u8(c1, v));
        m2 |= movemask16(vceqq_u8(c2, v));
        m3 |= movemask16(vceqq_u8(c3, v));
    }
    (m0 as u64) | ((m1 as u64) << 16) | ((m2 as u64) << 32) | ((m3 as u64) << 48)
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

        let backslash = byte_mask64(c0, c1, c2, c3, b'\\');
        let quote     = byte_mask64(c0, c1, c2, c3, b'"');

        // In-string fast probe: skip the escape/prefix-XOR path entirely when
        // we are already inside a string and there are no quotes or backslashes.
        if in_string != 0 && (backslash | quote) == 0 {
            bs_carry = 0;
            i += 64;
            continue;
        }

        let escaped    = super::find_escape_mask_with_carry(backslash, &mut bs_carry);
        let real_quote = quote & !escaped;
        let (inside, new_in_string) = inside_string_neon(real_quote, in_string);
        in_string = new_in_string;

        let struct_mask = structural_mask64(c0, c1, c2, c3);
        let final_mask  = (struct_mask & !inside) | real_quote;
        super::emit_bits(final_mask, i as u32, out);
        i += 64;
    }

    // Tail (<64 bytes): hand off to scalar emit, carrying in_string / bs_carry state.
    if i < buf.len() {
        let scalar_start = if in_string != 0 && bs_carry != 0 { i + 1 } else { i };
        super::scalar::scan_emit_resume(buf, scalar_start, in_string != 0, out)?;
    } else if in_string != 0 {
        return Err(buf.len());
    }

    super::validate_brackets(buf, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{Scanner, scalar::ScalarScanner};

    fn host_supports_neon_aes() -> bool {
        std::arch::is_aarch64_feature_detected!("aes")
    }

    fn parity(input: &[u8]) {
        let mut a = Vec::new();
        let mut b = Vec::new();
        let ra = ScalarScanner::scan(input, &mut a);
        let rb = NeonScanner::scan(input, &mut b);
        assert_eq!(ra, rb, "result mismatch on {:?}", std::str::from_utf8(input).unwrap_or("(non-utf8)"));
        assert_eq!(a, b,   "indices mismatch on {:?}", std::str::from_utf8(input).unwrap_or("(non-utf8)"));
    }

    #[test]
    fn no_strings_matches_scalar() {
        if !host_supports_neon_aes() { return; }
        parity(b"{}");
        parity(b"[]");
        parity(b"[{}]");
        parity(b"[[[]]]");
        parity(b"[1,2,3,4,5,6,7,8,9,0]");
        parity(b"[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]");
    }

    #[test]
    fn within_chunk_strings_match_scalar() {
        if !host_supports_neon_aes() { return; }
        parity(b"{\"a\":\"hello\"}");
        parity(b"{\"a\":\"he\\nlo\"}");
        parity(b"{\"a\":\"he\\\"lo\"}");
        parity(b"[\"x\",\"y\",\"z\"]");
    }

    #[test]
    fn chunked_path_with_string() {
        if !host_supports_neon_aes() { return; }
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"k\":\"");
        buf.resize(62, b'a');
        buf.extend_from_slice(b"\"}");
        assert_eq!(buf.len(), 64);
        parity(&buf);
    }

    #[test]
    fn chunked_path_with_escapes() {
        if !host_supports_neon_aes() { return; }
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"k\":\"aa\\\"bb\\\\cc");
        while buf.len() < 62 { buf.push(b'x'); }
        buf.push(b'"');
        buf.push(b'}');
        assert_eq!(buf.len(), 64);
        parity(&buf);
    }

    #[test]
    fn long_string_engages_skip_fastpath() {
        if !host_supports_neon_aes() { return; }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"k\":\"");
        buf.resize(buf.len() + 1_048_576, b'a');
        buf.extend_from_slice(b"\"}");
        while buf.len() % 64 != 0 { buf.push(b' '); }
        parity(&buf);
    }

    #[test]
    fn backslash_at_chunk_boundary() {
        if !host_supports_neon_aes() { return; }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"key\":\"");
        while buf.len() < 63 { buf.push(b'x'); }
        buf.push(b'\\');
        buf.push(b'"');
        buf.push(b'y');
        buf.push(b'"');
        buf.push(b'}');
        parity(&buf);
    }

    #[test]
    fn unaligned_tail_parity() {
        if !host_supports_neon_aes() { return; }
        for tail_len in [1usize, 5, 17, 33, 63] {
            let mut buf = Vec::new();
            buf.extend_from_slice(b"{\"key\":\"");
            while buf.len() < 60 { buf.push(b'x'); }
            buf.extend_from_slice(b"abc\"}");
            let target = 64 + tail_len;
            while buf.len() < target { buf.push(b' '); }
            assert_eq!(buf.len(), target, "test setup");
            parity(&buf);
        }
    }

    #[test]
    fn string_crosses_neon_boundary() {
        if !host_supports_neon_aes() { return; }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"k\":\"");
        while buf.len() < 80 { buf.push(b'a'); }
        buf.push(b'"');
        buf.push(b'}');
        parity(&buf);
    }

    #[test]
    fn pclmul_inside_string_correct() {
        if !host_supports_neon_aes() { return; }
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"a\":\"foo\",\"b\":\"bar\"}");
        while buf.len() < 64 { buf.push(b' '); }
        assert_eq!(buf.len(), 64);
        parity(&buf);

        let mut buf2 = Vec::with_capacity(64);
        buf2.extend_from_slice(b"[\"a\",\"b\",\"c\",\"d\",\"e\"]");
        while buf2.len() < 64 { buf2.push(b' '); }
        parity(&buf2);

        let mut buf3 = Vec::with_capacity(64);
        buf3.extend_from_slice(b"{\"a\":\"\\\\\\\\\\\"\"}");
        while buf3.len() < 64 { buf3.push(b' '); }
        parity(&buf3);
    }

    #[test]
    fn invalid_bracket_detected() {
        if !host_supports_neon_aes() { return; }
        // Mismatch detected in scalar tail (short input)
        assert!(NeonScanner::scan(b"{]", &mut Vec::new()).is_err());
        assert!(NeonScanner::scan(b"[}", &mut Vec::new()).is_err());
        assert!(NeonScanner::scan(b"{\"a\":\"foo\"", &mut Vec::new()).is_err());
    }
}
