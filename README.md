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
- **`# Safety` docs on unsafe FFI exports** — `make lint` currently fails on 22 `missing_safety_doc` clippy warnings from the public `qjd_*` C-ABI functions. Tracked separately so the Makefile can ship with `-D warnings` already wired up.
