//! PSHUFB nibble-LUT byte classifier shared by string and number validation.
//!
//! Each byte is decomposed into its high nibble and low nibble. Two
//! 16-entry lookup tables (one per nibble position) are queried and
//! AND'd together, yielding a 16×16 = 256-entry classification table
//! from only 32 bytes of LUT storage. `_mm256_shuffle_epi8` (PSHUFB)
//! applies the lookups across a 32-byte AVX2 chunk in a few cycles.
//!
//! This replaces the three-comparison approach (`high || bs || ctrl`)
//! used by the old string validation fast-path and extends the same
//! LUT infrastructure to number validation.
//!
//! Some items (number LUTs, constants) are kept for planned number
//! validation SIMD path.

#![allow(dead_code)]

pub(crate) const CLS_CTRL:  u8 = 0x01;
pub(crate) const CLS_BS:    u8 = 0x02;
pub(crate) const CLS_HIGH:  u8 = 0x04;
pub(crate) const CLS_DIGIT: u8 = 0x08;
// NUMS is split into two bits so each forms a valid nibble AND-product.
// NUMS0 = {+, -, .} (all share hi=2), NUMS1 = {e, E} (share lo=5).
pub(crate) const CLS_NUMS0: u8 = 0x10;
pub(crate) const CLS_NUMS1: u8 = 0x20;
pub(crate) const CLS_NUMS:  u8 = CLS_NUMS0 | CLS_NUMS1;

// ── LUT tables ──────────────────────────────────────────────────────────
//
// STR tables classify: CTRL (0x00..0x1F), BS (0x5C), HIGH (0x80..0xFF).
// NUM tables inherit string bits and add DIGIT (0x30..0x39) and NUMS
// (`.`, `-`, `+`, `e`, `E`). Each is indexed by the respective nibble;
// the AND of the two lookups yields the final class byte.

#[cfg(target_arch = "x86_64")]
static STR_LO_TABLE: [u8; 16] = [
    0x05, // 0x0  CTRL|HIGH
    0x05, // 0x1
    0x05, // 0x2
    0x05, // 0x3
    0x05, // 0x4
    0x05, // 0x5
    0x05, // 0x6
    0x05, // 0x7
    0x05, // 0x8
    0x05, // 0x9
    0x05, // 0xA
    0x05, // 0xB
    0x07, // 0xC  CTRL|HIGH|BS    (backslash)
    0x05, // 0xD
    0x05, // 0xE
    0x05, // 0xF
];

#[cfg(target_arch = "x86_64")]
static STR_HI_TABLE: [u8; 16] = [
    0x01, // 0x0  CTRL
    0x01, // 0x1  CTRL
    0x00, // 0x2
    0x00, // 0x3
    0x00, // 0x4
    0x02, // 0x5  BS              (backslash)
    0x00, // 0x6
    0x00, // 0x7
    0x04, // 0x8  HIGH
    0x04, // 0x9  HIGH
    0x04, // 0xA  HIGH
    0x04, // 0xB  HIGH
    0x04, // 0xC  HIGH
    0x04, // 0xD  HIGH
    0x04, // 0xE  HIGH
    0x04, // 0xF  HIGH
];

#[cfg(target_arch = "x86_64")]
static NUM_LO_TABLE: [u8; 16] = [
    0x0D, // 0x0  CTRL|HIGH|DIGIT
    0x0D, // 0x1  CTRL|HIGH|DIGIT
    0x0D, // 0x2  CTRL|HIGH|DIGIT
    0x0D, // 0x3  CTRL|HIGH|DIGIT
    0x0D, // 0x4  CTRL|HIGH|DIGIT
    0x2D, // 0x5  CTRL|HIGH|DIGIT|NUMS1   (digit 5, e, E)
    0x0D, // 0x6  CTRL|HIGH|DIGIT
    0x0D, // 0x7  CTRL|HIGH|DIGIT
    0x0D, // 0x8  CTRL|HIGH|DIGIT
    0x0D, // 0x9  CTRL|HIGH|DIGIT
    0x05, // 0xA  CTRL|HIGH
    0x15, // 0xB  CTRL|HIGH|NUMS0          (+)
    0x07, // 0xC  CTRL|HIGH|BS
    0x15, // 0xD  CTRL|HIGH|NUMS0          (-)
    0x15, // 0xE  CTRL|HIGH|NUMS0          (.)
    0x05, // 0xF  CTRL|HIGH
];

#[cfg(target_arch = "x86_64")]
static NUM_HI_TABLE: [u8; 16] = [
    0x01, // 0x0  CTRL
    0x01, // 0x1  CTRL
    0x10, // 0x2  NUMS0                 (+, -, .)
    0x08, // 0x3  DIGIT
    0x20, // 0x4  NUMS1                 (E)
    0x02, // 0x5  BS
    0x20, // 0x6  NUMS1                 (e)
    0x00, // 0x7
    0x04, // 0x8  HIGH
    0x04, // 0x9  HIGH
    0x04, // 0xA  HIGH
    0x04, // 0xB  HIGH
    0x04, // 0xC  HIGH
    0x04, // 0xD  HIGH
    0x04, // 0xE  HIGH
    0x04, // 0xF  HIGH
];

// ── AVX2 classify functions ─────────────────────────────────────────────

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
use core::arch::x86_64::*;

/// Core PSHUFB nibble-LUT classifier.
///
/// Each byte in `chunk` is split into high and low nibbles. The nibbles
/// index into `hi_lut` and `lo_lut` respectively (via `_mm256_shuffle_epi8`);
/// the AND of the two lookups is the per-byte class bitmask.
///
/// `lo_lut` and `hi_lut` are 32-byte `__m256i` whose lower and upper 128-bit
/// lanes each contain a copy of the same 16-entry nibble table (as required
/// by PSHUFB's lane-local indexing).
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn classify_chunk(chunk: __m256i, lo_lut: __m256i, hi_lut: __m256i) -> __m256i {
    let nib_mask = _mm256_set1_epi8(0x0Fu8 as i8);

    let lo_nibs   = _mm256_and_si256(chunk, nib_mask);
    let hi_shift  = _mm256_srli_epi32::<4>(chunk);
    let hi_nibs   = _mm256_and_si256(hi_shift, nib_mask);

    let lo_class = _mm256_shuffle_epi8(lo_lut, lo_nibs);
    let hi_class = _mm256_shuffle_epi8(hi_lut, hi_nibs);

    _mm256_and_si256(lo_class, hi_class)
}

/// Build a 32-byte `__m256i` from a 16-entry nibble LUT by duplicating
/// the table into both 128-bit lanes.
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
unsafe fn make_lut(table: &[u8; 16]) -> __m256i {
    let t = table;
    _mm256_setr_epi8(
        t[0]  as i8, t[1]  as i8, t[2]  as i8, t[3]  as i8,
        t[4]  as i8, t[5]  as i8, t[6]  as i8, t[7]  as i8,
        t[8]  as i8, t[9]  as i8, t[10] as i8, t[11] as i8,
        t[12] as i8, t[13] as i8, t[14] as i8, t[15] as i8,
        t[0]  as i8, t[1]  as i8, t[2]  as i8, t[3]  as i8,
        t[4]  as i8, t[5]  as i8, t[6]  as i8, t[7]  as i8,
        t[8]  as i8, t[9]  as i8, t[10] as i8, t[11] as i8,
        t[12] as i8, t[13] as i8, t[14] as i8, t[15] as i8,
    )
}

/// Classify a 32-byte chunk for string validation.
///
/// Returns a bitmask (one bit per byte) where set bits indicate bytes
/// that have any interesting class bit (CTRL | BS | HIGH). Zero means
/// the entire chunk is pure printable ASCII without escapes or UTF-8.
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn classify_str_chunk(chunk: __m256i) -> u32 {
    classify_str_mask(chunk)
}

/// Returns a bitmask of bytes that match CTRL | BS | HIGH.
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn classify_str_mask(chunk: __m256i) -> u32 {
    let lo_lut     = make_lut(&STR_LO_TABLE);
    let hi_lut     = make_lut(&STR_HI_TABLE);
    let classes    = classify_chunk(chunk, lo_lut, hi_lut);
    let zero       = _mm256_cmpeq_epi8(classes, _mm256_setzero_si256());
    let zero_mask  = _mm256_movemask_epi8(zero) as u32;
    zero_mask ^ 0xFFFF_FFFF   // invert: 1 = interesting
}

/// Classify a 32-byte chunk for number validation.
///
/// Returns `(class_vector, bad_mask)`:
///   - `class_vector`: per-byte class bitmask (DIGIT | NUMS | CTRL | …)
///   - `bad_mask`:     bits set for bytes with CTRL | BS | HIGH (unconditionally invalid in a number).
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn classify_num_chunk(chunk: __m256i) -> (__m256i, u32) {
    let lo_lut     = make_lut(&NUM_LO_TABLE);
    let hi_lut     = make_lut(&NUM_HI_TABLE);
    let classes    = classify_chunk(chunk, lo_lut, hi_lut);

    // bad = bytes where CTRL | BS | HIGH is set.
    let bad_bits   = _mm256_and_si256(classes, _mm256_set1_epi8((CLS_CTRL | CLS_BS | CLS_HIGH) as i8));
    let zero       = _mm256_cmpeq_epi8(bad_bits, _mm256_setzero_si256());
    let bad_mask   = _mm256_movemask_epi8(zero) as u32 ^ 0xFFFF_FFFF;

    (classes, bad_mask)
}

// ── Exhaustive LUT tests ────────────────────────────────────────────────

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;

    fn str_expected(b: u8) -> u8 {
        let mut bits = 0u8;
        if b <= 0x1F             { bits |= CLS_CTRL; }
        if b == b'\\'            { bits |= CLS_BS; }
        if b >= 0x80             { bits |= CLS_HIGH; }
        bits
    }

    fn num_expected(b: u8) -> u8 {
        let mut bits = str_expected(b);
        if b.is_ascii_digit()    { bits |= CLS_DIGIT; }
        if matches!(b, b'+' | b'-' | b'.') { bits |= CLS_NUMS0; }
        if matches!(b, b'e' | b'E')        { bits |= CLS_NUMS1; }
        bits
    }

    #[test]
    fn str_lut_exhaustive() {
        for b in 0..=255u8 {
            let hi = (b >> 4) as usize;
            let lo = (b & 0x0F) as usize;
            let got = STR_HI_TABLE[hi] & STR_LO_TABLE[lo];
            let exp = str_expected(b);
            assert_eq!(got, exp,
                "byte 0x{b:02X} ('{}'): got 0x{got:02X}, expected 0x{exp:02X}",
                b.escape_ascii());
        }
    }

    #[test]
    fn num_lut_exhaustive() {
        for b in 0..=255u8 {
            let hi = (b >> 4) as usize;
            let lo = (b & 0x0F) as usize;
            let got = NUM_HI_TABLE[hi] & NUM_LO_TABLE[lo];
            let exp = num_expected(b);
            assert_eq!(got, exp,
                "byte 0x{b:02X} ('{}'): got 0x{got:02X}, expected 0x{exp:02X}",
                b.escape_ascii());
        }
    }

    // Double-check nibble-resolution edge cases.
    #[test]
    fn num_digit5_is_digit_not_nums() {
        // 0x35 = '5': DIGIT set, neither NUMS0 nor NUMS1 set
        // (lo=5 carries NUMS1 for e/E; resolved by hi=3 which lacks NUMS1).
        let hi = 0x3;
        let lo = 0x5;
        let got = NUM_HI_TABLE[hi] & NUM_LO_TABLE[lo];
        assert_eq!(got, CLS_DIGIT,
            "'5' should be DIGIT only (got 0x{got:02X})");
    }

    #[test]
    fn num_e_is_nums1_not_digit() {
        // 0x65 = 'e': NUMS1 set, DIGIT not set
        // (lo=5 carries both DIGIT and NUMS1; resolved by hi=6 with NUMS1 only).
        let hi = 0x6;
        let lo = 0x5;
        let got = NUM_HI_TABLE[hi] & NUM_LO_TABLE[lo];
        assert_eq!(got, CLS_NUMS1,
            "'e' should be NUMS1 only (got 0x{got:02X})");
    }

    #[test]
    fn num_e_upper_is_nums1_not_digit() {
        let hi = 0x4;
        let lo = 0x5;
        let got = NUM_HI_TABLE[hi] & NUM_LO_TABLE[lo];
        assert_eq!(got, CLS_NUMS1,
            "'E' should be NUMS1 only (got 0x{got:02X})");
    }

    #[test]
    fn num_percent_is_not_nums() {
        // 0x25 = '%': hi=2 (NUMS0), lo=5 (NUMS1|DIGIT) → must NOT collide.
        let hi = 0x2;
        let lo = 0x5;
        let got = NUM_HI_TABLE[hi] & NUM_LO_TABLE[lo];
        assert_eq!(got, 0,
            "'%' should have no class bits (got 0x{got:02X})");
    }

    #[test]
    fn str_0x7f_is_clean() {
        // DEL (0x7F) is allowed by RFC 8259 in strings.
        let hi = 0x7;
        let lo = 0xF;
        let got = STR_HI_TABLE[hi] & STR_LO_TABLE[lo];
        assert_eq!(got, 0, "0x7F should be clean (got 0x{got:02X})");
    }
}
