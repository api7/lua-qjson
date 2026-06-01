# qjson

Rust-implemented fast JSON decoder exposed to LuaJIT via FFI. Optimized for the common case where a large JSON is parsed once and only a small number of fields are extracted before the document is discarded.

Primary supported runtime is OpenResty LuaJIT. Stock LuaJIT is supported with
the caveats documented below. Standard PUC Lua 5.1/5.2/5.3/5.4 is not
supported because qjson depends on LuaJIT FFI.

See [CHANGELOG.md](CHANGELOG.md) for release notes.

## Documentation

- [Migrating from lua-cjson](docs/migrating-from-cjson.md) maps common
  `cjson.*` calls to qjson and calls out behavior differences.
- [CONTRIBUTING.md](CONTRIBUTING.md) covers local setup, tests, linting, and
  release/contribution conventions.

## Status

Initial implementation complete: scalar, AVX2/PCLMUL, and ARM64 NEON/PMULL structural scanners (runtime-dispatched); root-path and cursor APIs; escape-decoded strings; integer/float/bool/typeof/len accessors; FFI panic barrier; and a LuaJIT wrapper. Rust unit/integration tests and Lua busted tests run in CI. The benchmark harness compares against lua-cjson and lua-resty-simdjson on x86_64; ARM64 NEON/PMULL is correctness-tested via the scanner cross-check suite, with a parse + access benchmark on Apple M4 reported in `docs/benchmarks.md` (cjson comparison only).

## Building

```sh
cargo build --release
# Output: target/release/libqjson.so
```

A `Makefile` wraps the common workflows; run `make help` to see `build`, `test`, `lint`, `bench`, and `clean` targets. Override `LUAJIT` / `LUA_CPATH` per invocation if your environment differs from the defaults.

## Installing

```sh
luarocks --lua-version=5.1 install lua-qjson
```

Build/install requirements:

- Rust/Cargo (the rock builds the native library during install)
- LuaRocks targeting a Lua 5.1 / LuaJIT-compatible tree
  (`--lua-version=5.1`)

Runtime requirements:

- OpenResty LuaJIT (intended runtime) or stock LuaJIT with the caveats below
- Access to the installed qjson native library and Lua module files

The Lua module name remains `qjson`:

```lua
local qjson = require("qjson")
```

## Production deployment (no Rust at runtime)

Rust/Cargo is only a build-time dependency. For production images, install the
rock into a staging tree in a builder stage, then copy that tree into a runtime
stage that contains OpenResty LuaJIT (preferred) or LuaJIT, but no Rust
toolchain.

The runtime file set is small:

| Installed path | Purpose |
|---|---|
| `/usr/local/lib/lua/5.1/qjson.so` | Compiled native library |
| `/usr/local/share/lua/5.1/qjson.lua` | Public Lua module |
| `/usr/local/share/lua/5.1/qjson/lib.lua` | FFI loader |
| `/usr/local/share/lua/5.1/qjson/table.lua` | Lazy table API and encoder |

The runtime also needs OpenResty LuaJIT (preferred) or LuaJIT, plus the normal
OS shared libraries required by LuaJIT and the native module.

Example multi-stage Dockerfile:

```Dockerfile
FROM rust:1-bookworm AS qjson-builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        git \
        libluajit-5.1-dev \
        luarocks \
        luajit \
    && rm -rf /var/lib/apt/lists/*

RUN luarocks --lua-version=5.1 install --tree=/qjson-rock lua-qjson

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates luajit \
    && rm -rf /var/lib/apt/lists/*

COPY --from=qjson-builder /qjson-rock/ /usr/local/

RUN ! command -v cargo \
    && ! command -v rustc \
    && luajit -e 'local qjson = require("qjson"); local doc = qjson.parse("{\"ok\":true}"); assert(doc:get_bool("ok") == true)'
```

For local release testing from a checkout, use the same staging pattern with
the rockspec:

```sh
luarocks --lua-version=5.1 make --tree=/tmp/qjson-rock rockspec/lua-qjson-0.1.0-1.rockspec
```

Then copy `/tmp/qjson-rock` into the runtime filesystem root that LuaJIT uses
for `/usr/local`.

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
local model      = body:get_str("model")
local temp       = body:get_f64("temperature") -- Lua number (double)
local request_id = body:get_i64("request_id")  -- int64_t cdata, lossless
```

`get_i64` returns LuaJIT `int64_t` cdata and `get_u64` returns `uint64_t`
cdata, preserving JSON integers that do not fit exactly in a Lua `number`.
Use `get_f64` for the convenient Lua-number path, or `tonumber(...)` when a
64-bit cdata value is known to fit in double precision.

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

local s = qjson.encode(t)                  -- cjson-compatible encode path for lazy proxies
```

`qjson.encode` works on lazy proxies (re-emitting unmodified subtrees as the
original JSON bytes), real Lua tables (matching `cjson.encode` output), mixed
trees, and LuaJIT `int64_t` / `uint64_t` cdata values. Callers cannot pass a
lazy proxy directly to `cjson.encode` (cjson bypasses metamethods in C); use
`qjson.encode` instead, or call `qjson.materialize(t)` to get a plain Lua table
that any third-party encoder can handle.

**Native `next` caveat.** `next(t)` is not proxy-aware: it bypasses the
`__pairs` / `__ipairs` hooks and may see qjson implementation fields instead of
JSON fields. Do not use native `next` to iterate a lazy proxy or test whether it
is empty. Use `qjson.pairs(t)`, `qjson.ipairs(t)`, or `qjson.len(t)` instead, or
call `qjson.materialize(t)` before passing the value to code that requires
ordinary Lua table traversal.

**LuaJIT compat-52 caveat.** `for k, v in pairs/ipairs(t)` and `#t` on a lazy
proxy rely on `__pairs` / `__ipairs` / `__len`, which LuaJIT only invokes when
built with `LUAJIT_ENABLE_LUA52COMPAT` (OpenResty's default and the intended
runtime setup). On a stock LuaJIT 5.1, use the explicit `qjson.pairs(t)`,
`qjson.ipairs(t)`, and `qjson.len(t)` helpers — they work on both builds.

## Testing — Lua

Requires LuaJIT + busted + lua-cjson installed system-wide.

```sh
cargo build --release
LD_LIBRARY_PATH="$PWD/target/release" \
  busted --lua="$(which luajit)" tests/lua --lpath='./lua/?.lua'
```

## Benchmarks

`qjson` vs. `lua-cjson` and `lua-resty-simdjson` on multimodal
chat-completion payloads (median ops/s under OpenResty LuaJIT 2.1,
AMD EPYC Rome, Zen 2, 4 vCPUs; 5 rounds, deterministic payload).

### Parse + access (read-only)

| Size | cjson | simdjson | `qjson.parse` | `qjson.decode + access` | speedup vs. cjson |
|---:|---:|---:|---:|---:|---:|
|   2 KB |  92,716 | 102,602 | 128,005 | 125,815 |  1.4× /  1.4× |
|  60 KB |   9,007 |  82,699 | 116,198 | 219,491 | 12.9× / 24.4× |
| 100 KB |   2,769 |  40,437 |  84,034 | 121,803 | 30.3× / 44.0× |
|   1 MB |     512 |   4,020 |  16,056 |  15,400 | 31.4× / 30.1× |
|  10 MB |      51 |     363 |   1,830 |   1,783 | 35.9× / 35.0× |

### Encode (unmodified) + modify-then-re-encode

Numbers from the same run as [`docs/benchmarks.md`](docs/benchmarks.md).

| Size | encode (unmodified) | modify top + encode | modify nested + encode |
|---:|---:|---:|---:|
|    2 KB | 260,322 | 58,242 |  43,003 |
|   60 KB | 141,563 | 37,498 | 134,590 |
|  100 KB | 105,374 | 28,114 |  71,942 |
|    1 MB |  16,269 |  3,125 |  13,649 |

> **qjson.encode(unmodified)** re-emits the original byte range via `memcpy` —
> no fields touched means zero serializer work.
> **qjson modify+encode** materializes only the mutated subtree; unmodified
> siblings stay on the fast path. See [`docs/benchmarks.md`](docs/benchmarks.md)
> for the full size ladder, cjson comparisons, speedup ratios, memory
> numbers, and environment.

```sh
make bench-smoke # correctness smoke, no timing
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
