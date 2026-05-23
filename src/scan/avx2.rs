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

        // In-string fast-probe: when the previous chunk left us inside a
        // string, check for `"` or `\` BEFORE computing the backslash /
        // escape masks. If neither byte appears in the chunk, the whole
        // chunk is pure string interior — skip without computing the
        // ~10-op scalar `find_escape_mask_with_carry`. bs_carry must be
        // 0 leaving this chunk (no backslashes in chunk → no trailing
        // run); in_string stays 1 (no real quote → no polarity flip).
        if in_string != 0 {
            let interesting = quote_or_backslash_mask(chunk_lo, chunk_hi);
            if interesting == 0 {
                bs_carry = 0;
                i += 64;
                // Cross-chunk jump: no quote/backslash means in_string polarity
                // cannot flip and no escape can start, so jump straight to the
                // 64B-aligned chunk containing the next interesting byte.
                // The 4 KB remaining-buffer threshold suppresses the memchr2
                // call entirely on small payloads (≤4 KB total), where the per-
                // call libc overhead exceeds the in-string probe loop it would
                // replace. On larger payloads only the last 4 KB foregoes the
                // jump — negligible against MB-scale gains.
                if i + 4096 <= buf.len() {
                    let scan_end = buf.len() - (buf.len() % 64);
                    let jump = match memchr::memchr2(b'"', b'\\', &buf[i..scan_end]) {
                        Some(rel) => rel & !63,
                        None      => scan_end - i,
                    };
                    i += jump;
                }
                continue;
            }
        }

        let backslash = byte_mask(chunk_lo, chunk_hi, b'\\');
        let quote     = byte_mask(chunk_lo, chunk_hi, b'"');
        let escaped   = super::find_escape_mask_with_carry(backslash, &mut bs_carry);
        let real_quote = quote & !escaped;

        let (inside, new_in_string) = inside_string_mask(real_quote, in_string);
        in_string = new_in_string;

        let struct_mask = structural_mask_chunk(chunk_lo, chunk_hi);
        // Exclude structural chars inside strings; re-add real quotes.
        let final_mask = (struct_mask & !inside) | real_quote;

        super::emit_bits(final_mask, i as u32, out);

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
        // Invariant: scalar_start ∈ {i, i+1} and i < buf.len(), so
        // scalar_start <= buf.len(). The boundary case scalar_start ==
        // buf.len() only fires when i == buf.len()-1 AND in_string != 0
        // AND bs_carry != 0; scan_emit_resume handles it by entering with
        // an empty loop body and returning Err(buf.len()) from its
        // post-loop `if in_str` check.
        let scalar_start = if in_string != 0 && bs_carry != 0 {
            i + 1
        } else {
            i
        };
        super::scalar::scan_emit_resume(buf, scalar_start, in_string != 0, out)?;
    } else if in_string != 0 {
        // 64-aligned input that ended mid-string: tail handler never runs,
        // so flag the unterminated string here.
        return Err(buf.len());
    }

    super::validate_brackets(buf, out)
}

// Tag bits for nibble-based structural byte classification (mirrors NEON).
const TAG_QUOTE: u8         = 0x01;
const TAG_COMMA: u8         = 0x02;
const TAG_COLON: u8         = 0x04;
const TAG_OPEN_BRACKET: u8  = 0x08;
const TAG_CLOSE_BRACKET: u8 = 0x10;
const TAG_OPEN_BRACE: u8    = 0x20;
const TAG_CLOSE_BRACE: u8   = 0x40;

#[inline(always)]
unsafe fn structural_mask_chunk(lo: __m256i, hi: __m256i) -> u64 {
    // Nibble-based classification via PSHUFB LUTs.  Each structural byte
    // has a unique (hi, lo) nibble pair; the LUTs hold disjoint tag bits
    // so that HI_LUT[hi] & LO_LUT[lo] is non-zero only for the 7
    // structural bytes: { } [ ] : , "
    #[rustfmt::skip]
    const HI_LUT: [u8; 16] = [
        0, 0,
        TAG_QUOTE | TAG_COMMA,           // index 2: 0x2_
        TAG_COLON,                       // index 3: 0x3_
        0,
        TAG_OPEN_BRACKET | TAG_CLOSE_BRACKET, // index 5: 0x5_
        0,
        TAG_OPEN_BRACE | TAG_CLOSE_BRACE,     // index 7: 0x7_
        0, 0, 0, 0, 0, 0, 0, 0,
    ];
    #[rustfmt::skip]
    const LO_LUT: [u8; 16] = [
        0, 0,
        TAG_QUOTE,                                   // index  2: 0x_2
        0, 0, 0, 0, 0, 0, 0,
        TAG_COLON,                                   // index 10: 0x_A
        TAG_OPEN_BRACKET | TAG_OPEN_BRACE,           // index 11: 0x_B
        TAG_COMMA,                                   // index 12: 0x_C
        TAG_CLOSE_BRACKET | TAG_CLOSE_BRACE,         // index 13: 0x_D
        0, 0,
    ];

    let hi_lut = _mm256_broadcastsi128_si256(
        _mm_loadu_si128(HI_LUT.as_ptr() as *const __m128i));
    let lo_lut = _mm256_broadcastsi128_si256(
        _mm_loadu_si128(LO_LUT.as_ptr() as *const __m128i));
    let mask_0f = _mm256_set1_epi8(0x0f);
    let zero   = _mm256_setzero_si256();
    let all_ff = _mm256_cmpeq_epi8(zero, zero); // 0xFF in every lane

    let classify = |chunk: __m256i| -> i32 {
        let hi_nib = _mm256_and_si256(_mm256_srli_epi16::<4>(chunk), mask_0f);
        let lo_nib = _mm256_and_si256(chunk, mask_0f);
        let hi_part = _mm256_shuffle_epi8(hi_lut, hi_nib);
        let lo_part = _mm256_shuffle_epi8(lo_lut, lo_nib);
        let tags = _mm256_and_si256(hi_part, lo_part);
        // tags != 0  →  structural.  Map to 0xFF / 0x00 for movemask.
        let is_zero = _mm256_cmpeq_epi8(tags, zero);
        _mm256_movemask_epi8(_mm256_xor_si256(is_zero, all_ff))
    };

    let mlo = classify(lo);
    let mhi = classify(hi);
    (mlo as u32 as u64) | ((mhi as u32 as u64) << 32)
}

/// Build a u64 mask where bit i is 1 if byte i in (lo|hi) equals `"` OR `\`.
/// Used by the in-string fast-probe to detect pure string-interior chunks
/// in ~10 vector ops (4 cmpeq + 2 or + 2 movemask + shift/or), avoiding
/// the ~25-op slow path including find_escape_mask_with_carry.
#[inline(always)]
unsafe fn quote_or_backslash_mask(lo: __m256i, hi: __m256i) -> u64 {
    let vq = _mm256_set1_epi8(b'"' as i8);
    let vb = _mm256_set1_epi8(b'\\' as i8);
    let lo_or = _mm256_or_si256(_mm256_cmpeq_epi8(lo, vq), _mm256_cmpeq_epi8(lo, vb));
    let hi_or = _mm256_or_si256(_mm256_cmpeq_epi8(hi, vq), _mm256_cmpeq_epi8(hi, vb));
    let mlo = _mm256_movemask_epi8(lo_or) as u32 as u64;
    let mhi = _mm256_movemask_epi8(hi_or) as u32 as u64;
    mlo | (mhi << 32)
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
    ///
    /// Sized at ≥1 MB so thousands of consecutive probe-hit chunks exercise
    /// the new in-string fast-probe path; smaller inputs would only hit a
    /// few hundred chunks and miss patterns that need a long pure-interior
    /// run to surface.
    #[test]
    fn long_string_engages_skip_fastpath() {
        if !host_supports_avx2() { return; }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"k\":\"");
        // ≥1 MB of string interior — thousands of chunks fully inside the
        // string, all hitting the in_string probe path.
        buf.resize(buf.len() + 1_048_576, b'a');
        buf.extend_from_slice(b"\"}");
        // Pad to 64-aligned to also exercise the no-tail branch.
        while buf.len() % 64 != 0 { buf.push(b' '); }
        parity(&buf);
    }

    /// Long string with periodic backslash-escape sequences. Alternates
    /// probe-hit chunks (pure interior) and probe-miss chunks (containing
    /// `\` or escaped `"`), so the slow path engages every few chunks
    /// while the fast probe handles the rest. Parity guarantees the two
    /// paths agree under the new condition.
    #[test]
    fn long_string_with_periodic_backslash() {
        if !host_supports_avx2() { return; }
        let mut buf = Vec::new();
        buf.extend_from_slice(b"{\"k\":\"");
        // ~5 chunks (320 bytes) of pure interior, then an escape sequence,
        // repeated. Mix `\\n` (escaped newline letter) and `\\\"` (escaped
        // quote) so both backslash-only and quote-after-backslash chunks
        // appear.
        for i in 0..200 {
            buf.resize(buf.len() + 320, b'a');
            if i % 2 == 0 {
                buf.extend_from_slice(b"\\n");
            } else {
                buf.extend_from_slice(b"\\\"");
            }
        }
        buf.push(b'"');
        buf.push(b'}');
        while buf.len() % 64 != 0 { buf.push(b' '); }
        parity(&buf);
    }

    /// bs_carry = 1 leaving a chunk that ends in an odd-length backslash
    /// run, then the next chunk is pure string interior (no `"`, no `\`).
    /// Verifies that the in-string fast probe correctly resets bs_carry
    /// to 0 (matching the slow path's `find_escape_mask_with_carry` else
    /// branch). If the probe forgot to clear bs_carry, the third chunk's
    /// byte 0 would be wrongly treated as escaped.
    #[test]
    fn bs_carry_one_at_pure_string_chunk_boundary() {
        if !host_supports_avx2() { return; }
        let mut buf = Vec::new();
        // Chunk 0 (bytes 0..64): open object, open string, then padding
        // ending with exactly one trailing backslash at byte 63. The
        // backslash is preceded by even bytes of non-backslash, so the
        // trailing run has length 1 (odd) → bs_carry=1 leaving chunk 0.
        buf.extend_from_slice(b"{\"k\":\"");          // 6 bytes
        buf.resize(63, b'a');                         // pad to byte 63
        buf.push(b'\\');                              // byte 63: single backslash
        assert_eq!(buf.len(), 64);
        // Chunk 1 (bytes 64..128): byte 64 is the escape TARGET (any
        // non-special byte). Then pure interior — no `"`, no `\` — for
        // the rest of the chunk. This is the chunk the probe must handle
        // correctly. With incoming bs_carry=1, slow path would set
        // escaped[0]=1; new fast probe just clears bs_carry to 0. Both
        // produce zero emitted offsets in this chunk.
        buf.push(b'n');                               // byte 64: escape target
        buf.resize(128, b'a');                        // bytes 65..128: pure interior
        // Chunk 2 (bytes 128..192): another pure-interior chunk to
        // confirm bs_carry stays clean across multiple probe hits.
        buf.resize(192, b'a');
        // Close the string and object in a third chunk.
        buf.push(b'"');
        buf.push(b'}');
        while buf.len() % 64 != 0 { buf.push(b' '); }
        parity(&buf);
    }

    /// String contains escaped quotes — the parity output must still
    /// match scalar. (We cannot directly observe whether the fast path
    /// took the branch; parity asserts equivalence either way.)
    #[test]
    fn escaped_quotes_remain_correct_with_fastpath() {
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
