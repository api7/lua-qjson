# lua-quick-decode

Rust-implemented fast JSON decoder exposed to LuaJIT via FFI. Optimized for the common case where a large JSON is parsed once and only a small number of fields are extracted before the document is discarded.

## Status

Initial implementation complete: scalar + AVX2/PCLMUL + ARM64 NEON/PMULL structural scanner (runtime-dispatched), root-path and cursor APIs, escape-decoded strings, integer/float/bool/typeof/len, FFI panic barrier, and a LuaJIT wrapper. Rust unit/integration tests and Lua busted tests run in CI. The benchmark harness compares against lua-cjson and lua-resty-simdjson.

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
chat-completion payloads, "parse + access model, temperature, and all
messages[*].content paths" workload (median ops/s under OpenResty LuaJIT 2.1,
Intel Core i5-9400; 5 rounds, deterministic payload):

| Size | cjson | simdjson | `qd.parse` | `qd.decode + access content` | speedup vs. cjson |
|---:|---:|---:|---:|---:|---:|
|   2 KB | 106,646 | 137,427 | 135,296 |  97,574 |  1.3× /  0.9× |
| 100 KB |   6,045 |  46,577 | 137,931 | 134,590 | 22.8× / 22.3× |
|   1 MB |     594 |   4,408 |  16,447 |  16,340 | 27.7× / 27.5× |
|  10 MB |      59 |     356 |   1,035 |   1,028 | 17.5× / 17.4× |

`qd.parse` wins because it skips building a Lua table for the parts you
never read; `qd.decode + t.field` adds a cjson-shaped table proxy on top
with similar throughput. Memory retention for `quickdecode` is essentially
flat in payload size (a few KB for the reusable buffers), while `cjson`
and `simdjson` retain more Lua heap because they materialize the table tree.

See [`docs/benchmarks.md`](docs/benchmarks.md) for the full size ladder,
memory numbers, an "encode round-trip" row (passthrough emit via
`memcpy`), exact environment, and the reproduction command. `make bench`
uses `lua-resty-simdjson` when `resty.simdjson` is available in the
OpenResty environment; otherwise it skips the simdjson rows.

```sh
make bench       # quickdecode vs cjson and lua-resty-simdjson
```

## RFC 8259 conformance

This crate implements RFC 8259 with both strict and lenient modes; the strict
(eager) mode is the default and is required by API-gateway use cases that must
reject malformed payloads before forwarding them upstream.

- Strict-mode acceptance corpus: `tests/rfc8259_compliance.rs`
- Industry corpus: `tests/json_test_suite.rs` (against the
  [JSONTestSuite](https://github.com/nst/JSONTestSuite) submodule at
  `tests/vendor/JSONTestSuite`)
- Behavior on implementation-defined (`i_*`) cases: `docs/rfc8259-conformance.md`

### Switching modes

From Lua:

```lua
local doc = qd.parse(json)                            -- eager (default)
local doc = qd.parse(json, { lazy = true })           -- lazy mode
local doc = qd.parse(json, { max_depth = 256 })       -- stricter depth limit
local doc = qd.parse(json, { lazy = true, max_depth = 256 })
```

From C:

```c
qjd_options opts = { .mode = QJD_MODE_LAZY, .max_depth = 256 };
qjd_doc* doc = qjd_parse_ex(buf, len, &opts, &err);
```

### Known gaps

Three structural-grammar checks are deferred to a follow-up — they require a grammar-aware walk beyond the current heuristic. See `tests/rfc8259_compliance.rs` for the specific `#[ignore]`d cases, and `tests/json_test_suite.rs::KNOWN_N_FAILURES` for the corresponding JSONTestSuite files.
