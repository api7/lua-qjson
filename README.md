# qjson

Rust-implemented fast JSON decoder exposed to LuaJIT via FFI. Optimized for the common case where a large JSON is parsed once and only a small number of fields are extracted before the document is discarded.

## Status

Scalar + AVX2/PCLMUL + ARM64 NEON/PMULL structural scanner (runtime-dispatched), root-path and cursor APIs, escape-decoded strings, integer/float/bool/typeof/len, FFI panic barrier, and a LuaJIT wrapper. Eager validation uses a fused single-pass grammar state machine with a PSHUFB nibble-LUT byte classifier for string validation. Rust unit/integration tests and Lua busted tests run in CI. The benchmark harness compares against lua-cjson and lua-resty-simdjson.

## Building

```sh
cargo build --release
# Output: target/release/libqjson.so
```

A `Makefile` wraps the common workflows; run `make help` to see `build`, `test`, `lint`, `bench`, and `clean` targets. Override `LUAJIT` / `LUA_CPATH` per invocation if your environment differs from the defaults.

## Installing

```sh
luarocks install lua-qjson
```

The rock builds the Rust native library during installation, so Rust/Cargo
and LuaJIT must be available on the target system. The Lua module name remains
`qjson`:

```lua
local qjson = require("qjson")
```

## Testing

```sh
git submodule update --init --recursive
cargo test --release
```

## LuaJIT Usage

```lua
local qjson = require("qjson")
local doc = qjson.parse(json_str)

-- Root-path getter:
local model = doc:get_str("body.model")

-- Cursor (avoid re-walking shared prefix):
local body = doc:open("body")
local model = body:get_str("model")
local temp  = body:get_f64("temperature")
```

### Lazy table API (`qjson.decode` / `qjson.encode`)

For callers migrating from `cjson`, an alternative API returns a table-shaped
lazy view. Reads, iteration, and length all work like a `cjson.decode`'d
table; writes materialize the affected level into a plain Lua table.

```lua
local qjson    = require("qjson")
local cjson = require("cjson")          -- optional; provides null / empty_array sentinels

local t = qjson.decode(json_str)

print(t.model)
for _, m in qjson.ipairs(t.messages) do
    print(m.role, m.content)
end

t.extra = "x"

local s = qjson.encode(t)                  -- drop-in replacement for cjson.encode
```

`qjson.encode` works on lazy proxies (re-emitting unmodified subtrees as the
original JSON bytes), real Lua tables (matching `cjson.encode` output), and
mixed trees. Callers cannot pass a lazy proxy directly to `cjson.encode`
(cjson bypasses metamethods in C); use `qjson.encode` instead, or call
`qjson.materialize(t)` to get a plain Lua table that any third-party encoder
can handle.

**LuaJIT compat-52 caveat.** `for k, v in pairs/ipairs(t)` and `#t` on a lazy
proxy rely on `__pairs` / `__ipairs` / `__len`, which LuaJIT only invokes when
built with `LUAJIT_ENABLE_LUA52COMPAT` (OpenResty's default). On a stock LuaJIT
5.1, use the explicit `qjson.pairs(t)`, `qjson.ipairs(t)`, and `qjson.len(t)` helpers
— they work on both builds.

## Testing — Lua

Requires LuaJIT + busted + lua-cjson installed system-wide.

```sh
cargo build --release
LD_LIBRARY_PATH="$PWD/target/release" \
  busted --lua="$(which luajit)" tests/lua --lpath='./lua/?.lua'
```

## Benchmarks

`qjson` vs. `lua-cjson` and `lua-resty-simdjson` on multimodal
chat-completion payloads, "parse + access model, temperature, and all
messages[*].content paths" workload (median ops/s under OpenResty LuaJIT 2.1,
AMD EPYC Rome (Zen 2, 4 vCPUs); 5 rounds, deterministic payload):

| Size | cjson | simdjson | `qjson.parse` | `qjson.decode + access content` | speedup vs. cjson |
|---:|---:|---:|---:|---:|---:|
|   2 KB |  94,075 | 108,108 | 127,214 | 120,398 |  1.4× /  1.3× |
|  60 KB |   9,041 |  83,043 | 123,487 | 214,500 | 13.7× / 23.7× |
| 100 KB |   5,302 |  32,248 | 109,649 | 102,564 | 20.7× / 19.3× |
|   1 MB |     517 |   3,538 |  16,520 |  16,988 | 32.0× / 32.9× |
|  10 MB |      50 |     402 |   1,899 |   1,918 | 38.0× / 38.4× |

`qjson.parse` wins because it skips building a Lua table for the parts you
never read; `qjson.decode + t.field` adds a cjson-shaped table proxy on top
with similar throughput. Memory retention for `qjson` is essentially
flat in payload size (a few KB for the reusable buffers), while `cjson`
and `simdjson` retain more Lua heap because they materialize the table tree.

See [`docs/benchmarks.md`](docs/benchmarks.md) for the full size ladder,
memory numbers, an "encode round-trip" row (passthrough emit via
`memcpy`), exact environment, and the reproduction command. `make bench`
uses `lua-resty-simdjson` when `resty.simdjson` is available in the
OpenResty environment; otherwise it skips the simdjson rows.

```sh
make bench       # qjson vs cjson and lua-resty-simdjson
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
local doc = qjson.parse(json)                            -- eager (default)
local doc = qjson.parse(json, { lazy = true })           -- lazy mode
local doc = qjson.parse(json, { max_depth = 256 })       -- stricter depth limit
local doc = qjson.parse(json, { lazy = true, max_depth = 256 })
```

From C:

```c
qjson_options opts = { .mode = QJSON_MODE_LAZY, .max_depth = 256 };
qjson_doc* doc = qjson_parse_ex(buf, len, &opts, &err);
```

### Known gaps

There are no known strict-mode structural grammar gaps at this time:
`tests/json_test_suite.rs::KNOWN_N_FAILURES` is empty, and the RFC 8259
suite has no ignored structural cases. Update this section whenever a
temporary conformance exception is introduced.