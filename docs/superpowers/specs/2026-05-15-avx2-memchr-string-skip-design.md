# AVX2 scanner: cheaper in-string fast path

**Status**: design approved, ready for implementation plan
**Issue**: [#5 perf(scan): memchr-based fast path for in-string content](https://github.com/membphis/lua-quick-decode/issues/5)
**Touches**: `src/scan/avx2.rs`, `benches/lua_bench.lua`, `README.md` (Roadmap / Deferred)

## Problem

The AVX2 scanner's current in-string fast path (`src/scan/avx2.rs:34-43`, added in PR #3) detects when a 64-byte chunk lies fully inside a string and skips the structural-mask + PCLMUL prefix-XOR work. The condition is `in_string != 0 && real_quote == 0`, which still requires computing both the backslash mask and the escape mask before it can fire.

Per-chunk cost when the current fast path *fires*:

- 2 × `loadu` (free, needed for any path)
- `backslash` byte mask: ~6 ops
- `quote` byte mask: ~6 ops
- `find_escape_mask_with_carry`: ~10 scalar ALU ops + several branches
- final `real_quote == 0` test

≈ 25 ops per "skip" chunk. On string-heavy payloads — e.g. a multimodal-shaped JSON whose `data` field is ~10 MB of base64 — ~95% of chunks hit this path, making it the dominant scanner cost.

## Goal

Lower per-chunk cost on string-interior chunks from ~25 ops to ~10 ops, by replacing the current fast-path *condition* with a cheaper probe that detects "chunk has no `"` and no `\`" directly, before computing the escape mask.

Estimated speedup on a 10 MB string-heavy payload: ~3× scan-phase throughput (op-count analysis; the implementation will validate via `make bench` against a synthetic fixture).

This proposal is the chunk-granularity step (Option 1 in brainstorming). Cross-chunk `memchr2` jumps for very long string interiors are deferred (see Roadmap / Deferred).

## Non-goals

- Touching the scalar scanner (`src/scan/scalar.rs`). The hot path for the targeted workloads is the AVX2 backend.
- Changing validation semantics. Every byte still gets scanned for well-formedness; bracket balance still validated at end.
- Adding a new cargo feature. The change rides on the existing `avx2` feature.
- Cross-chunk jumps (`memchr2` jump path). Deferred — see Roadmap / Deferred.

## Design

### Code change

Single file: `src/scan/avx2.rs::scan_avx2_impl`. The chunk loop body becomes:

```rust
while i + 64 <= buf.len() {
    let chunk_lo = _mm256_loadu_si256(buf.as_ptr().add(i)      as *const __m256i);
    let chunk_hi = _mm256_loadu_si256(buf.as_ptr().add(i + 32) as *const __m256i);

    // in_string fast-probe: only enter when previous chunk left us inside
    // a string. Cheap quote-or-backslash mask; if zero, the chunk is pure
    // string interior and we can skip ALL mask computation including the
    // escape-run scan.
    if in_string != 0 {
        let interesting = quote_or_backslash_mask(chunk_lo, chunk_hi);
        if interesting == 0 {
            // No `"` or `\` in chunk → no escapes can originate here, so
            // bs_carry must be 0 leaving this chunk. in_string stays 1.
            bs_carry = 0;
            i += 64;
            continue;
        }
    }

    // Slow path unchanged below.
    let backslash = byte_mask(chunk_lo, chunk_hi, b'\\');
    let quote     = byte_mask(chunk_lo, chunk_hi, b'"');
    let escaped   = find_escape_mask_with_carry(backslash, &mut bs_carry);
    let real_quote = quote & !escaped;

    let (inside, new_in_string) = inside_string_mask(real_quote, in_string);
    in_string = new_in_string;

    let struct_mask = structural_mask_chunk(chunk_lo, chunk_hi);
    let final_mask = (struct_mask & !inside) | real_quote;

    emit_bits(final_mask, i as u32, out);

    i += 64;
}
```

The current fast-path branch (`if in_string != 0 && real_quote == 0 { i += 64; continue; }`) is **removed** — the new probe is a true subset of its trigger condition (proof in §"Correctness"), so removing the late fast path costs nothing and the code reads more linearly.

### New helper

```rust
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
```

Matches the style of existing helpers (`byte_mask`, `structural_mask_chunk`): `#[inline(always)] unsafe fn` with no explicit `#[target_feature]` annotation — the caller `scan_avx2_impl` carries `#[target_feature(enable = "avx2,pclmulqdq")]` and inlining propagates the feature set.

Op count: 4 `cmpeq` + 2 `or` + 2 `movemask` + 1 shift + 1 or = ~10 vector ops, no scalar ALU, no branches.

### Op-count comparison

| chunk shape | current path | new path | delta |
|---|---|---|---|
| not in_string | full mask path (~25 ops, no fast path) | unchanged | 0 |
| in_string, chunk pure string interior | ~25 ops (current fast path) | ~10 ops (new probe) | **−60%** |
| in_string, chunk has `\` or `"` | ~25 ops slow path | ~10 ops probe + ~25 slow = ~35 | +40% |

Net effect on a 10 MB base64-style payload (~95% pure-interior chunks): probe-hit case dominates; expected ~3× scan throughput. Mixed payloads with frequent escapes inside strings see a smaller win or slight regression on the in-string-with-escapes chunks; bench will measure the crossover.

## Correctness

The new fast path fires when `in_string == 1 ∧ chunk contains no '"' and no '\'`. We must prove that taking the branch (skip 64 bytes, set `bs_carry = 0`, keep `in_string = 1`) produces output identical to letting the slow path run.

### (a) `bs_carry` leaves the chunk as 0

`bs_carry` represents whether the trailing backslash run of the current chunk has odd parity (and thus escapes byte 0 of the next chunk). With `backslash == 0`:

- `trailing_bs = 0` in `find_escape_mask_with_carry`
- Falls into the `else` branch: `new_carry = 0 & 1 = 0`

So slow-path `bs_carry` after this chunk is 0, regardless of incoming `bs_carry`. Setting it to 0 explicitly is equivalent.

### (b) `in_string` stays 1

With `real_quote == 0` (which follows from `quote == 0`), `inside_string_mask` computes:

- `q = 0`, prefix-XOR via `_mm_clmulepi64_si128` = 0
- If `prev_in_string != 0`, `mask = !0 = u64::MAX`
- `new_state = (u64::MAX >> 63) & 1 = 1`

Slow path leaves `in_string = 1`. Explicit retention is equivalent.

### (c) No structural offsets are emitted for this chunk

Slow path: `final_mask = (struct_mask & !inside) | real_quote`. With the whole chunk inside the string (`inside = u64::MAX`) and `real_quote = 0`, `final_mask = 0`. Zero offsets emitted. Skipping the chunk emits nothing. Equivalent.

### (d) New condition is strictly narrower than current fast path

Current condition `in_string != 0 ∧ real_quote == 0` fires when `quote & !escaped == 0`. New condition fires when `quote == 0 ∧ backslash == 0`. The new condition implies `quote == 0 ⇒ real_quote == 0`, so any chunk hit by the new path was also hit by the current fast path. The reverse is not true: a chunk with `quote != 0` where every quote bit is escaped (preceded by an odd backslash run) hits the current fast path but not the new one. Those chunks now go through the slow path — correctness unchanged, performance unchanged (slow path is the same code).

### Edge cases

| scenario | behavior |
|---|---|
| Entering chunk with `bs_carry == 1`, chunk byte 0 is `\` | `backslash != 0` → probe miss → slow path → `pc=1` handled by `find_escape_mask_with_carry` as before |
| Entering chunk with `bs_carry == 1`, chunk has no `"` or `\` | Probe hit → `bs_carry := 0`, equivalent to slow path's `else` branch returning `new_carry = 0` |
| 64-aligned input ending mid-string | Unchanged — main loop exits with `i == buf.len()`, existing post-loop `if i < buf.len() ... else if in_string != 0 { return Err(buf.len()) }` still flags unterminated |
| Non-aligned tail with `bs_carry=1` from probe-hit chunk | `bs_carry = 0` after probe hit, so `scalar_start = i` (existing logic), correct |

## Bench fixture

`benches/lua_bench.lua` gains a synthetic "string-heavy" scenario. **Fixture is generated at run time, not committed.**

- Top-level shape: `{"id": "...", "ts": <int>, "data": "<base64-ish>"}`
- `data` value: `QJD_BENCH_BIG_MB` MB (default 10) of characters drawn from `A-Za-z0-9+/`. Guaranteed no `"` or `\` in the payload. Deterministic seed for reproducibility.
- Bench reports fixture size + three-run median for:
  - `lua-cjson` full parse
  - `quickdecode` parse + single-field extract on `data`

Bench is a manual `make bench` target. **Not a CI gate.** Its output goes into the PR description and a Performance section update in `README.md`.

## Tests

Rust unit tests in `src/scan/avx2.rs::tests`. The host-AVX2 guard pattern (`if !host_supports_avx2() { return; }`) is preserved.

| test | new / modified | purpose |
|---|---|---|
| `long_string_engages_skip_fastpath` | modified | bump from ~10 KB to ≥1 MB string interior — multiple probe-hit chunks in a row |
| `long_string_with_periodic_backslash` | **new** | every ~5 chunks inject `\\n` / `\\\"` escape sequences; alternates probe-hit and slow path, asserts parity with scalar |
| `bs_carry_one_at_pure_string_chunk_boundary` | **new** | construct prior chunk ending in odd-length backslash run (`bs_carry=1`), next chunk fully pure string interior with no `"`/`\`; assert parity (verifies §(a)) |
| `escaped_quotes_remain_correct_with_fastpath` | unchanged | existing test, still passes |
| `scanner_crosscheck` (proptest, `tests/scanner_crosscheck.rs`) | unchanged | 2000-case property test; if shrinking finds a regression case, `.proptest-regressions` gets committed |

## CI matrix

Unchanged. No new cargo features, no new test binaries.

1. `cargo test --release` — exercises new path (host AVX2 required)
2. `cargo test --release --no-default-features` — scalar-only, new code excluded by `#![cfg(target_arch = "x86_64")]` + feature gate
3. `cargo test --features test-panic --release` — FFI panic barrier unchanged
4. Lua busted suite under LuaJIT — unchanged

## Roadmap / Deferred

After landing, add to `README.md` under Roadmap / Deferred:

> - **memchr2 jump for ≥N consecutive in-string chunks** — current chunk-per-chunk probe leaves ~10 vector ops/chunk on the table for very large string-interior runs (≥1 MB single string). A `memchr2(b'"', b'\\')` jump path can approach memory bandwidth; deferred until a workload that benefits clearly emerges.

## Out of scope

- Scalar scanner changes.
- Auto-tuning the probe threshold or making the probe optional.
- Reworking `find_escape_mask_with_carry` (its cost is paid only on slow-path chunks now).
- Cross-chunk `memchr2` jumps (Option 2 from brainstorming; tracked in Roadmap).
