#![cfg(target_arch = "x86_64")]

use core::arch::x86_64::*;
use super::Scanner;

pub struct Avx2Scanner;

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
    let mut bs_carry: u64 = 0;
    let mut in_string: u64 = 0;

    while i + 64 <= buf.len() {
        let chunk_lo = _mm256_loadu_si256(buf.as_ptr().add(i)        as *const __m256i);
        let chunk_hi = _mm256_loadu_si256(buf.as_ptr().add(i + 32)   as *const __m256i);

        let backslash = byte_mask(chunk_lo, chunk_hi, b'\\');
        let quote     = byte_mask(chunk_lo, chunk_hi, b'"');
        let escaped   = find_escape_mask_with_carry(backslash, &mut bs_carry);
        let real_quote = quote & !escaped;

        // String-skip fast path: when the previous chunk left us inside a
        // string and this chunk contains no unescaped quote, the entire
        // chunk is string interior. No structural chars to emit and
        // in_string stays 1; bs_carry was already updated above. Skip the
        // 14 cmpeq / movemask ops in structural_mask_chunk plus the PCLMUL
        // prefix-XOR — the dominant cost on string-heavy payloads.
        if in_string != 0 && real_quote == 0 {
            i += 64;
            continue;
        }

        let (inside, new_in_string) = inside_string_mask(real_quote, in_string);
        in_string = new_in_string;

        let struct_mask = structural_mask_chunk(chunk_lo, chunk_hi);
        // Exclude structural chars inside strings; re-add real quotes.
        let final_mask = (struct_mask & !inside) | real_quote;

        emit_bits(final_mask, i as u32, out);

        i += 64;
    }

    // Tail (<64 bytes): continue emit-only via scalar, carrying the
    // in_string / bs_carry state from the last AVX2 chunk. Bracket pairing
    // is checked once at the end on the merged indices.
    //
    // If bs_carry == 1 the byte at position `i` is escape-targeted by the
    // trailing backslash run of the prior chunk; inside a string we must
    // skip it (treat as an escaped data byte, not a structural). Outside
    // a string backslashes are plain characters and bs_carry has no effect.
    if i < buf.len() {
        let scalar_start = if in_string != 0 && bs_carry != 0 {
            i + 1
        } else {
            i
        };
        if scalar_start <= buf.len() {
            super::scalar::scan_emit_resume(buf, scalar_start, in_string != 0, out)?;
        } else if in_string != 0 {
            return Err(buf.len());
        }
    } else if in_string != 0 {
        return Err(buf.len());
    }

    super::validate_brackets(buf, out)
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

/// Compute escape mask + new carry. Pure bit-twiddling, no SIMD intrinsics.
/// `prev_carry` is 1 iff the previous chunk ended such that the FIRST byte of
/// the current chunk is "escaped" (preceded by an odd-length run of backslashes
/// that ends at byte 0 of this chunk).
#[inline(always)]
fn find_escape_mask_with_carry(bs: u64, prev_carry: &mut u64) -> u64 {
    let pc = *prev_carry;

    // Identify run starts: positions where bs[i] is set AND bs[i-1] is not.
    // Bit 0's "i-1" is the prev-chunk carry. If prev_carry is 1, bit 0
    // continues a previous run (not a new start). If 0, bit 0 is a new start
    // iff bs bit 0 is set.
    let starts = bs & !((bs << 1) | pc);

    let even_bits: u64 = 0x5555_5555_5555_5555;
    let odd_bits:  u64 = 0xAAAA_AAAA_AAAA_AAAA;
    let even_starts = starts & even_bits;
    let odd_starts  = starts & odd_bits;

    // Carry-adding: each start propagates 1-bits through the run via the bs mask.
    let even_carries = bs.wrapping_add(even_starts);
    let odd_carries  = bs.wrapping_add(odd_starts);

    let even_carry_ends = even_carries & !bs;
    let odd_carry_ends  = odd_carries  & !bs;

    // Bytes that follow odd-length runs are escaped.
    // Even-start, odd-length runs end at an odd position.
    // Odd-start, odd-length runs end at an even position.
    let escaped_from_runs = (even_carry_ends & odd_bits) | (odd_carry_ends & even_bits);

    // If carry-in is 1, bit 0 is also escaped (the prev-chunk run ended exactly
    // at the boundary with odd parity).
    let escaped = escaped_from_runs | pc;

    // Compute the new carry: it's 1 iff the chunk ends mid-run AND the run's
    // length (combined with any continuation from prev_carry) is odd at the
    // boundary.
    //
    // Count trailing backslashes in bs (consecutive 1-bits ending at bit 63):
    let trailing_bs = (!bs).leading_zeros();

    let new_carry = if bs == u64::MAX {
        // Whole chunk is backslashes — parity flips by 64 (even).
        pc
    } else {
        // The trailing run is isolated in this chunk.
        (trailing_bs as u64) & 1
    };

    *prev_carry = new_carry;
    escaped
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

    fn host_supports_avx2() -> bool {
        std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("pclmulqdq")
    }

    fn parity(input: &[u8]) {
        let mut a = Vec::new();
        let mut b = Vec::new();
        ScalarScanner::scan(input, &mut a).unwrap();
        Avx2Scanner::scan(input, &mut b).unwrap();
        assert_eq!(a, b, "mismatch on input {:?}", std::str::from_utf8(input).unwrap_or("(non-utf8)"));
    }

    #[test]
    fn no_strings_matches_scalar() {
        if !host_supports_avx2() { return; }
        parity(b"{}");
        parity(b"[]");
        parity(b"[{}]");
        parity(b"[[[]]]");
        parity(b"[1,2,3,4,5,6,7,8,9,0]");
        parity(b"[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]");
    }

    #[test]
    fn within_chunk_strings_match_scalar() {
        if !host_supports_avx2() { return; }
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
        if !host_supports_avx2() { return; }
        // Build a 64-byte input where bytes 0..64 are a single AVX2 chunk
        // containing a string, and there is no tail.
        // Layout: `{"k":"<48 a's>"}` = 1 + 4 + 1 + 48 + 1 + 1 = 56 bytes. Need 64.
        // Use longer key padding.
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"{\"k\":\"");          // 6
        buf.resize(62, b'a');                          // +56 = 62
        buf.extend_from_slice(b"\"}");                 // +2 = 64
        assert_eq!(buf.len(), 64);
        parity(&buf);
    }

    /// String with internal escapes inside a 64-byte chunk.
    #[test]
    fn chunked_path_with_escapes() {
        if !host_supports_avx2() { return; }
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

    /// Exercises the string-skip fast path: a string spanning many AVX2
    /// chunks with no internal quotes. The fast-path branch must produce
    /// the same emitted offsets as the slow path (which the parity check
    /// against scalar implicitly verifies).
    #[test]
    fn long_string_engages_skip_fastpath() {
        if !host_supports_avx2() { return; }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"k\":\"");
        // ~10 KB of string interior — many chunks fully inside the string.
        buf.resize(buf.len() + 10_000, b'a');
        buf.extend_from_slice(b"\"}");
        // Pad to 64-aligned to also exercise the no-tail branch.
        while buf.len() % 64 != 0 { buf.push(b' '); }
        parity(&buf);
    }

    /// String contains escaped quotes — the fast path must NOT fire when
    /// `real_quote != 0` even though we may still be inside a string at
    /// the chunk boundary.
    #[test]
    fn escaped_quotes_do_not_trip_fastpath() {
        if !host_supports_avx2() { return; }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"k\":\"");
        // Embed real escape sequences periodically so quotes appear in the
        // raw bytes but are escaped; real_quote should still be 0 over
        // these chunks, fast path should fire.
        for _ in 0..50 {
            buf.extend_from_slice(b"aaaaaaaa\\\"");  // 8 a's + \"
        }
        buf.push(b'"');
        buf.push(b'}');
        while buf.len() % 64 != 0 { buf.push(b' '); }
        parity(&buf);
    }

    /// AVX2 main loop + scalar tail: input length not a multiple of 64.
    /// Exercises the path that used to bypass AVX2 entirely.
    #[test]
    fn unaligned_tail_parity() {
        if !host_supports_avx2() { return; }
        // Valid JSON of various non-64-aligned total lengths.
        for tail_len in [1usize, 5, 17, 33, 63] {
            let mut buf = Vec::new();
            buf.extend_from_slice(b"{\"key\":\"");
            while buf.len() < 60 { buf.push(b'x'); }
            buf.extend_from_slice(b"abc\"}");
            // buf now well-formed; pad with spaces after the closing `}`
            // to land at 64 + tail_len total bytes.
            let target = 64 + tail_len;
            while buf.len() < target { buf.push(b' '); }
            assert_eq!(buf.len(), target, "test setup");
            parity(&buf);
        }
    }

    /// String spans the 64-byte chunk boundary; the closing quote lives
    /// in the scalar tail. Requires in_string state to carry correctly.
    #[test]
    fn string_crosses_avx2_boundary() {
        if !host_supports_avx2() { return; }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"k\":\"");      // 6 bytes, in_string from byte 5
        while buf.len() < 80 { buf.push(b'a'); }  // long string content past byte 64
        buf.push(b'"');
        buf.push(b'}');
        parity(&buf);
    }

    /// Backslash at the LAST byte of the AVX2 chunk; the escaped target
    /// is the FIRST byte of the scalar tail. Exercises bs_carry.
    #[test]
    fn backslash_at_chunk_boundary() {
        if !host_supports_avx2() { return; }
        // Bytes 0..63: `{"key":"` followed by 'x' padding ending with `\`.
        // Byte 64 (first tail byte): an escaped `"` — must NOT close the string.
        // Then real closing `"` and `}` follow.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"key\":\"");    // 8 bytes
        while buf.len() < 63 { buf.push(b'x'); }  // pad to 63
        buf.push(b'\\');                          // byte 63: backslash
        buf.push(b'"');                           // byte 64: escaped quote (tail)
        buf.push(b'y');                           // byte 65
        buf.push(b'"');                           // byte 66: real string close
        buf.push(b'}');                           // byte 67
        parity(&buf);
    }

    /// Verifies PCLMUL prefix-XOR produces correct inside-string mask
    /// for multiple strings in a single 64-byte chunk.
    #[test]
    fn pclmul_inside_string_correct() {
        if !host_supports_avx2() { return; }
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
