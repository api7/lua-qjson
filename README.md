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
|   2 KB | 237,124 | 705,000 | 390,000 |  3.0× /  1.6× |
| 100 KB |  14,667 | 232,000 | 208,000 | 15.8× / 14.2× |
|   1 MB |   1,494 |  33,700 |  33,000 | 22.6× / 22.1× |
|  10 MB |     150 |   3,376 |   3,454 | 22.5× / 23.0× |

See [`docs/benchmarks.md`](docs/benchmarks.md) for the full size ladder,
memory numbers, an "encode round-trip" row (passthrough emit via
`memcpy`), the pure-decode (no-access) comparison, and the exact
methodology + reproduction command.

```sh
make bench       # quickdecode vs cjson
```
