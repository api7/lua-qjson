# lua-quick-decode

Rust-implemented fast JSON decoder exposed to LuaJIT via FFI. Optimized for the common case where a large JSON is parsed once and only a small number of fields are extracted before the document is discarded.

## Status

Initial implementation complete: scalar + AVX2/PCLMUL + ARM64 NEON/PMULL structural scanner (runtime-dispatched), root-path and cursor APIs, escape-decoded strings, integer/float/bool/typeof/len, FFI panic barrier, and a LuaJIT wrapper. Rust unit/integration tests and Lua busted tests run in CI. The benchmark harness compares against lua-cjson but tuning is pending — see `Roadmap / Deferred` below.

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

### Lazy table API (`qd.decode` / `qd.encode`)

For callers migrating from `cjson`, an alternative API returns a table-shaped
lazy view. Reads, iteration, and length all work like a `cjson.decode`'d
table; writes materialize the affected level into a plain Lua table.

```lua
local qd    = require("quickdecode")
local cjson = require("cjson")          -- optional; provides null / empty_array sentinels

local t = qd.decode(json_str)

print(t.model)
for _, m in qd.ipairs(t.messages) do
    print(m.role, m.content)
end

t.extra = "x"

local s = qd.encode(t)                  -- drop-in replacement for cjson.encode
```

`qd.encode` works on lazy proxies (re-emitting unmodified subtrees as the
original JSON bytes), real Lua tables (matching `cjson.encode` output), and
mixed trees. Callers cannot pass a lazy proxy directly to `cjson.encode`
(cjson bypasses metamethods in C); use `qd.encode` instead, or call
`qd.materialize(t)` to get a plain Lua table that any third-party encoder
can handle.

**LuaJIT compat-52 caveat.** `for k, v in pairs/ipairs(t)` and `#t` on a lazy
proxy rely on `__pairs` / `__ipairs` / `__len`, which LuaJIT only invokes when
built with `LUAJIT_ENABLE_LUA52COMPAT` (OpenResty's default). On a stock LuaJIT
5.1, use the explicit `qd.pairs(t)`, `qd.ipairs(t)`, and `qd.len(t)` helpers
— they work on both builds.

## Testing — Lua

Requires LuaJIT + busted + lua-cjson installed system-wide.

```sh
cargo build --release
busted tests/lua --lpath='./lua/?.lua' --cpath='./target/release/lib?.so'
```

## Benchmarks

`quickdecode` vs. `lua-cjson` and `lua-resty-simdjson` on multimodal
chat-completion payloads, "parse + access 3 fields" workload (median ops/s
under LuaJIT 2.1, Skylake; 5 rounds, deterministic payload):

| Size | cjson | simdjson | `qd.parse` | `qd.decode + t.f x3` | speedup vs. cjson |
|---:|---:|---:|---:|---:|---:|
|   2 KB | 39,414 | 54,395 | 117,233 | 126,807 |  3.0× / 3.2× |
| 100 KB |  2,589 | 19,944 |  72,202 |  61,162 | 27.9× / 23.6× |
|   1 MB |    355 |  2,048 |  12,723 |  12,448 | 35.8× / 35.1× |
|  10 MB |     32 |    128 |     537 |     609 | 16.8× / 19.0× |

`qd.parse` wins because it skips building a Lua table for the parts you
never read; `qd.decode + t.field` adds a cjson-shaped table proxy on top
with similar throughput. Memory retention for `quickdecode` is essentially
flat in payload size (a few KB for the reusable buffers), where `cjson`
and `simdjson` retain ~1× the input size as live Lua-table state.

ARM64 (Apple M4, NEON/PMULL scanner, same workload):

| Size | cjson | `qd.parse` | `qd.decode + t.f x3` | speedup vs. cjson |
|---:|---:|---:|---:|---:|
|   2 KB | 254,738 | 654,108 | 392,711 | 2.6× / 1.5× |
| 100 KB |  15,281 | 108,932 |  99,701 | 7.1× / 6.5× |
|   1 MB |   1,523 |  11,905 |  11,876 | 7.8× / 7.8× |
|  10 MB |     153 |   1,218 |   1,222 | 8.0× / 8.0× |

See [`docs/benchmarks.md`](docs/benchmarks.md) for the full size ladder,
memory numbers, an "encode round-trip" row (passthrough emit via
`memcpy`), the pure-decode (no-access) comparison, and the exact
methodology + reproduction command.

```sh
make bench       # quickdecode vs cjson
```

## Roadmap / Deferred

Items intentionally pushed out of the first implementation. Each will be picked up individually.

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
- **`validate_brackets` fusion in SIMD scanners** — fused into `ScalarScanner` via `scan_and_validate`; AVX2 and NEON scanners still run the two-pass emit + `validate_brackets` design. A working implementation was prototyped in [#18](https://github.com/membphis/lua-quick-decode/pull/18) (closed): `emit_bits_validate` carries a depth stack inline and dispatches on `buf[pos]` per emitted bit, eliminating the second pass over `indices`. Measured ±2% (within noise) on the multimodal bench because the per-emit `buf[pos]` lookup adds back roughly what the eliminated pass saved, and the structural-char density is too low for the savings to dominate. Revisit only when a structurally-dense fixture (config / JSONL / object-shape JSON with hundreds of keys per chunk) is added to the bench harness and profiles flag `validate_brackets` as the bottleneck.
- **`memchr2` cross-chunk jump for very long string interiors** — the AVX2 in-string fast probe (issue #5) drops per-chunk cost from ~25 to ~10 ops but still pays ALU work for every 64-byte chunk in a string. A `memchr2(b'"', b'\\')` jump can approach memory bandwidth on multi-MB single-string payloads. Deferred until a workload that benefits clearly emerges; needs careful `bs_carry` reasoning across the jump.
- **Stateful O(N) iterator FFI** — current `qd.pairs` and the `__newindex`
  materialization path walk the object cursor from the start on every step,
  giving O(N²) total cost for full enumeration. Acceptable for the "read a
  few keys" use case the library is optimized for; full-iteration workloads
  (e.g. encoding a deeply-keyed object that has been materialized) would
  benefit from a `qjd_iter_init` / `qjd_iter_next` pair that holds position
  state across calls.
- **Lazy-table read overhead vs path API** — `qd.decode + t.field x3` lands
  ~30–40% behind `qd.parse:get_str` on small-to-medium payloads, converging
  to parity at multi-MB sizes. The gap is structural (per-access `__index`
  metamethod dispatch + transient cdata allocation for nested wraps). Worth
  attempting if a workload-driven need surfaces; current measured cost is
  still 14× faster than `cjson.decode` at 100 KB, so the lazy API is the
  right default for migrating callers.
