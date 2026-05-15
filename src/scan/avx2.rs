#![cfg(target_arch = "x86_64")]

use core::arch::x86_64::*;
use super::Scanner;

pub(crate) struct Avx2Scanner;

impl Scanner for Avx2Scanner {
    fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize> {
        if buf.is_empty() { return Ok(()); }
        out.reserve(buf.len() / 6);
        // SAFETY: target feature presence is verified at runtime by the
        // dispatcher (Task 16). Direct calls in tests assume the host has
        // AVX2+PCLMUL (the CI / test runner is responsible for this).
        unsafe { scan_avx2_impl(buf, out) }
    }
}

#[target_feature(enable = "avx2,pclmulqdq")]
unsafe fn scan_avx2_impl(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize> {
    let mut i: usize = 0;

    while i + 64 <= buf.len() {
        let chunk_lo = _mm256_loadu_si256(buf.as_ptr().add(i)        as *const __m256i);
        let chunk_hi = _mm256_loadu_si256(buf.as_ptr().add(i + 32)   as *const __m256i);

        let backslash = byte_mask(chunk_lo, chunk_hi, b'\\');
        let quote     = byte_mask(chunk_lo, chunk_hi, b'"');
        let escaped   = find_escape_mask(backslash);
        let real_quote = quote & !escaped;

        let (inside, _new_in_string) = inside_string_mask(real_quote, 0);

        let struct_mask = structural_mask_chunk(chunk_lo, chunk_hi);
        // Exclude structural chars inside strings; re-add real quotes.
        let final_mask = (struct_mask & !inside) | real_quote;

        emit_bits(final_mask, i as u32, out);

        i += 64;
    }

    let mut tail = Vec::new();
    super::ScalarScanner::scan(&buf[i..], &mut tail).map_err(|p| p + i)?;
    out.extend(tail.into_iter().map(|p| p + i as u32));
    Ok(())
}

#[inline(always)]
unsafe fn structural_mask_chunk(lo: __m256i, hi: __m256i) -> u64 {
    // For each byte, set 1 if byte is one of: { } [ ] : , "
    // Bit-OR results from 7 byte-equality compares.
    let chars: [u8; 7] = [b'{', b'}', b'[', b']', b':', b',', b'"'];
    let mut mask_lo: i32 = 0;
    let mut mask_hi: i32 = 0;
    for c in chars {
        let v = _mm256_set1_epi8(c as i8);
        let eq_lo = _mm256_cmpeq_epi8(lo, v);
        let eq_hi = _mm256_cmpeq_epi8(hi, v);
        mask_lo |= _mm256_movemask_epi8(eq_lo);
        mask_hi |= _mm256_movemask_epi8(eq_hi);
    }
    (mask_lo as u32 as u64) | ((mask_hi as u32 as u64) << 32)
}

#[inline(always)]
fn emit_bits(mut mask: u64, base: u32, out: &mut Vec<u32>) {
    while mask != 0 {
        let tz = mask.trailing_zeros();
        out.push(base + tz);
        mask &= mask - 1; // clear lowest bit
    }
}

/// Build a u64 mask where bit i is 1 if byte i in (lo|hi) equals `c`.
#[inline(always)]
unsafe fn byte_mask(lo: __m256i, hi: __m256i, c: u8) -> u64 {
    let v = _mm256_set1_epi8(c as i8);
    let eq_lo = _mm256_cmpeq_epi8(lo, v);
    let eq_hi = _mm256_cmpeq_epi8(hi, v);
    let mlo = _mm256_movemask_epi8(eq_lo) as u32 as u64;
    let mhi = _mm256_movemask_epi8(eq_hi) as u32 as u64;
    mlo | (mhi << 32)
}

/// Compute the mask of bytes that are escaped by a preceding backslash.
/// Algorithm (simdjson): find run-starts, split by parity, carry-add to find
/// run-ends. The byte AFTER an odd-length run is escaped.
///
/// Chunk-local: assumes the chunk does NOT begin inside a backslash run.
/// Cross-chunk carry comes in Task 16.
#[inline(always)]
unsafe fn find_escape_mask(backslash_mask: u64) -> u64 {
    let starts = backslash_mask & !(backslash_mask << 1);
    let even_bits: u64 = 0x5555_5555_5555_5555;
    let odd_bits:  u64 = 0xAAAA_AAAA_AAAA_AAAA;
    let even_starts = starts & even_bits;
    let odd_starts  = starts & odd_bits;
    let even_carries = backslash_mask.wrapping_add(even_starts);
    let odd_carries  = backslash_mask.wrapping_add(odd_starts);
    let even_carry_ends = even_carries & !backslash_mask;
    let odd_carry_ends  = odd_carries  & !backslash_mask;
    // Even-start runs of odd length end at odd positions; odd-start odd-length end at even.
    (even_carry_ends & odd_bits) | (odd_carry_ends & even_bits)
}

/// Given the chunk's real-quote mask and the prior chunk's "ended-in-string"
/// state, return (inside_string_mask, new_in_string_state).
/// `prev_in_string` is 0 or 1.
#[target_feature(enable = "avx2,pclmulqdq")]
unsafe fn inside_string_mask(real_quote: u64, prev_in_string: u64) -> (u64, u64) {
    // Prefix XOR via carry-less multiply by all-ones.
    let ones = _mm_set1_epi64x(-1i64);
    let q = _mm_set_epi64x(0, real_quote as i64);
    let prefix = _mm_clmulepi64_si128::<0>(q, ones);
    let mut mask = _mm_cvtsi128_si64(prefix) as u64;
    // If the chunk began inside a string, flip polarity.
    if prev_in_string != 0 {
        mask = !mask;
    }
    let new_state = (mask >> 63) & 1;
    (mask, new_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{Scanner, scalar::ScalarScanner};

    fn parity(input: &[u8]) {
        let mut a = Vec::new();
        let mut b = Vec::new();
        ScalarScanner::scan(input, &mut a).unwrap();
        Avx2Scanner::scan(input, &mut b).unwrap();
        assert_eq!(a, b, "mismatch on input {:?}", std::str::from_utf8(input).unwrap_or("(non-utf8)"));
    }

    #[test]
    fn no_strings_matches_scalar() {
        parity(b"{}");
        parity(b"[]");
        parity(b"[{}]");
        parity(b"[[[]]]");
        parity(b"[1,2,3,4,5,6,7,8,9,0]");
        parity(b"[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]");
    }

    #[test]
    fn within_chunk_strings_match_scalar() {
        // These are <64 bytes so they go through the scalar tail path only;
        // they still verify Avx2Scanner does not corrupt the output for these
        // inputs, but they do NOT exercise the AVX2 string handling.
        parity(b"{\"a\":\"hello\"}");
        parity(b"{\"a\":\"he\\nlo\"}");
        parity(b"{\"a\":\"he\\\"lo\"}");
        parity(b"[\"x\",\"y\",\"z\"]");
    }

    /// Exercise the actual AVX2 chunked path with a string spanning bytes
    /// within a single 64-byte chunk.
    #[test]
    fn chunked_path_with_string() {
        // Build a 64-byte input where bytes 0..64 are a single AVX2 chunk
        // containing a string, and there is no tail.
        // Layout: `{"k":"<48 a's>"}` = 1 + 4 + 1 + 48 + 1 + 1 = 56 bytes. Need 64.
        // Use longer key padding.
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"k\":\"");          // 6
        for _ in 0..56 { buf.push(b'a'); }            // +56 = 62
        buf.push(b'"');                                // +1 = 63
        buf.push(b'}');                                // +1 = 64
        assert_eq!(buf.len(), 64);
        parity(&buf);
    }

    /// String with internal escapes inside a 64-byte chunk.
    #[test]
    fn chunked_path_with_escapes() {
        // Bytes: {"k":"aa\"bb\\cc<padding>"}
        // Need exactly 64 bytes. Build it carefully.
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"k\":\"aa\\\"bb\\\\cc"); // 16 bytes
        // Currently 16 bytes. Need 64. Pad with 'x' to reach 62, then close.
        while buf.len() < 62 { buf.push(b'x'); }
        buf.push(b'"');
        buf.push(b'}');
        assert_eq!(buf.len(), 64);
        parity(&buf);
    }

    /// Verifies PCLMUL prefix-XOR produces correct inside-string mask
    /// for multiple strings in a single 64-byte chunk.
    #[test]
    fn pclmul_inside_string_correct() {
        // {"a":"foo","b":"bar"}<padding to 64>
        // Strings "foo" and "bar" both fully within the chunk.
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"a\":\"foo\",\"b\":\"bar\"}");  // 21 bytes
        // Pad with spaces (which are non-structural, non-escapes) to reach 64.
        while buf.len() < 64 { buf.push(b' '); }
        assert_eq!(buf.len(), 64);
        parity(&buf);

        // Array of strings, all <64 bytes total then padded to 64.
        let mut buf2 = Vec::with_capacity(64);
        buf2.extend_from_slice(b"[\"a\",\"b\",\"c\",\"d\",\"e\"]");
        while buf2.len() < 64 { buf2.push(b' '); }
        parity(&buf2);

        // Adversarial: nested escapes inside a string, all in one chunk.
        let mut buf3 = Vec::with_capacity(64);
        buf3.extend_from_slice(b"{\"a\":\"\\\\\\\\\\\"\"}");  // {"a":"\\\\\"" with proper escapes
        while buf3.len() < 64 { buf3.push(b' '); }
        parity(&buf3);
    }
}
