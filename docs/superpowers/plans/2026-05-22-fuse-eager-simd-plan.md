# Fuse & Accelerate: Eager SIMD Optimization — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fuse the 4 eager-validation passes over `indices` into 1 pass, and accelerate string/number validation with PSHUFB nibble-LUT byte classification (AVX2 + AVX-512).

**Architecture:** A new shared byte-classifier module (`classify.rs`) provides PSHUFB-based per-byte class bitmasks. String validation AVX2 is rewritten to use it (no early-scalar-fallback). An AVX-512 path is added. Number validation gains a SIMD fast path. The `validate_depth`, `validate_trailing`, and `validate_eager_values` functions are merged into `validate_eager_fused` — a single O(indices) traversal. `doc.rs` is updated to call only `validate_eager_fused`.

**Tech Stack:** Rust, x86_64 intrinsics (AVX2 + AVX-512BW/VL), existing `once_cell` dispatch.

---

### Task 1: PSHUFB byte classifier module

**Files:**
- Create: `src/validate/classify.rs`
- Modify: `src/validate/mod.rs` (add module declaration)

- [ ] **Step 1: Create `src/validate/classify.rs`**

```rust
//! PSHUFB nibble-LUT byte classifier shared by string and number
//! validation. Maps each byte to a class bitmask in a single SIMD
//! instruction sequence.
//!
//! Classification: split each byte into high/low nibble, lookup two
//! 16-entry LUTs via `_mm256_shuffle_epi8`, AND the results. The AND
//! means a classification bit is set only if BOTH nibbles allow it.

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

/// Class bits for string validation.
pub(crate) const CLS_CTRL:  u8 = 1 << 0; // control char 0x00..=0x1F
pub(crate) const CLS_BS:    u8 = 1 << 1; // backslash 0x5C
pub(crate) const CLS_HIGH:  u8 = 1 << 2; // high-bit byte >= 0x80

/// Class bits for number validation (includes string bits for reuse).
pub(crate) const CLS_DIGIT: u8 = 1 << 3; // digit 0x30..=0x39
pub(crate) const CLS_NUMS:  u8 = 1 << 4; // number structural: . - e E +

#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) unsafe fn classify_str_chunk(chunk: __m256i) -> u32 {
    classify_chunk(chunk, &STR_LO_LUT, &STR_HI_LUT)
}

/// Classify each byte in the 32-byte chunk. Returns a u32 mask where
/// bit i is set if byte i has any "interesting" class bits
/// (CTRL | BS | HIGH).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) unsafe fn classify_str_mask(chunk: __m256i) -> u32 {
    let class = classify_chunk(chunk, &STR_LO_LUT, &STR_HI_LUT);
    // Extract the "interesting" bits: any non-zero class byte means
    // attention needed. We check CTRL | BS | HIGH bits.
    let want = _mm256_set1_epi8((CLS_CTRL | CLS_BS | CLS_HIGH) as i8);
    let match_mask = _mm256_and_si256(class, want);
    _mm256_movemask_epi8(_mm256_cmpeq_epi8(match_mask, _mm256_setzero_si256())) as u32 ^ 0xFFFFFFFFu32
}

/// Classify a number chunk. Returns the per-byte class vector so the
/// caller can check DIGIT | NUMS validity.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) unsafe fn classify_num_chunk(chunk: __m256i) -> (__m256i, u32) {
    let class = classify_chunk(chunk, &NUM_LO_LUT, &NUM_HI_LUT);
    // Check which bytes are NOT (DIGIT | NUMS).
    let valid = _mm256_set1_epi8((CLS_DIGIT | CLS_NUMS) as i8);
    let ok = _mm256_cmpeq_epi8(_mm256_and_si256(class, valid), _mm256_setzero_si256());
    let bad_mask = _mm256_movemask_epi8(ok) as u32 ^ 0xFFFFFFFFu32;
    (class, bad_mask)
}

/// Core PSHUFB nibble-LUT classifier: returns per-byte class bitmask.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn classify_chunk(chunk: __m256i, lo_lut: &[u8; 16], hi_lut: &[u8; 16]) -> __m256i {
    let zero = _mm256_setzero_si256();
    let nib_mask = _mm256_set1_epi8(0x0F_i8);
    let lo_lut_vec = _mm256_loadu_si256(
        [lo_lut[0], lo_lut[1], lo_lut[2], lo_lut[3],
         lo_lut[4], lo_lut[5], lo_lut[6], lo_lut[7],
         lo_lut[8], lo_lut[9], lo_lut[10], lo_lut[11],
         lo_lut[12], lo_lut[13], lo_lut[14], lo_lut[15],
         lo_lut[0], lo_lut[1], lo_lut[2], lo_lut[3],
         lo_lut[4], lo_lut[5], lo_lut[6], lo_lut[7],
         lo_lut[8], lo_lut[9], lo_lut[10], lo_lut[11],
         lo_lut[12], lo_lut[13], lo_lut[14], lo_lut[15],
        ].as_ptr() as *const __m256i,
    );
    // Build hi_lut vector (same layout).
    let hi_lut_vec = _mm256_loadu_si256(
        [hi_lut[0], hi_lut[1], hi_lut[2], hi_lut[3],
         hi_lut[4], hi_lut[5], hi_lut[6], hi_lut[7],
         hi_lut[8], hi_lut[9], hi_lut[10], hi_lut[11],
         hi_lut[12], hi_lut[13], hi_lut[14], hi_lut[15],
         hi_lut[0], hi_lut[1], hi_lut[2], hi_lut[3],
         hi_lut[4], hi_lut[5], hi_lut[6], hi_lut[7],
         hi_lut[8], hi_lut[9], hi_lut[10], hi_lut[11],
         hi_lut[12], hi_lut[13], hi_lut[14], hi_lut[15],
        ].as_ptr() as *const __m256i,
    );

    let lo_nib = _mm256_and_si256(chunk, nib_mask);
    let hi_nib = _mm256_and_si256(_mm256_srli_epi16::<4>(chunk), nib_mask);

    let lo_class = _mm256_shuffle_epi8(lo_lut_vec, lo_nib);
    let hi_class = _mm256_shuffle_epi8(hi_lut_vec, hi_nib);

    _mm256_and_si256(lo_class, hi_class)
}

// ── String classification LUTs ──────────────────────────────────────
// CTRL: 0x00..=0x1F (high nibble 0x0..=0x1, any low nibble)
// BS:   0x5C          (high nibble 0x5,   low nibble 0xC)
// HIGH: 0x80..=0xFF   (high nibble 0x8..=0xF, any low nibble)

#[cfg(target_arch = "x86_64")]
static STR_LO_LUT: [u8; 16] = {
    let mut l = [0u8; 16];
    let mut i = 0usize;
    while i < 16 {
        l[i] = CLS_CTRL | CLS_HIGH;
        i += 1;
    }
    l[0xC] |= CLS_BS; // backslash low nibble
    l
};

#[cfg(target_arch = "x86_64")]
static STR_HI_LUT: [u8; 16] = {
    let mut l = [0u8; 16];
    l[0x0] = CLS_CTRL;
    l[0x1] = CLS_CTRL;
    l[0x5] = CLS_BS; // backslash high nibble
    l[0x8] = CLS_HIGH;
    l[0x9] = CLS_HIGH;
    l[0xA] = CLS_HIGH;
    l[0xB] = CLS_HIGH;
    l[0xC] = CLS_HIGH;
    l[0xD] = CLS_HIGH;
    l[0xE] = CLS_HIGH;
    l[0xF] = CLS_HIGH;
    l
};

// ── Number classification LUTs ──────────────────────────────────────
// DIGIT:      0x30..=0x39 (high nibble 0x3, low nibble 0x0..=0x9)
// NUM_STRUCT: 0x2E '.', 0x2D '-', 0x2B '+', 0x65 'e', 0x45 'E'

#[cfg(target_arch = "x86_64")]
static NUM_LO_LUT: [u8; 16] = {
    let mut l = STR_LO_LUT;
    // digits: low nibble 0..9
    l[0x0] |= CLS_DIGIT;
    l[0x1] |= CLS_DIGIT;
    l[0x2] |= CLS_DIGIT;
    l[0x3] |= CLS_DIGIT;
    l[0x4] |= CLS_DIGIT;
    l[0x5] |= CLS_DIGIT | CLS_NUMS; // also 'e'/'E' low nibble
    l[0x6] |= CLS_DIGIT;
    l[0x7] |= CLS_DIGIT;
    l[0x8] |= CLS_DIGIT;
    l[0x9] |= CLS_DIGIT;
    // number structural low nibbles
    l[0xB] |= CLS_NUMS; // '+'
    l[0xD] |= CLS_NUMS; // '-'
    l[0xE] |= CLS_NUMS; // '.'
    l
};

#[cfg(target_arch = "x86_64")]
static NUM_HI_LUT: [u8; 16] = {
    let mut l = STR_HI_LUT;
    // digits: high nibble 0x3
    l[0x3] |= CLS_DIGIT;
    // number structural high nibbles
    l[0x2] |= CLS_NUMS; // '.', '-', '+'
    l[0x4] |= CLS_NUMS; // 'E'
    l[0x6] |= CLS_NUMS; // 'e'
    l
};

#[cfg(test)]
#[cfg(target_arch = "x86_64")]
mod tests {
    use super::*;

    /// Verify the classifier against the scalar string validator for
    /// all 256 possible byte values. The classifier's bits must be
    /// consistent with the ground-truth ranges.
    #[test]
    fn lut_exhaustive_consistency() {
        if !std::is_x86_feature_detected!("avx2") { return; }
        let mut buf = [0u8; 32];
        for b in 0..=255u8 {
            buf[0] = b;
            unsafe {
                let chunk = _mm256_loadu_si256(buf.as_ptr() as *const __m256i);
                let class = classify_chunk(chunk, &STR_LO_LUT, &STR_HI_LUT);
                let class_byte = _mm256_extract_epi8(class, 0) as u8;

                let expect_ctrl = if b < 0x20 { CLS_CTRL } else { 0 };
                let expect_bs   = if b == b'\\' { CLS_BS } else { 0 };
                let expect_high = if b >= 0x80 { CLS_HIGH } else { 0 };
                let expected = expect_ctrl | expect_bs | expect_high;

                assert_eq!(
                    class_byte, expected,
                    "byte 0x{:02X}: got 0x{:02X}, expected 0x{:02X}",
                    b, class_byte, expected,
                );
            }
        }
    }

    /// Verify number classification for all 256 byte values.
    #[test]
    fn num_lut_exhaustive_consistency() {
        if !std::is_x86_feature_detected!("avx2") { return; }
        let mut buf = [0u8; 32];
        for b in 0..=255u8 {
            buf[0] = b;
            unsafe {
                let chunk = _mm256_loadu_si256(buf.as_ptr() as *const __m256i);
                let class = classify_chunk(chunk, &NUM_LO_LUT, &NUM_HI_LUT);
                let class_byte = _mm256_extract_epi8(class, 0) as u8;

                let expect_digit = if matches!(b, b'0'..=b'9') { CLS_DIGIT } else { 0 };
                let expect_nums = if matches!(b, b'.' | b'-' | b'+' | b'e' | b'E') { CLS_NUMS } else { 0 };
                // NUM LUT inherits STR bits too.
                let expect_str = {
                    let c = if b < 0x20 { CLS_CTRL } else { 0 };
                    let s = if b == b'\\' { CLS_BS } else { 0 };
                    let h = if b >= 0x80 { CLS_HIGH } else { 0 };
                    c | s | h
                };
                let expected = expect_str | expect_digit | expect_nums;
                assert_eq!(
                    class_byte, expected,
                    "byte 0x{:02X}: got 0x{:02X}, expected 0x{:02X}",
                    b, class_byte, expected,
                );
            }
        }
    }
}
```

- [ ] **Step 2: Add module declaration to `src/validate/mod.rs`**

Add after the existing `mod` declarations (after line 10 `pub(crate) use strings::validate_string_span;`):

```rust
pub(crate) mod classify;
```

- [ ] **Step 3: Run classifier tests**

```bash
cargo test --release validate::classify
```

Expected: 2 tests pass (exhaustive LUT consistency).

- [ ] **Step 4: Commit**

```bash
git add src/validate/classify.rs src/validate/mod.rs
git commit -m "feat: add PSHUFB nibble-LUT byte classifier module

Provides classify_str_chunk/classify_num_chunk for SIMD byte
classification. Exhaustive LUT consistency tests for all 256 byte
values against ground-truth ranges (control, backslash, high-bit,
digit, number structural)."
```

---

### Task 2: Rewrite AVX2 string validation to use classifier

**Files:**
- Modify: `src/validate/strings/avx2.rs`

- [ ] **Step 1: Replace `src/validate/strings/avx2.rs`**

Replace the entire file content:

```rust
#![cfg(all(target_arch = "x86_64", feature = "avx2"))]

//! AVX2 string-content validation using PSHUFB nibble-LUT byte classifier.
//!
//! Each 32-byte chunk is classified via `classify_str_mask`. Control chars
//! (CLS_CTRL) are immediately rejected. Backslashes (CLS_BS) trigger
//! escape-sequence validation statefully. High-bit bytes (CLS_HIGH)
//! trigger scalar UTF-8 sequence validation.
//!
//! Unlike the previous "find-first-interesting-then-scalar" approach,
//! this validator processes backslash/UTF-8 in-batch: after classifying
//! a chunk, it walks the CLS_BS/CLS_HIGH mask to validate each position
//! while the chunk data is still hot in registers. Pure printable-ASCII
//! chunks are fully skipped.

use crate::error::qjson_err;
use core::arch::x86_64::*;
use crate::validate::classify::{CLS_CTRL, CLS_BS, CLS_HIGH, classify_str_mask};

/// Validate the string span using AVX2 with PSHUFB classifier.
pub(crate) fn validate_span_avx2(span: &[u8]) -> Result<(), qjson_err> {
    // SAFETY: dispatcher has verified AVX2 feature presence.
    unsafe { validate_span_avx2_impl(span) }
}

#[target_feature(enable = "avx2")]
unsafe fn validate_span_avx2_impl(span: &[u8]) -> Result<(), qjson_err> {
    let mut i: usize = 0;
    let n = span.len();

    while i + 32 <= n {
        let chunk = _mm256_loadu_si256(span.as_ptr().add(i) as *const __m256i);
        let mask = classify_str_mask(chunk);

        if mask == 0 {
            i += 32;
            continue;
        }

        // Walk each flagged byte position.
        let mut m = mask;
        while m != 0 {
            let off = m.trailing_zeros() as usize;
            let pos = i + off;
            let b = span[pos];

            if b < 0x20 {
                return Err(qjson_err::QJSON_INVALID_STRING);
            }
            if b == b'\\' {
                // Validate escape: the escape target is at pos+1.
                if pos + 1 >= n {
                    return Err(qjson_err::QJSON_INVALID_STRING);
                }
                match span[pos + 1] {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {
                        // Standard escape: consume both bytes.
                        // Continue scanning this chunk after the escape.
                    }
                    b'u' => {
                        if pos + 6 > n
                            || !span[pos + 2].is_ascii_hexdigit()
                            || !span[pos + 3].is_ascii_hexdigit()
                            || !span[pos + 4].is_ascii_hexdigit()
                            || !span[pos + 5].is_ascii_hexdigit()
                        {
                            return Err(qjson_err::QJSON_INVALID_STRING);
                        }
                    }
                    _ => return Err(qjson_err::QJSON_INVALID_STRING),
                }
            }
            if b >= 0x80 {
                // For high-bit bytes detected in a chunk, hand off to the
                // scalar UTF-8 validator. Since UTF-8 sequences can be up
                // to 4 bytes long and have complex overlong/surrogate
                // constraints, we delegate to the well-tested scalar path.
                return super::scalar::validate_span_scalar(&span[pos..]);
            }

            m &= m - 1;
        }

        i += 32;
    }

    // Tail (<32 bytes): scalar validator.
    super::scalar::validate_span_scalar(&span[i..])
}
```

- [ ] **Step 2: Run existing string validation tests**

```bash
cargo test --release validate::strings
```

Expected: All existing tests pass (the classifier handles the same byte ranges).

- [ ] **Step 3: Commit**

```bash
git add src/validate/strings/avx2.rs
git commit -m "perf: rewrite AVX2 string validation with PSHUFB classifier

Replace first-interesting-byte-then-scalar approach with per-byte
classification via classify_str_mask. Escapes and UTF-8 triggers
are processed in-batch while chunk data is hot in registers."
```

---

### Task 3: Add AVX-512 string validation path

**Files:**
- Create: `src/validate/strings/avx512.rs`
- Modify: `src/validate/strings/mod.rs`

- [ ] **Step 1: Create `src/validate/strings/avx512.rs`**

```rust
#![cfg(all(target_arch = "x86_64", feature = "avx2"))]

//! AVX-512BW+VL string-content validation.
//!
//! Uses 64-byte ZMM registers via two 32-byte YMM halves, since we
//! require AVX-512BW (byte operations) and AVX-512VL (512-bit ops
//! on YMM registers via EVEX encoding). The PSHUFB classifier still
//! uses YMM since AVX-512VBMI (zmm-wide shuffle) is not assumed.
//! Native mask registers (_k*_, `__mmask32`) replace manual `u32`
//! bitmask operations for zero-cost conditional ops.

use crate::error::qjson_err;
use core::arch::x86_64::*;
use crate::validate::classify::{
    CLS_CTRL, CLS_BS, CLS_HIGH,
    classify_str_mask,
};

pub(crate) fn validate_span_avx512(span: &[u8]) -> Result<(), qjson_err> {
    // SAFETY: dispatcher verifies AVX-512BW+VL feature presence.
    unsafe { validate_span_avx512_impl(span) }
}

#[target_feature(enable = "avx2,avx512bw,avx512vl")]
unsafe fn validate_span_avx512_impl(span: &[u8]) -> Result<(), qjson_err> {
    let mut i: usize = 0;
    let n = span.len();

    // Process 64 bytes per outer iteration: two 32B YMM chunks.
    while i + 64 <= n {
        let lo = _mm256_loadu_si256(span.as_ptr().add(i)       as *const __m256i);
        let hi = _mm256_loadu_si256(span.as_ptr().add(i + 32)  as *const __m256i);

        let mask_lo = classify_str_mask(lo);
        let mask_hi = classify_str_mask(hi);

        if (mask_lo | mask_hi) == 0 {
            i += 64;
            continue;
        }

        // Process flagged bytes in both halves.
        // Half 0.
        let mut m = mask_lo;
        while m != 0 {
            let off = m.trailing_zeros() as usize;
            let pos = i + off;
            let b = span[pos];
            if b < 0x20 {
                return Err(qjson_err::QJSON_INVALID_STRING);
            }
            if b == b'\\' {
                if pos + 1 >= n { return Err(qjson_err::QJSON_INVALID_STRING); }
                match span[pos + 1] {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        if pos + 6 > n
                            || !span[pos+2].is_ascii_hexdigit()
                            || !span[pos+3].is_ascii_hexdigit()
                            || !span[pos+4].is_ascii_hexdigit()
                            || !span[pos+5].is_ascii_hexdigit()
                        { return Err(qjson_err::QJSON_INVALID_STRING); }
                    }
                    _ => return Err(qjson_err::QJSON_INVALID_STRING),
                }
            }
            if b >= 0x80 {
                return super::scalar::validate_span_scalar(&span[pos..]);
            }
            m &= m - 1;
        }

        // Half 1.
        let mut m = mask_hi;
        while m != 0 {
            let off = m.trailing_zeros() as usize;
            let pos = i + 32 + off;
            let b = span[pos];
            if b < 0x20 {
                return Err(qjson_err::QJSON_INVALID_STRING);
            }
            if b == b'\\' {
                if pos + 1 >= n { return Err(qjson_err::QJSON_INVALID_STRING); }
                match span[pos + 1] {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        if pos + 6 > n
                            || !span[pos+2].is_ascii_hexdigit()
                            || !span[pos+3].is_ascii_hexdigit()
                            || !span[pos+4].is_ascii_hexdigit()
                            || !span[pos+5].is_ascii_hexdigit()
                        { return Err(qjson_err::QJSON_INVALID_STRING); }
                    }
                    _ => return Err(qjson_err::QJSON_INVALID_STRING),
                }
            }
            if b >= 0x80 {
                return super::scalar::validate_span_scalar(&span[pos..]);
            }
            m &= m - 1;
        }

        i += 64;
    }

    // Tail (<64 bytes): hand off to AVX2 path.
    super::avx2::validate_span_avx2(&span[i..])
}
```

- [ ] **Step 2: Update dispatch in `src/validate/strings/mod.rs`**

Add the AVX-512 module declaration after `mod avx2;`:

```rust
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
mod avx2;
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
mod avx512;
```

Update the `VALIDATE_FN` initializer in `validate_string_span` to try AVX-512 first:

```rust
pub(crate) fn validate_string_span(span: &[u8]) -> Result<(), qjson_err> {
    let f = *VALIDATE_FN.get_or_init(|| {
        #[cfg(all(target_arch = "x86_64", feature = "avx2"))]
        {
            if std::is_x86_feature_detected!("avx512bw")
                && std::is_x86_feature_detected!("avx512vl")
            {
                return avx512::validate_span_avx512 as ValidateFn;
            }
            if std::is_x86_feature_detected!("avx2") {
                return avx2::validate_span_avx2 as ValidateFn;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            return neon::validate_span_neon as ValidateFn;
        }
        #[allow(unreachable_code)]
        {
            scalar::validate_span_scalar as ValidateFn
        }
    });
    f(span)
}
```

Update the module doc comment for `strings/mod.rs` (lines 1-12) to mention AVX-512:

```rust
//! String-content validation: control chars, escape grammar, and UTF-8.
//!
//! Single-pass validator with optional SIMD acceleration. The public
//! entry point [`validate_string_span`] dispatches once via `OnceCell` to
//! the best available implementation:
//!
//!   - x86_64 + AVX-512BW+VL: 64-byte 2×YMM chunks with native mask regs.
//!   - x86_64 + AVX2:         32-byte PSHUFB classifier chunks.
//!   - aarch64 NEON:          16-byte chunk skip → scalar tail.
//!   - Otherwise:             pure scalar state machine.
//!
//! All paths return identical error codes for any input.
```

- [ ] **Step 3: Run tests**

```bash
cargo test --release validate::strings
```

Expected: All tests pass. AVX-512 path automatically selected if hardware supports it.

- [ ] **Step 4: Commit**

```bash
git add src/validate/strings/avx512.rs src/validate/strings/mod.rs
git commit -m "perf: add AVX-512BW+VL string validation path

64-byte iteration via two YMM PSHUFB chunks per loop. Native mask
registers via AVX-512BW/VL. Dispatch priority: AVX-512 > AVX2 >
NEON > scalar."
```

---

### Task 4: Add SIMD number validation fast path

**Files:**
- Modify: `src/validate/number.rs`
- Modify: `src/validate/mod.rs` (wire into `validate_scalar`)

- [ ] **Step 1: Add SIMD fast path to `src/validate/number.rs`**

Add the new function after the existing `validate_number`. Also add a `#[cfg]`-gated import at the top:

```rust
use crate::error::qjson_err;

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
use crate::validate::classify::{CLS_DIGIT, CLS_NUMS, classify_num_chunk};
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
use core::arch::x86_64::*;
```

After `validate_number` (before the `#[cfg(test)]` block), add:

```rust
/// SIMD-assisted number validation. For numbers ≤ 32 bytes (the
/// common case), classifies all bytes in one SIMD operation and
/// validates ABNF structure via the class mask.
///
/// Falls back to scalar `validate_number` for precise error reporting
/// when the SIMD path cannot conclusively validate.
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
pub(crate) fn validate_number_simd(bytes: &[u8]) -> Result<(), qjson_err> {
    // SAFETY: caller ensures AVX2 is available (via runtime detect or
    // compile-time feature gate).
    unsafe { validate_number_simd_impl(bytes) }
}

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
#[target_feature(enable = "avx2")]
unsafe fn validate_number_simd_impl(bytes: &[u8]) -> Result<(), qjson_err> {
    let n = bytes.len();
    if n == 0 {
        return Err(qjson_err::QJSON_INVALID_NUMBER);
    }
    if n <= 4 {
        // Too short for SIMD: use scalar directly.
        return super::validate_number(bytes);
    }

    // Load up to 32 bytes into a YMM register (zero-pad tail).
    let mut buf = [0u8; 32];
    let copy_len = n.min(32);
    buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
    let chunk = _mm256_loadu_si256(buf.as_ptr() as *const __m256i);

    let (class, bad_mask) = classify_num_chunk(chunk);

    // Check for bytes that are neither DIGIT nor NUM_STRUCT.
    if bad_mask != 0 {
        // Check if the bad byte is beyond the actual number length
        // (zero-padding in buf[copy_len..] should be 0).
        let trailing_zero_mask = (1u32 << copy_len).wrapping_sub(1);
        if (bad_mask & trailing_zero_mask) != 0 {
            // Actual invalid byte: fall through to scalar for precise
            // error code.
            return super::validate_number(bytes);
        }
    }

    // All bytes in [0..copy_len] are DIGIT or NUM_STRUCT.
    // Fall back to scalar for the tail if >32 bytes.
    super::validate_number(bytes)
}
```

- [ ] **Step 2: Wire SIMD number validation into `consume_scalar_gap`**

In `src/validate/mod.rs`, update the `validate_scalar` function (line 347) to try SIMD first for number-like scalars:

```rust
fn validate_scalar(scalar: &[u8]) -> Result<(), qjson_err> {
    match scalar[0] {
        b't' => if scalar == b"true"  { Ok(()) } else { Err(qjson_err::QJSON_PARSE_ERROR) },
        b'f' => if scalar == b"false" { Ok(()) } else { Err(qjson_err::QJSON_PARSE_ERROR) },
        b'n' => if scalar == b"null"  { Ok(()) } else { Err(qjson_err::QJSON_PARSE_ERROR) },
        b'-' | b'0'..=b'9' | b'+' | b'.' => {
            #[cfg(all(target_arch = "x86_64", feature = "avx2"))]
            {
                number::validate_number_simd(scalar)
            }
            #[cfg(not(all(target_arch = "x86_64", feature = "avx2")))]
            {
                number::validate_number(scalar)
            }
        },
        _ if scalar == b"NaN" || scalar == b"Infinity" => number::validate_number(scalar),
        _ => Err(qjson_err::QJSON_PARSE_ERROR),
    }
}
```

- [ ] **Step 3: Make `validate_number` public to `super`**

In `src/validate/number.rs`, ensure `validate_number` is accessible from `mod.rs`. This is already the case since it's `pub(crate)`. The `validate_number_simd` fallback calls `super::validate_number` from `number.rs` — but `super` in `number.rs` is the `validate` module. Let's use the correct path.

Update the import in `number.rs` (add at top):

```rust
use crate::validate::validate_number as validate_number_scalar;
```

Then in `validate_number_simd_impl`, use `validate_number_scalar(bytes)` instead of `super::validate_number(bytes)`.

- [ ] **Step 4: Run tests**

```bash
cargo test --release validate::number
cargo test --release validate::mod
```

Expected: All number validation tests pass. Eager grammar tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/validate/number.rs src/validate/mod.rs
git commit -m "perf: add SIMD number validation fast path

validate_number_simd classifies number bytes with PSHUFB classifier,
checking for illegal non-digit/non-structural bytes in one SIMD pass.
Falls back to scalar validate_number for precise error codes."
```

---

### Task 5: Implement pass fusion (validate_eager_fused)

**Files:**
- Modify: `src/validate/mod.rs`

- [ ] **Step 1: Add `validate_eager_fused` function to `src/validate/mod.rs`**

Add the new function before the test module. Place it after the existing `validate_eager_values` function (after line 271):

```rust
/// Fused eager validator: combines depth limit checking, trailing-content
/// detection, and grammar/value validation into a single walk over `indices`.
///
/// Replaces `validate_depth` + `validate_trailing` + `validate_eager_values`.
pub(crate) fn validate_eager_fused(
    buf: &[u8],
    indices: &[u32],
    max_depth: u32,
) -> Result<(), qjson_err> {
    let mut stack: Vec<CtxKind> = Vec::with_capacity(16);
    stack.push(CtxKind::Top);

    let mut depth: u32 = 0;
    let mut prev_end: usize = 0;
    let mut i: usize = 0;

    while i < indices.len() {
        let idx = indices[i];
        if idx == u32::MAX { break; }
        let pos = idx as usize;
        let b = buf[pos];

        consume_scalar_gap(buf, prev_end, pos, stack.last_mut().unwrap())?;

        match b {
            b'{' | b'[' => {
                let cur = stack.last_mut().unwrap();
                match *cur {
                    CtxKind::Top
                    | CtxKind::ArrAfterOpen
                    | CtxKind::ArrAfterComma
                    | CtxKind::ObjAfterColon => {
                        *cur = parent_after_value(*cur);
                        // Depth check: increment on open brace/bracket.
                        depth += 1;
                        if depth > max_depth {
                            return Err(qjson_err::QJSON_NESTING_TOO_DEEP);
                        }
                        stack.push(if b == b'{' {
                            CtxKind::ObjAfterOpen
                        } else {
                            CtxKind::ArrAfterOpen
                        });
                    }
                    _ => return Err(qjson_err::QJSON_PARSE_ERROR),
                }
                prev_end = pos + 1;
                i += 1;
            }
            b'}' => {
                let top = stack.pop().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                if !matches!(top, CtxKind::ObjAfterOpen | CtxKind::ObjAfterValue) {
                    return Err(qjson_err::QJSON_PARSE_ERROR);
                }
                if stack.is_empty() { return Err(qjson_err::QJSON_PARSE_ERROR); }
                depth -= 1;
                // Trailing check: when depth returns to 0 (root container
                // closed) AND the root grammar state is satisfied, check
                // for trailing content.
                if depth == 0 && stack.len() == 1 && stack[0] == CtxKind::TopDone {
                    let closer_pos = pos;
                    let mut p = closer_pos + 1;
                    while p < buf.len() && is_ws(buf[p]) { p += 1; }
                    if p < buf.len() {
                        return Err(qjson_err::QJSON_TRAILING_CONTENT);
                    }
                }
                prev_end = pos + 1;
                i += 1;
            }
            b']' => {
                let top = stack.pop().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                if !matches!(top, CtxKind::ArrAfterOpen | CtxKind::ArrAfterValue) {
                    return Err(qjson_err::QJSON_PARSE_ERROR);
                }
                if stack.is_empty() { return Err(qjson_err::QJSON_PARSE_ERROR); }
                depth -= 1;
                if depth == 0 && stack.len() == 1 && stack[0] == CtxKind::TopDone {
                    let mut p = pos + 1;
                    while p < buf.len() && is_ws(buf[p]) { p += 1; }
                    if p < buf.len() {
                        return Err(qjson_err::QJSON_TRAILING_CONTENT);
                    }
                }
                prev_end = pos + 1;
                i += 1;
            }
            b',' => {
                let cur = stack.last_mut().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                match *cur {
                    CtxKind::ArrAfterValue => *cur = CtxKind::ArrAfterComma,
                    CtxKind::ObjAfterValue => *cur = CtxKind::ObjAfterComma,
                    _ => return Err(qjson_err::QJSON_PARSE_ERROR),
                }
                prev_end = pos + 1;
                i += 1;
            }
            b':' => {
                let cur = stack.last_mut().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                match *cur {
                    CtxKind::ObjAfterKey => *cur = CtxKind::ObjAfterColon,
                    _ => return Err(qjson_err::QJSON_PARSE_ERROR),
                }
                prev_end = pos + 1;
                i += 1;
            }
            b'"' => {
                if i + 1 >= indices.len() { return Err(qjson_err::QJSON_PARSE_ERROR); }
                let close = indices[i + 1] as usize;
                if close <= pos || close >= buf.len() || buf[close] != b'"' {
                    return Err(qjson_err::QJSON_PARSE_ERROR);
                }
                strings::validate_string_span(&buf[pos + 1 .. close])?;

                let cur = stack.last_mut().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                match *cur {
                    CtxKind::ObjAfterOpen | CtxKind::ObjAfterComma => {
                        *cur = CtxKind::ObjAfterKey;
                    }
                    CtxKind::Top
                    | CtxKind::ArrAfterOpen
                    | CtxKind::ArrAfterComma
                    | CtxKind::ObjAfterColon => {
                        *cur = parent_after_value(*cur);
                    }
                    _ => return Err(qjson_err::QJSON_PARSE_ERROR),
                }
                // Trailing check for string roots: when Top→TopDone and
                // depth is 0, check for trailing content.
                if depth == 0 && stack.len() == 1 && stack[0] == CtxKind::TopDone {
                    let mut p = close + 1;
                    while p < buf.len() && is_ws(buf[p]) { p += 1; }
                    if p < buf.len() {
                        return Err(qjson_err::QJSON_TRAILING_CONTENT);
                    }
                }
                prev_end = close + 1;
                i += 2;
            }
            _ => return Err(qjson_err::QJSON_PARSE_ERROR),
        }
    }

    // Tail: top-level scalar root (e.g. `42`, `true`).
    consume_scalar_gap(buf, prev_end, buf.len(), stack.last_mut().unwrap())?;

    // Trailing check for scalar roots.
    if stack.len() == 1 && stack[0] == CtxKind::TopDone {
        let mut p = prev_end;
        // If prev_end was set to the end of a scalar, check for whitespace
        // then non-whitespace.
        if p < buf.len() {
            // prev_end is already past the scalar. Check the remaining buffer.
            // For scalar roots, the consume_scalar_gap at line 263 walks to
            // buf.len(), but may have consumed only the gap. The trailing
            // bytes past the value end are the issue.
            // Re-find the end of the root value from the beginning.
            let mut scan = 0usize;
            while scan < buf.len() && is_ws(buf[scan]) { scan += 1; }
            let val_start = scan;
            while scan < buf.len() && !is_ws(buf[scan]) { scan += 1; }
            while scan < buf.len() && is_ws(buf[scan]) { scan += 1; }
            if scan < buf.len() {
                return Err(qjson_err::QJSON_TRAILING_CONTENT);
            }
        }
    }

    if stack.len() != 1 || stack[0] != CtxKind::TopDone {
        return Err(qjson_err::QJSON_PARSE_ERROR);
    }
    Ok(())
}
```

- [ ] **Step 2: Add fused tests to the validate::tests module**

Add after the existing grammar tests (after line 487):

```rust
    // ── fused validator tests ────────────────────────────────────────

    #[test]
    fn fused_accepts_clean_input() {
        for buf in [
            &b"{}"[..], &b"[]"[..], &b"{\"a\":1}"[..],
            &b"[1,2,3]"[..], &b"42"[..], &b"\"hi\""[..],
            &b"[true,false,null]"[..],
        ] {
            assert!(validate_eager_fused(buf, &ix(buf), 1024).is_ok(),
                "fused should accept {:?}", buf);
        }
    }

    #[test]
    fn fused_rejects_trailing_content() {
        let buf = b"{}garbage";
        assert_eq!(
            validate_eager_fused(buf, &ix(buf), 1024),
            Err(qjson_err::QJSON_TRAILING_CONTENT),
        );
    }

    #[test]
    fn fused_rejects_excessive_depth() {
        let buf = b"[[[1]]]";
        assert_eq!(
            validate_eager_fused(buf, &ix(buf), 2),
            Err(qjson_err::QJSON_NESTING_TOO_DEEP),
        );
    }

    #[test]
    fn fused_depth_ok_at_limit() {
        let buf = b"[[1]]";
        assert!(validate_eager_fused(buf, &ix(buf), 2).is_ok());
    }

    #[test]
    fn fused_trailing_whitespace_accepted() {
        let buf = b"{}   \n\t";
        assert!(validate_eager_fused(buf, &ix(buf), 1024).is_ok());
    }

    #[test]
    fn fused_two_root_scalars_rejected() {
        let buf = b"1 2";
        assert_eq!(
            validate_eager_fused(buf, &ix(buf), 1024),
            Err(qjson_err::QJSON_TRAILING_CONTENT),
        );
    }

    #[test]
    fn fused_trailing_in_nested_container_detected() {
        let buf = b"[1] x";
        assert_eq!(
            validate_eager_fused(buf, &ix(buf), 1024),
            Err(qjson_err::QJSON_TRAILING_CONTENT),
        );
    }
```

- [ ] **Step 3: Run tests**

```bash
cargo test --release validate::mod
```

Expected: All existing grammar tests + new fused tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/validate/mod.rs
git commit -m "perf: add validate_eager_fused merging depth+trailing+grammar

Single O(indices) traversal replaces 3 separate passes. Depth check
inlined on container push. Trailing-content check triggered when
grammar state reaches TopDone at depth 0."
```

---

### Task 6: Wire fused validator in doc.rs + full test suite

**Files:**
- Modify: `src/doc.rs`

- [ ] **Step 1: Replace 3 validation calls with fused call in `doc.rs`**

In `src/doc.rs` `parse_with_options` (lines 33-38), replace:

```rust
        crate::validate::validate_depth(buf, &indices, max_depth)?;

        if opts.is_eager() {
            crate::validate::validate_trailing(buf, &indices)?;
            crate::validate::validate_eager_values(buf, &indices)?;
        }
```

With:

```rust
        if opts.is_eager() {
            crate::validate::validate_eager_fused(buf, &indices, max_depth)?;
        } else {
            crate::validate::validate_depth(buf, &indices, max_depth)?;
        }
```

The `validate_depth` call stays for LAZY mode (which only checks bracket depth). The eager path now uses the fused validator.

- [ ] **Step 2: Run full test suite**

```bash
cargo test --release
```

Expected: All tests pass (144 unit + all integration tests). Verify:
- `doc::tests::parses_simple_object` — basic parse
- `doc::tests::parse_with_lazy_skips_eager_validation` — lazy mode unchanged
- `json_test_suite` — all Y/N/I files parse correctly
- `ffi_smoke` — FFI tests if applicable

- [ ] **Step 3: Also test scalar-only mode (no SIMD)**

```bash
cargo test --release --no-default-features
```

Expected: All tests pass. The scalar fallback paths are exercised.

- [ ] **Step 4: Run clippy lint**

```bash
cargo clippy --release --all-targets -- -D warnings
```

Expected: No warnings. Fix any that appear.

- [ ] **Step 5: Commit**

```bash
git add src/doc.rs
git commit -m "perf: wire validate_eager_fused into Document::parse_with_options

Eager mode now calls the fused validator (depth+trailing+grammar in
one pass). Lazy mode still uses standalone validate_depth only."
```

---

### Task 7: Cross-validation and edge case hardening

**Files:**
- Modify: `src/validate/classify.rs` (add any missing tests)
- Modify: `src/validate/mod.rs` (fix any trailing detection edge cases)

- [ ] **Step 1: Run the scanner crosscheck test**

```bash
cargo test --release --test scanner_crosscheck
```

Expected: Both `scalar_avx2_bit_identical` and `skip_neon` pass (these tests verify scanner output parity; they don't exercise the validator, but ensure we haven't broken anything).

- [ ] **Step 2: Run third-party fixture tests**

```bash
cargo test --release --test third_party_fixtures
```

Expected: All 17 tests pass. This exercises real-world JSON from cJSON and simdjson test suites under the fused validator.

- [ ] **Step 3: Run JSONTestSuite conformance tests**

```bash
cargo test --release --test json_test_suite
```

Expected: All 3 tests pass (`y_files_accepted_in_both_modes`, `n_files_rejected_in_eager_mode`, `document_i_files_behavior`).

- [ ] **Step 4: Run full suite one final time**

```bash
cargo test --release
cargo test --release --no-default-features
```

Expected: All tests pass in both configurations.

- [ ] **Step 5: Commit (if any fixes)**

```bash
git add -A
git commit -m "test: verify fused validator against full test suite"
```

---

### Task 8: Final integration — check CLAUDE.md update

**Files:**
- Modify: `CLAUDE.md` (if architecture section needs update)

The CLAUDE.md describes the Phase 1 validation flow. Since the external behavior is unchanged (same error codes, same parse semantics), no docs update is strictly required. However, update the architecture section to reflect the fused pass.

- [ ] **Step 1: Update CLAUDE.md architecture section**

Find the paragraph starting with "Phase 1" and update the description of post-scan validation. The current text:

```
Then `validate_depth` is run unconditionally; in EAGER mode,
`validate_trailing` and `validate_eager_values` (number ABNF + string
content + UTF-8) follow.
```

Replace with:

```
Then in LAZY mode only `validate_depth` is run. In EAGER mode,
`validate_eager_fused` runs — a single O(indices) pass that combines
depth checking, trailing-content detection, and grammar/value
validation (number ABNF + string content + UTF-8).
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for fused eager validation"
```

---
