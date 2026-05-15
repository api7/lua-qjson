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

        let struct_mask = structural_mask_chunk(chunk_lo, chunk_hi);
        emit_bits(struct_mask, i as u32, out);

        i += 64;
    }

    // Tail: scalar fallback for the remainder. Append offsets adjusted by i.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{Scanner, scalar::ScalarScanner};

    fn parity(input: &[u8]) {
        let mut a = Vec::new();
        let mut b = Vec::new();
        ScalarScanner::scan(input, &mut a).unwrap();
        Avx2Scanner::scan(input, &mut b).unwrap();
        assert_eq!(a, b, "mismatch on input {:?}", std::str::from_utf8(input));
    }

    #[test]
    fn no_strings_matches_scalar() {
        // Pure structural inputs (no strings) — Task 13 only handles these correctly.
        parity(b"{}");
        parity(b"[]");
        parity(b"[{}]");
        parity(b"[[[]]]");
        parity(b"[1,2,3,4,5,6,7,8,9,0]");
        // Note: a buffer of nested empty arrays > 64 bytes exercises the chunked path.
        parity(b"[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]");
    }
}
