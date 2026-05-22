# Fuse & Accelerate: Eager Decode SIMD Optimization

Date: 2026-05-22
Branch: `fuse-eager-passes`

## Motivation

The eager decode path (`Document::parse_with_options` in `src/doc.rs`) runs **4 independent passes** over the `indices` array after structural scanning:

1. `validate_depth` — depth counting
2. `validate_trailing` — reject trailing non-whitespace
3. `validate_eager_values` — grammar state machine + string validation + number validation

Each pass is a scalar O(indices) walk. Additionally, string validation SIMD (`strings/avx2.rs`) is conservative: it hands off to scalar on the *first* interesting byte found (backslash, control, or high-bit), leaving most of the SIMD register width unused on mixed content. Number validation has no SIMD path at all.

Target: ASCII-dominant JSON payloads (REST APIs, config files), x86-64 with AVX2 + AVX-512 runtime dispatch, throughput-focused.

## Architecture

### Pass Fusion

Merge three post-scan validation passes into one:

```
Before:
  scan(buf) → indices
  validate_depth(buf, indices, max_depth)
  validate_trailing(buf, indices)
  validate_eager_values(buf, indices)

After:
  scan(buf) → indices
  validate_eager_fused(buf, indices, max_depth)
```

`validate_eager_fused` integrates depth checking and trailing-content detection into the existing grammar state machine:

- **Depth**: increment on `{`/`[` push; if depth > max_depth → `QJSON_NESTING_TOO_DEEP`.
- **Trailing**: after the grammar state reaches `TopDone`, any further non-whitespace byte → `QJSON_TRAILING_CONTENT`.

The `CtxKind` enum and state-machine structure from `validate_eager_values` are preserved. The existing `validate_depth` and `validate_trailing` functions remain in the codebase but are no longer called in the eager hot path (they stay available for lazy mode or internal reuse).

### PSHUFB Byte Classifier for String Validation

Replace the current AVX2 "find-first-interesting-byte-then-scalar" approach with a **nibble-LUT byte classifier** using `_mm256_shuffle_epi8` (PSHUFB).

**Classification bits** (one u8 per byte):

| Bit | Meaning |
|-----|---------|
| 0   | Control char (0x00..0x1F) |
| 1   | Backslash (0x5C) |
| 2   | High-bit byte (0x80..0xFF) |
| 3   | Printable ASCII (0x20..0x7E, excluding backslash) |

**Algorithm per 32-byte chunk:**
1. Split each byte into high-nibble and low-nibble via shift + mask.
2. `_mm256_shuffle_epi8(lo_nibble, lo_lut)` and `_mm256_shuffle_epi8(hi_nibble, hi_lut)`.
3. AND low and high LUT results → per-byte class bitmask.
4. If any bit 0 set → `QJSON_INVALID_STRING` (control char).
5. If bits 1 and 2 are zero → pure printable ASCII, advance 32 bytes.
6. Otherwise: scan class bitmask for backslash positions, validate escape sequences; for high-bit bytes, run SIMD-enhanced UTF-8 validation.

Key improvement: the classifier tells us **exactly which bytes need what kind of attention**, rather than a binary "there's a problem here". Multiple backslashes in one chunk are all located without re-scanning. High-bit bytes are identified by position, enabling batch UTF-8 validation.

### AVX-512 Dual Path

New file `src/validate/strings/avx512.rs`, dispatched at runtime via the existing `OnceCell` pattern in `strings/mod.rs`.

| Feature | AVX2 | AVX-512 |
|---------|------|---------|
| Register width | 32B (ymm) | 64B (zmm) |
| Movemask | `_mm256_movemask_epi8` → u32 | `_mm256_movepi8_mask` (AVX512BW/VL) → `__mmask32`, zero-cost |
| Byte classifier | Two ymm PSHUFB per chunk | Two ymm PSHUFB per 32B half (AVX-512VBMI not required) |
| Masking | Manual `u32` bitmask | Native `__mmask32` with `_mm256_maskz_*` operations |
| Chunk throughput | 32B/iter | 64B/iter (loop processes two 32B halves) |

**Dispatch priority**: AVX-512 (Ice Lake 2019+, Zen 4 2022+) → AVX2 (Haswell 2013+) → scalar fallback.

**Not included**: AVX-512VBMI (`vpermb` for zmm-wide PSHUFB). This requires Cannon Lake/Ice Lake+ and the gain over loop-unrolled ymm PSHUFB is marginal for string validation.

### SIMD-Accelerated Number Validation

Extend the PSHUFB classifier with two additional bits:

| Bit | Meaning |
|-----|---------|
| 4   | Digit (0x30..0x39) |
| 5   | Number structural (0x2E `.`, 0x2D `-`, 0x65 `e`, 0x45 `E`, 0x2B `+`) |

**Hot path** for numbers in `consume_scalar_gap`:
1. Classify 32-byte chunk(s) of the number byte range.
2. `illegal = !(digit | structural)` — if mask is non-zero, scalar fallback handles exact error location.
3. Validate ABNF structure: leading zero check, digit-after-dot check, digit-after-exponent check — verified via popcount and bit-scan on the classification mask, falling back to the existing scalar `validate_number` for precise error codes when structure is violated.

When a number is short (≤32 bytes, i.e. the vast majority of real-world numbers), it fits in one SIMD iteration. The existing scalar `validate_number` remains as fallback for correctness and precise error reporting.

## Files Changed

| File | Change |
|------|--------|
| `src/validate/mod.rs` | Add `validate_eager_fused()` merging depth + trailing + grammar. Keep existing functions. |
| `src/validate/strings/avx2.rs` | Rewrite with PSHUFB nibble-LUT classifier. |
| `src/validate/strings/avx512.rs` | **New.** AVX-512BW+VL 64B chunk path. |
| `src/validate/strings/mod.rs` | Add AVX-512 to dispatch. |
| `src/validate/number.rs` | Add `validate_number_simd()` with PSHUFB classifier. |
| `src/doc.rs` | Replace 3 validate calls with single `validate_eager_fused`. |
| `Cargo.toml` | Optionally add `avx512` feature gate (feature name only; dispatch uses runtime detection). |

## Files NOT Changed

- `src/scan/` — structural scanner unchanged.
- `src/cursor.rs`, `src/path.rs` — Phase 2 unchanged.
- `src/decode/` — lazy decode unchanged (still calls `validate_string_span` which now uses the new SIMD paths transparently).
- `src/ffi.rs`, `lua/qjson.lua` — FFI surface unchanged.
- `include/qjson.h` — public header unchanged.

## Risks

1. **Error-code precedence.** When fused pass encounters multiple errors simultaneously (e.g., depth violation AND invalid string), current behavior picks the first detected. The fused pass must preserve this.
2. **AVX-512 dispatch stability.** Some VM/hypervisor configurations mask AVX-512 CPUID bits inconsistently. The existing `is_x86_feature_detected!()` pattern is proven safe for this.
3. **PSHUFB LUT correctness.** The 16-entry nibble LUTs must be exhaustively verified against the existing scalar validator for all 256 byte values. This is done in unit tests.

## Expected Performance Impact

- **Pass fusion**: ~15-25% throughput improvement for small-to-medium payloads (eliminates 2 full indices traversals).
- **PSHUFB string validation**: ~20-40% improvement for string-heavy payloads (no premature scalar fallback; CJK/escape content benefits most).
- **AVX-512 string validation**: ~10-15% additional improvement over AVX2 (2× chunk width, native mask registers).
- **SIMD number validation**: ~10-20% improvement for number-dense payloads (arrays of numbers, metrics responses).

Combined estimate: **30-50%** throughput improvement on typical REST API payloads.
