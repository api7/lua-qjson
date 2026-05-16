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
    let escaped = escaped_from_runs | pc;

    let trailing_bs = (!bs).leading_zeros();

    let new_carry = if bs == u64::MAX {
        pc
    } else {
        (trailing_bs as u64) & 1
    };

    *prev_carry = new_carry;
    escaped
}

/// Emit all set-bit positions in `mask` (relative to `base`) into `out`, while
/// fusing bracket-pair validation inline. The SIMD scanners guarantee that any
/// emitted offset corresponds to a byte that is either a real (unescaped) quote
/// or a top-level structural char outside of strings — so `"`, `:`, `,` are
/// no-ops here and `{` `[` `}` `]` are validated against `stack`.
///
/// Returns `Err(pos)` on the first bracket mismatch. On success, `stack` is
/// left in its final state for the caller (further tail emits and end-of-input
/// `stack.is_empty()` check).
#[inline(always)]
pub(crate) fn emit_bits_validate(
    buf: &[u8],
    mut mask: u64,
    base: u32,
    stack: &mut Vec<u8>,
    out: &mut Vec<u32>,
) -> Result<(), usize> {
    while mask != 0 {
        let tz  = mask.trailing_zeros();
        let pos = base + tz;
        out.push(pos);
        match buf[pos as usize] {
            c @ (b'{' | b'[') => stack.push(c),
            b'}' => if stack.pop() != Some(b'{') { return Err(pos as usize); },
            b']' => if stack.pop() != Some(b'[') { return Err(pos as usize); },
            _ => {} // `"` `:` `,` — no validation
        }
        mask &= mask - 1;
    }
    Ok(())
}

/// Walk already-emitted indices (from the scalar tail handler) and continue
/// bracket-pair validation using the SIMD-loop's stack. Same per-index logic
/// as `emit_bits_validate`; does not push to `out` (the tail handler already
/// did). Used after `scan_emit_resume` to fold the tail into the same pass.
///
/// Like `emit_bits_validate`, this relies on the invariant that no in-string
/// bracket / colon / comma is ever emitted: `"`, `:`, `,` are no-ops.
#[inline]
pub(crate) fn validate_tail_indices(
    buf: &[u8],
    indices: &[u32],
    stack: &mut Vec<u8>,
) -> Result<(), usize> {
    for &idx in indices {
        let pos = idx as usize;
        match buf[pos] {
            c @ (b'{' | b'[') => stack.push(c),
            b'}' => if stack.pop() != Some(b'{') { return Err(pos); },
            b']' => if stack.pop() != Some(b'[') { return Err(pos); },
            _ => {} // `"` `:` `,` — no validation
        }
    }
    Ok(())
}
