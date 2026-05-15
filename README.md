# lua-quick-decode

Rust-implemented fast JSON decoder exposed to LuaJIT via FFI. Optimized for the common case where a large JSON is parsed once and only a small number of fields are extracted before the document is discarded.

Design document: `docs/superpowers/specs/2026-05-15-rust-quick-json-decode-design.md`.

## Status

Initial implementation complete: scalar + AVX2/PCLMUL structural scanner, root-path and cursor APIs, escape-decoded strings, integer/float/bool/typeof/len, FFI panic barrier, and a LuaJIT wrapper. Rust unit/integration tests and Lua busted tests run in CI. The benchmark harness compares against lua-cjson but tuning is pending — see `Roadmap / Deferred` below.

## Building

```sh
cargo build --release
# Output: target/release/libquickdecode.so
```

A `Makefile` wraps the common workflows; run `make help` to see `build`, `test`, `lint`, `bench`, and `clean` targets. Override `LUAJIT` / `LUA_CPATH` per invocation if your environment differs from the defaults.

## Testing

```sh
cargo test
```

## LuaJIT Usage

```lua
local qd = require("quickdecode")
local doc = qd.parse(json_str)

-- Root-path getter:
local model = doc:get_str("body.model")

-- Cursor (avoid re-walking shared prefix):
local body = doc:open("body")
local model = body:get_str("model")
local temp  = body:get_f64("temperature")
```

### Reusable decoder (pooled API)

For hot paths that parse many payloads (typical in OpenResty workers), use a
reusable decoder to amortize the per-parse indices / scratch / skip-cache
allocations:

```lua
local decoder = qd.new_decoder()         -- one per worker is enough
for _, payload in ipairs(payloads) do
    local doc = decoder:parse(payload)
    -- ...access doc / open cursors...
end
decoder:reset()      -- optional: shrink internal buffers
decoder:destroy()    -- optional: free buffers eagerly
```

A `doc` returned by `decoder:parse()` becomes stale as soon as the same
decoder parses another payload (or is reset / destroyed). Accessor calls on a
stale doc return `nil`, the same convention as a missing path.

## Testing — Lua

Requires LuaJIT + busted + lua-cjson installed system-wide.

```sh
cargo build --release
busted tests/lua --lpath='./lua/?.lua' --cpath='./target/release/lib?.so'
```

## Benchmarking vs lua-cjson

Requires LuaJIT.

```sh
cargo build --release
luajit benches/lua_bench.lua
```

The benchmark measures end-to-end "parse + extract 3 fields" cost on small (~5KB) and medium (~60KB) JSON fixtures.

## Roadmap / Deferred

Items intentionally pushed out of the first implementation. Each will be picked up individually.

- **ARM64 NEON scanner backend** — first version ships with scalar + AVX2 backends only. NEON backend (for Apple Silicon / Graviton / 鲲鹏) is deferred.
- **SmallVec fast path for small documents (< 4 KB)** — avoid heap allocation for `indices` on tiny inputs.
- **SIMD-accelerated backslash search** in the `decode_string` fast path.
- **`lexical` fast float parser** if `<f64>::from_str` benchmarks as a bottleneck.
- **Lossless 64-bit integer mode** — return cdata `int64_t` to LuaJIT to preserve precision > 2⁵³.
- **Skip-cache LRU eviction** — only if memory pressure on huge documents proves problematic in practice.
- **Path-position info on Phase 1 errors** — currently only an opaque `QJD_PARSE_ERROR`.
- **Large bench fixtures** — spec §9.3 lists `large_dump.json` (~20 MB) and `deep_nest.json` (depth stress test); not yet committed. Only `small_api.json` and `medium_resp.json` ship today.
- **`structural_mask_chunk` via shuffle-based set check** — the current AVX2 scanner does 7 `_mm256_cmpeq_epi8` + `_mm256_movemask_epi8` per chunk half (one per structural char in `{}[]:,"`). A single `_mm256_shuffle_epi8` against a 16-byte LUT plus one cmpeq can do the same set membership in 2-3 ops per half. Estimated 15-25% scanner speedup on dense-structural workloads. Not on the hot path for string-heavy payloads (those already short-circuit via the fast path).
- **Adaptive `out.reserve` in scanners** — `out.reserve(buf.len() / 6)` is calibrated for object-heavy JSON. On string-heavy multimodal payloads (one big content array, mostly base64) the actual emit rate is <1 structural per 1 KB, so we over-reserve by 100x+. Mainly a memory hygiene concern (mmap'd pages stay lazily faulted), <5% throughput effect.
- **AVX-512 scanner backend** — 64-byte → 128-byte chunks. On the 1 MB string-heavy bench, profile shows scan throughput is L3-bandwidth-bound, so realistic win is ~1.5–1.8×, not a clean 2×; larger wins need fixtures that fit in L1/L2. Needs `avx512bw` + `vpclmulqdq` (Sapphire Rapids, Zen 4+).
- **`cargo fmt --check` not enforced** — `make lint` runs clippy only. The codebase uses intentional manual column alignment in struct definitions and compact single-line literals that default rustfmt would reflow. Skip rather than reformat until a project-wide style decision is made.
- **`validate_brackets` fusion into scan emit loop** — surfaced by profiling: on structurally-dense workloads `validate_brackets` is 65% of parse time (second linear pass over emitted indices). Folding bracket pairing into the scan emit loop via an inline depth stack eliminates that pass. No effect on the current string-heavy bench (0.3% there); a win for config / JSONL / table-shape JSON.
- **`memchr2` cross-chunk jump for very long string interiors** — the AVX2 in-string fast probe (issue #5) drops per-chunk cost from ~25 to ~10 ops but still pays ALU work for every 64-byte chunk in a string. A `memchr2(b'"', b'\\')` jump can approach memory bandwidth on multi-MB single-string payloads. Deferred until a workload that benefits clearly emerges; needs careful `bs_carry` reasoning across the jump.
- **Eliminate `validate_brackets` per-scan stack alloc on the pooled path** — the bracket-balance check builds a fresh `Vec::with_capacity(32)` every scan. On the pooled decoder API this and the per-parse `Box<qjd_doc>` are the only allocations the count-allocs test still sees (2 / parse). A pre-allocated stack on the `Decoder` would drop the count further; deferred because the absolute cost is tiny and the cleanest fix overlaps with the `validate_brackets` fusion item above.
- **Decoder pool / shared-decoder shortcut for `qd.parse`** — `qd.parse(payload)` still constructs a private decoder per call (1 indices Vec + 1 scratch + 1 skip-cache alloc each). A module-level shared decoder could make the legacy API allocation-free too, but adds a global-state footgun (no concurrent parses from coroutines); decoder pooling is exposed via the explicit `qd.new_decoder()` API instead. Reconsider if profiling shows `qd.parse` callers refusing to migrate.
- **Decoder generation counter wrap** — after `2^32` parses on the same decoder the gen wraps to a value an old (Lua-GC-still-alive) doc might match, masking staleness. With 1 ms/parse that is ~50 days of continuous reuse; in practice the doc is reclaimed long before. Could widen to `u64` or trip a hard error near the wrap point if a real-world workload comes close.
