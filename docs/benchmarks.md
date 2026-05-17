# Benchmarks

Throughput and memory comparison of `quickdecode` (this library) against
`lua-cjson` and `lua-resty-simdjson` on a multimodal chat-completion payload
ladder from 2 KB to 10 MB.

`quickdecode` is optimized for *parse + read a small number of fields*; the
data below quantifies how the lazy structural scan beats an eager build-the-
whole-table parser, and where the gap narrows. `lua-cjson` is the baseline.
`lua-resty-simdjson` (a Lua binding over the simdjson C++ library, eager) is
included to show how much of the win comes from SIMD vs. from skipping the
table build.

## Environment

| | |
|---|---|
| Host CPU | Intel Xeon (Skylake, IBRS), 4 cores |
| Memory | 7.6 GiB |
| OS | Linux x86_64 |
| Runtime | OpenResty `resty` 0.29 / openresty 1.29.2.3 / LuaJIT 2.1 ROLLING |
| `quickdecode` | this repo, release build, AVX2 + PCLMUL scanner active |
| `lua-cjson` | bundled with OpenResty |
| `lua-resty-simdjson` | upstream `main` build at `/tmp/lua-resty-simdjson`, simdjson C++ pinned by that repo |

The bench uses the OpenResty `resty` CLI because `lua-resty-simdjson` pulls in
`ngx.null` / `ngx.sleep` at load time and cannot run under bare LuaJIT
without an OpenResty environment. `lua-cjson` and `quickdecode` themselves
run fine under bare LuaJIT.

## Methodology

The harness lives at `benches/lua_bench.lua`. For each scenario:

1. Warmup pass (≥ 3 iterations, or `iters / 5`) to let LuaJIT compile hot
   traces and the `quickdecode` `indices` / `scratch` buffers grow to their
   working size. Warmup is excluded from timing and the memory delta.
2. `collectgarbage("collect")` baseline.
3. 5 rounds × N iterations of the workload; report the **median** ops/s
   across rounds (mean + range also reported in the raw output).
4. Final `collectgarbage("count")` to capture the post-run memory delta in
   KB — measures GC-rooted state retained by the parser, not transient
   per-call allocations.

The payload is a synthetic multimodal chat-completion request — one
~1.5 KB text part plus N base64-encoded image parts of 50–500 KB each
until the target size is reached. The image size sequence comes from a
Park–Miller LCG with `seed=42` rather than `math.random` so the payload is
byte-identical across hosts.

A separate `github-100k` scenario simulates a GitHub Issues API response
(`/repos/{owner}/{repo}/issues`) with ~100 KB of realistic REST API
structure: nested user objects, labels arrays, URLs, timestamps, and
markdown body text. This provides a benchmark for typical REST API
parsing workloads with ~3-5% structural density.

### Workload — what each row does

| Row | What it does | Notes |
|---|---|---|
| `cjson.decode + access 3 fields` | `cjson.decode(s)` then read 3 fields | Eager Lua table |
| `resty.simdjson:decode + access 3 fields` | `parser:decode(s)` then read 3 fields | Eager Lua table; **parser instance is reused** across iterations (the upstream-recommended pattern) |
| `quickdecode.parse + access 3 fields` | `qd.parse(s)` then `d:get_str/get_f64` × 3 | Lazy structural scan; explicit path-based reads |
| `qd.decode + t.field x3` | `qd.decode(s)` then `t.model` / `t.temperature` / `t.messages[1].role` | Lazy table proxy; reads go through `__index` |
| `qd.decode + qd.encode (unmodified)` | `qd.decode(s)` then re-emit as JSON | Substring fast path — no fields touched, so the proxy re-emits the original byte range via `memcpy` |

## Reproducing

The straight comparison against `cjson` is one command:

```sh
make bench
```

This invokes `benches/lua_bench.lua` with `LD_LIBRARY_PATH=target/release`
and a `LUA_CPATH` that picks up `cjson` from the system locations. It does
**not** include `lua-resty-simdjson`.

To also include `lua-resty-simdjson` you need (1) the library installed
somewhere `package.cpath` can reach the `.so`, (2) its Lua wrapper on
`package.path`, and (3) the bench script patched to require it. The patch
that adds the bench rows is a small `pcall(require, "resty.simdjson")` block;
keep it local — it is not part of the upstream bench file. Run it through
`resty` so the `ngx.*` symbols are available:

```sh
LD_LIBRARY_PATH=$PWD/target/release \
LUA_CPATH='/path/to/lua-resty-simdjson/?.so;./?.so;/usr/local/openresty/lualib/?.so;/usr/local/lib/lua/5.1/?.so' \
LUA_PATH='/path/to/lua-resty-simdjson/lib/?.lua;/path/to/lua-resty-simdjson/lib/?/init.lua;./lua/?.lua;;' \
/usr/local/openresty/bin/resty benches/lua_bench.lua
```

Numbers below come from one such run.

## Results — throughput (median ops/s)

Each row is "parse + access 3 fields" on the named payload.

| Scenario | Size | cjson | simdjson | `qd.parse` | `qd.decode + t.f x3` | `qd.decode + qd.encode` |
|---|---:|---:|---:|---:|---:|---:|
| small      |   2.1 KB | 39,414 | 54,395 | 117,233 | 126,807 | 268,240 |
| medium     |  60.4 KB |  5,600 | 40,180 |  90,074 | 120,627 | 126,263 |
| github-100k |   100 KB |  5,373 |      — |  27,020 |  27,367 |  36,430 |
| 100k       |   100 KB |  2,589 | 19,944 |  72,202 |  61,162 |  80,257 |
| 200k       |   200 KB |  1,414 | 14,397 |  57,670 |  48,031 |  58,548 |
| 500k       |   500 KB |    722 |  5,882 |  34,602 |  33,167 |  36,900 |
| 1m         |  1.00 MB |    355 |  2,048 |  12,723 |  12,448 |  12,669 |
| 2m         |  2.00 MB |    157 |    886 |   7,143 |   6,521 |   7,432 |
| 5m         |  5.00 MB |     64 |    250 |   2,509 |   2,235 |   2,552 |
| 10m        | 10.00 MB |     32 |    128 |     537 |     609 |     540 |
| interleaved (100k/200k/500k/1m, cycled) |  — |    723 |  4,399 |  21,424 |  23,378 |  24,004 |

### Speed-up vs. baselines

| Scenario | simdjson / cjson | `qd.parse` / cjson | `qd.parse` / simdjson | `qd.decode + access` / cjson |
|---|---:|---:|---:|---:|
| small  | 1.4× |  3.0× | 2.2× |  3.2× |
| medium | 7.2× | 16.1× | 2.2× | 21.5× |
| github-100k | — | 5.0× | — | 5.1× |
| 100k   | 7.7× | 27.9× | 3.6× | 23.6× |
| 200k   | 10.2× | 40.8× | 4.0× | 34.0× |
| 500k   | 8.1× | 47.9× | 5.9× | 45.9× |
| 1m     | 5.8× | 35.8× | 6.2× | 35.1× |
| 2m     | 5.6× | 45.5× | 8.1× | 41.5× |
| 5m     | 3.9× | 39.2× | 10.0× | 34.9× |
| 10m    | 4.0× | 16.8× | 4.2× | 19.0× |

## Results — memory delta (KB retained after 5 rounds)

Post-run `collectgarbage("count")` minus baseline. Captures GC-rooted state
the parser retains across iterations; transient per-call allocations are
collected before the snapshot.

| Scenario | cjson | simdjson | `qd.parse` | `qd.decode + t.f x3` | `qd.decode + qd.encode` |
|---|---:|---:|---:|---:|---:|
| small      | +15,881 | +16,284 | +1,338 | +4,337 | +11,140 |
| medium     |  +1,955 |  +2,661 |    +66 |   +500 |  +1,120 |
| github-100k | +12,867 |       — |    +19 |   +592 |    +273 |
| 100k       |    +601 |    +950 |    +18 |   +429 |    +229 |
| 200k       |    +505 |    +722 |     +7 |   +206 |    +112 |
| 500k       |    +648 |    +757 |     +3 |    +83 |     +45 |
| 1m         |  +1,151 |  +1,246 |     +2 |    +62 |     +34 |
| 2m         |  +2,311 |  +2,510 |     +3 |    +82 |     +45 |
| 5m         |  +5,723 |  +6,191 |     +3 |    +82 |     +45 |
| 10m        | +11,262 | +12,053 |     +3 |    +83 |     +45 |
| interleaved |  +4,509 |  +6,464 |    +53 | +1,671 |    +898 |

`qd.parse` retention is essentially constant across payload size: the only
GC-rooted state is the reusable `indices: Vec<u32>` and `scratch` buffers.
The `qd.decode + ...` paths retain a bit more — a few Lua tables for the
lazy proxy and any cached child views — but still allocate one to two
orders of magnitude less than the eager parsers, which materialize every
key into the Lua table heap.

## Pure-decode comparison (no field access)

Where the rows above measure "decode + use a few fields", this isolates
parse-time only. `cjson` and `simdjson` must still materialize a full Lua
table (no API to stop short of that); `qd.parse` does only the structural
scan and the skip-cache prep, deferring all per-field decode to whoever
later asks. Captures the upper bound of the lazy win.

| Scenario | cjson | simdjson | `qd.parse` | `qd.parse` / cjson | `qd.parse` / simdjson |
|---|---:|---:|---:|---:|---:|
| small  |  47,699 | 72,776 | 264,985 |  5.6× | 3.6× |
| medium |   6,698 | 48,328 | 105,485 | 15.7× | 2.2× |
| 100k   |   3,944 | 35,753 | 154,321 | 39.1× | 4.3× |
| 200k   |   1,974 | 17,403 |  80,386 | 40.7× | 4.6× |
| 500k   |     773 |  6,911 |  35,149 | 45.5× | 5.1× |
| 1m     |     362 |  2,611 |  14,691 | 40.6× | 5.6× |
| 2m     |     179 |  1,197 |   7,516 | 42.0× | 6.3× |
| 5m     |      74 |    293 |   2,876 | 38.9× | 9.8× |
| 10m    |      37 |    143 |     665 | 18.0× | 4.7× |

## Observations

1. **`simdjson` is 4–10× faster than `cjson` in the medium-to-large range**;
   the gap narrows at both ends — very small payloads are dominated by
   fixed per-call overhead, very large ones become memory-bandwidth bound on
   the Lua-table build.
2. **`quickdecode` is 16–48× faster than `cjson` and 2–10× faster than
   `simdjson`** on this workload. The win is not from SIMD — `simdjson`
   already has that — but from never building a Lua table. Field reads pay
   their own cost, but most fields are never read.
3. **The win drops at 10 MB.** `qd.parse` is L3-bandwidth-bound at that
   size, and the `qd.decode` proxy's per-`__index` dispatch starts to
   amortize less well against the cheaper structural scan. Other parsers
   are still allocating into the table heap at that size, so they degrade
   too, but the ratio compresses.
4. **`qd.decode + qd.encode (unmodified)` is the headline number for
   passthrough workloads** — e.g. an LLM gateway re-emitting the original
   JSON after light-touch inspection. The substring fast path means
   re-emit is `memcpy`, not re-serialize, and the throughput tracks
   `qd.parse` very closely.
5. **Memory retention** for `quickdecode` is essentially flat in payload
   size; the eager parsers retain ~1× the input size after the first run
   because the Lua table tree stays GC-rooted until the next collection.
   The 10 MB case retains ~11 MB for `cjson` / `simdjson`, ~3 KB for
   `qd.parse`.
6. **REST API payloads (github-100k) show a 5× speedup** — lower than the
   multimodal payloads because the structural density is higher (~3-5% vs
   <0.1%). However, memory savings remain dramatic: 677× less retention
   (12.8 MB → 19 KB) because `cjson` must materialize every nested object
   and string into the Lua heap.

## When to pick which

- **Read most/all fields** → `cjson` or `simdjson`. `simdjson` is a near-
  drop-in faster replacement (pool the parser).
- **Parse, read a few fields, discard / re-emit** → `quickdecode`. The
  bigger the payload and the smaller the read fraction, the larger the
  win. `qd.decode` / `qd.encode` gives a `cjson`-shaped surface; `qd.parse`
  + path getters is the lower-level API with slightly higher peak
  throughput on the access-light workloads.
- **Round-trip / passthrough an unmodified JSON** → `qd.decode +
  qd.encode`. Re-emit is `memcpy` for any subtree the caller did not
  touch.

## Caveats

- Single-host single-run numbers. Absolute ops/s does not port; the ratios
  do, broadly.
- Workload is biased toward string-heavy payloads (chat-completion image
  parts). Object-key-heavy JSON shifts the picture: more structural work
  per byte and less raw `memcpy`, so the SIMD scanners (`simdjson`,
  `quickdecode`'s AVX2 path) get further ahead of `cjson` and the
  table-build cost on the eager side rises.
- `quickdecode` retains the source buffer on the `Doc`, so the input
  string stays alive for the document's lifetime. If you parse and
  immediately discard the JSON string in the caller, GC can still free
  the input — but only after the `Doc` is also unreachable.
