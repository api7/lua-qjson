pub(crate) mod scalar;
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
pub(crate) mod avx2;
#[cfg(target_arch = "aarch64")]
pub(crate) mod neon;

use once_cell::sync::OnceCell;

/// A structural scanner: given a JSON byte buffer, append the byte offset of
/// every structural character (`{` `}` `[` `]` `:` `,` `"`) that is NOT inside
/// a string literal to `out`. On shallow validation failure (unclosed string,
/// unmatched bracket), returns `Err(offset)` where `offset` is the byte
/// position the failure was detected at. The offset is informational and not
/// exposed via FFI in v1.
pub trait Scanner {
    fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize>;
}

pub use scalar::ScalarScanner;

type ScanFn = fn(&[u8], &mut Vec<u32>) -> Result<(), usize>;
static SCAN_FN: OnceCell<ScanFn> = OnceCell::new();

pub(crate) fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize> {
    let f = *SCAN_FN.get_or_init(|| {
        #[cfg(all(target_arch = "x86_64", feature = "avx2"))]
        {
            if std::is_x86_feature_detected!("avx2")
                && std::is_x86_feature_detected!("pclmulqdq")
            {
                return <avx2::Avx2Scanner as Scanner>::scan;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            if std::arch::is_aarch64_feature_detected!("aes") {
                return <neon::NeonScanner as Scanner>::scan;
            }
        }
        <ScalarScanner as Scanner>::scan
    });
    f(buf, out)
}

/// Compute escape mask + new carry. Pure bit-twiddling, no SIMD intrinsics.
/// `prev_carry` is 1 iff the previous chunk ended such that the FIRST byte of
/// the current chunk is "escaped" (preceded by an odd-length run of backslashes
/// that ends at byte 0 of this chunk).
#[inline(always)]
pub(crate) fn find_escape_mask_with_carry(bs: u64, prev_carry: &mut u64) -> u64 {
    let pc = *prev_carry;

    // Identify run starts: positions where bs[i] is set AND bs[i-1] is not.
    let starts = bs & !((bs << 1) | pc);

    let even_bits: u64 = 0x5555_5555_5555_5555;
    let odd_bits:  u64 = 0xAAAA_AAAA_AAAA_AAAA;
    let even_starts = starts & even_bits;
    let odd_starts  = starts & odd_bits;

    let even_carries = bs.wrapping_add(even_starts);
    let odd_carries  = bs.wrapping_add(odd_starts);

    let even_carry_ends = even_carries & !bs;
    let odd_carry_ends  = odd_carries  & !bs;

    let escaped_from_runs = (even_carry_ends & odd_bits) | (odd_carry_ends & even_bits);
    let mut escaped = escaped_from_runs | pc;

    // If a backslash run crosses the chunk boundary, `starts` deliberately
    // suppresses bit 0, so restore the escaped byte after that leading run.
    if pc != 0 && (bs & 1) != 0 {
        let leading_bs = bs.trailing_ones();
        if leading_bs < 64 && (leading_bs & 1) == 0 {
            escaped |= 1u64 << leading_bs;
        }
    }

    let trailing_bs = (!bs).leading_zeros();

    let new_carry = if bs == u64::MAX {
        pc
    } else {
        (trailing_bs as u64) & 1
    };

    *prev_carry = new_carry;
    escaped
}

/// Emit all set-bit positions in `mask` (relative to `base`) into `out`.
#[inline(always)]
pub(crate) fn emit_bits(mut mask: u64, base: u32, out: &mut Vec<u32>) {
    while mask != 0 {
        let tz = mask.trailing_zeros();
        out.push(base + tz);
        mask &= mask - 1;
    }
}

/// Walk a sequence of already-emitted structural offsets and verify that
/// `{`/`}` and `[`/`]` are properly paired. String quotes toggle an
/// `in_string` flag and are otherwise skipped. This pass trusts the emit
/// phase: a forged quote in the index list would flip `in_string` and
/// mask subsequent bracket mismatches, so the function is correctness-
/// coupled with the scanner that produced `indices`, not defensive
/// against arbitrary inputs.
///
/// On the first mismatch, returns `Err(offset_in_buf)`. On unmatched
/// openers at end of input, returns `Err(buf.len())`.
pub(crate) fn validate_brackets(buf: &[u8], indices: &[u32]) -> Result<(), usize> {
    let mut stack: Vec<u8> = Vec::with_capacity(32);
    let mut in_string = false;

    for &idx in indices {
        let pos = idx as usize;
        let b = buf[pos];

        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }

        match b {
            b'{' | b'[' => stack.push(b),
            b'}' if stack.pop() != Some(b'{') => return Err(pos),
            b']' if stack.pop() != Some(b'[') => return Err(pos),
            _ => {}
        }
    }

    if !stack.is_empty() {
        return Err(buf.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::find_escape_mask_with_carry;

    #[test]
    fn continued_escape_mask_marks_quote_after_even_boundary_run() {
        // Previous chunk ended with one '\' and this chunk starts with two
        // more before a quote: the quote sees a 3-byte run and is escaped.
        let mut carry = 1;
        let escaped = find_escape_mask_with_carry(0b11, &mut carry);
        assert_ne!(escaped & (1 << 2), 0);
        assert_eq!(carry, 0);
    }

    #[test]
    fn continued_escape_mask_leaves_quote_after_odd_boundary_run_real() {
        // Previous odd run + one leading '\' = an even run before byte 1.
        let mut carry = 1;
        let escaped = find_escape_mask_with_carry(0b1, &mut carry);
        assert_eq!(escaped & (1 << 1), 0);
        assert_eq!(carry, 0);
    }
}
