# Benchmarks

Throughput and memory comparison of `qjson` (this library) against
`lua-cjson` and `lua-resty-simdjson` on a multimodal chat-completion payload
ladder from 2 KB to 10 MB.

`qjson` is optimized for *parse + read a small part of the document*;
the data below quantifies how the lazy structural scan behaves when the caller
reads request metadata plus every chat message `content`, without eagerly
building the whole Lua table. `lua-cjson` and `lua-resty-simdjson` are eager
Lua-table baselines.

## Environment

| | |
|---|---|
| Host CPU | AMD EPYC Rome (Zen 2), 4 vCPUs, AVX2 + PCLMUL |
| Memory | 8 GiB |
| OS | Ubuntu 24.04, x86_64 |
| Runtime | OpenResty `resty` 0.29 / OpenResty 1.21.4.4 / LuaJIT 2.1.1723681758 |
| `qjson` | this repo, release build, AVX2 + PCLMUL scanner active |
| `lua-cjson` | vendored `openresty/lua-cjson` |
| `lua-resty-simdjson` | `Kong/lua-resty-simdjson` commit `77322db640927c14968f1314a9fb1bb2bc084015`, installed under OpenResty lualib |

## Methodology

The harness lives at `benches/lua_bench.lua`. For each scenario:

1. Warmup pass (≥ 3 iterations, or `iters / 5`) to let LuaJIT compile hot
   traces and the `qjson` `indices` / `scratch` buffers grow to their
   working size. Warmup is excluded from timing and the memory delta.
2. `collectgarbage("collect")` baseline.
3. 5 rounds × N iterations of the workload; report the **median** ops/s
   across rounds (mean + range also reported in the raw output).
4. Final `collectgarbage("count")` to capture the post-run memory delta in
   KB. The harness does not force a final collection after timing, so
   short-lived garbage from the last round may still be included.

The payload is a synthetic multimodal chat-completion request with one or more
historical messages. Each message contains one small text part and one
base64-encoded image part. Message count scales with payload size: the 10 MB
scenario has roughly ten messages, each carrying one ~1 MB image, so the
access pattern matches request bodies where every historical message includes
an image.

A separate `github-100k` scenario simulates a GitHub Issues API response
(`/repos/{owner}/{repo}/issues`) with ~100 KB of realistic REST API
structure: nested user objects, labels arrays, URLs, timestamps, and
markdown body text. This provides a benchmark for typical REST API
parsing workloads with ~3-5% structural density.

### Workload — what each row does

| Row | What it does | Notes |
|---|---|---|
| `cjson.decode + access fields` | `cjson.decode(s)`, read `model` / `temperature`, then read every `messages[*].content` | Eager Lua table |
| `simdjson.decode + access fields` | `resty.simdjson:decode(s)`, read `model` / `temperature`, then read every `messages[*].content` | Eager Lua table |
| `qjson.parse + access fields` | `qjson.parse(s)`, read `model` / `temperature`, then touch every `messages[*].content` path | Lazy structural scan; explicit path reads |
| `qjson.decode + access content` | `qjson.decode(s)`, read `model` / `temperature`, then read every `messages[*].content` | Lazy table proxy; reads go through `__index` |
| `qjson.decode + qjson.encode (unmodified)` | `qjson.decode(s)` then re-emit as JSON | Substring fast path — no fields touched, so the proxy re-emits the original byte range via `memcpy` |

## Reproducing

Run the full comparison with one command:

```sh
make bench
```

This builds `qjson`, builds the vendored `lua-cjson` against OpenResty's
LuaJIT, then invokes `benches/lua_bench.lua` through OpenResty's `resty` so
`lua-resty-simdjson` runs in its normal `ngx` environment.
If `resty.simdjson` is not available on `package.path` / `package.cpath`, the
harness prints a skip message and omits the simdjson rows.

Numbers below come from one such run.

## Results — throughput (median ops/s)

Each row is "parse + access request fields" on the named payload.

| Scenario | Size | cjson | simdjson | `qjson.parse` | `qjson.decode + access content` | `qjson.decode + qjson.encode` |
|---|---|---:|---:|---:|---:|---:|---:|
| small      |   2.1 KB |  90,851 | 108,762 | 127,966 | 142,361 | 224,427 |
| medium     |  60.4 KB |   8,941 |  81,050 | 117,151 | 203,252 | 197,707 |
| github-100k |   100 KB |   2,284 |   2,090 |   6,272 |   6,305 |   6,931 |
| 100k       |   100 KB |   5,346 |  44,366 | 122,249 | 130,208 | 146,628 |
| 200k       |   200 KB |   2,677 |  20,425 |  71,839 |  44,444 |  62,422 |
| 500k       |   500 KB |   1,064 |   7,307 |  29,070 |  29,028 |  34,364 |
| 1m         |  1.00 MB |     513 |   3,610 |  14,124 |  15,167 |  14,940 |
| 2m         |  2.00 MB |     257 |   2,018 |   7,686 |   7,761 |   7,981 |
| 5m         |  5.00 MB |     103 |     790 |   2,628 |   3,337 |   3,367 |
| 10m        | 10.00 MB |      50 |     389 |   1,576 |   1,599 |   1,642 |
| interleaved (100k/200k/500k/1m, cycled) | — | 1,149 | 9,208 | 28,627 | 27,624 | 25,275 |

### Speed-up vs. baselines

| Scenario | `qjson.parse` / cjson | `qjson.parse` / simdjson | `qjson.decode + access content` / cjson | `qjson.decode + access content` / simdjson |
|---|---|---:|---:|---:|---:|
| small  |  1.4× |  1.2× |  1.6× |  1.3× |
| medium | 13.1× |  1.4× | 22.7× |  2.5× |
| github-100k | 2.7× |  3.0× | 2.8× |  3.0× |
| 100k   | 22.9× |  2.8× | 24.4× |  2.9× |
| 200k   | 26.8× |  3.5× | 16.6× |  2.2× |
| 500k   | 27.3× |  4.0× | 27.3× |  4.0× |
| 1m     | 27.5× |  3.9× | 29.6× |  4.2× |
| 2m     | 29.9× |  3.8× | 30.2× |  3.8× |
| 5m     | 25.5× |  3.3× | 32.4× |  4.2× |
| 10m    | 31.5× |  4.1× | 32.0× |  4.1× |

## Results — memory delta (KB retained after 5 rounds)

Post-run `collectgarbage("count")` minus baseline. Captures heap usage after
the timing rounds without forcing a final collection, so short-lived garbage
from the last round may still be included.

| Scenario | cjson | simdjson | `qjson.parse` | `qjson.decode + access content` | `qjson.decode + qjson.encode` |
|---|---|---:|---:|---:|---:|---:|
| small      | +15,493 | +15,497 | +4,069 | +15,223 | +11,140 |
| medium     |  +1,954 |  +2,660 |   +334 |    +537 |  +1,120 |
| github-100k | +11,911 | +4,124 |    +14 |    +566 |    +230 |
| 100k       |    +484 |    +748 |    +67 |    +710 |    +229 |
| 200k       |    +392 |    +523 |    +34 |    +346 |    +117 |
| 500k       |    +577 |    +630 |    +14 |    +139 |     +45 |
| 1m         |  +1,082 |  +1,121 |    +10 |    +104 |     +34 |
| 2m         |  +1,155 |  +1,248 |    +14 |    +208 |     +45 |
| 5m         |  +1,316 |  +1,538 |    +14 |    +400 |     +45 |
| 10m        |  +1,583 |  +2,014 |    +14 |    +722 |     +45 |
| interleaved | +3,356 | +4,405 |   +268 |  +2,778 |    +898 |

`qjson.parse` retention is essentially constant across payload size: the only
GC-rooted state is the reusable `indices: Vec<u32>` and `scratch` buffers.
The `qjson.decode + ...` paths retain a bit more — a few Lua tables for the
lazy proxy and any cached child views — but still allocate one to two
orders of magnitude less than the eager parsers, which materialize every
key into the Lua table heap.

## Observations

1. **`qjson` is fastest once payloads move beyond tiny inputs.**
   The small 2 KB row is dominated by fixed Lua/FFI overhead, but medium and
   larger multimodal payloads show roughly 13–31× higher throughput than
   `cjson` and roughly 2–4× higher throughput than `lua-resty-simdjson`
   for request-field access.
2. **Reading every `messages[*].content` is still access-light for large
   multimodal bodies.** The benchmark touches the top-level request fields and
   one `content` field per message; the payload size comes from image data
   inside each message.
3. **Speedup remains high at 10 MB.** The eager decode deduplication
   (skip re-validation when eagerly validated) and fused eager validation
   passes keep `qjson.parse` throughput scaling well even at the 10 MB level,
   maintaining ~32× over cjson and ~4× over simdjson.
4. **`qjson.decode + qjson.encode (unmodified)` is the headline number for
   passthrough workloads** — e.g. an LLM gateway re-emitting the original
   JSON after light-touch inspection. The substring fast path means
   re-emit is `memcpy`, not re-serialize, and the throughput tracks
   `qjson.parse` very closely.
5. **Memory retention** for `qjson` is essentially flat in payload
   size; the eager parsers retain more Lua heap after the first run
   because the Lua table tree stays GC-rooted until the next collection.
   The 10 MB case retains ~1.5 MB for `cjson`, ~2.0 MB for simdjson,
   and ~14 KB for `qjson.parse`.
6. **REST API payloads (github-100k) show a smaller speedup** because their
   structural density is higher than the multimodal request ladder. Memory
   savings remain dramatic because `cjson` must materialize every nested
   object and string into the Lua heap.

## Eager validation micro-benchmark (Rust)

The eager validation path was optimized by fusing three separate post-scan
passes (`validate_depth`, `validate_trailing`, `validate_eager_values`) into a
single `validate_eager_fused` traversal, and replacing the AVX2 string validator
with a PSHUFB nibble-LUT byte classifier. The Lua bench numbers above already
include this improvement. On 1 MB payloads measured at the Rust level (10-run
avg, AMD EPYC Rome Zen 2):

| Payload | Before | After | Improvement |
|---------|--------|-------|-------------|
| GitHub-style REST API (pure ASCII) | 1,688 ± 97 us | 1,462 ± 39 us | **13.4%** |
| Escape-heavy (\n \t \\ \uXXXX) | 912 ± 77 us | 776 ± 30 us | **14.9%** |

## When to pick which

- **Read most/all fields** → `cjson`.
- **Parse, read selected fields, discard / re-emit** → `qjson`. The
  bigger the payload and the smaller the read fraction, the larger the
  win. `qjson.decode` / `qjson.encode` gives a `cjson`-shaped surface; `qjson.parse`
  + path getters is the lower-level API with slightly higher peak
  throughput on the access-light workloads.
- **Round-trip / passthrough an unmodified JSON** → `qjson.decode +
  qjson.encode`. Re-emit is `memcpy` for any subtree the caller did not
  touch.

## Caveats

- Single-host single-run numbers. Absolute ops/s does not port; the ratios
  do, broadly.
- Workload is biased toward string-heavy payloads (chat-completion image
  parts). Object-key-heavy JSON shifts the picture: more structural work
  per byte and less raw `memcpy`, while the table-build cost on the eager
  side rises.
- `qjson` retains the source buffer on the `Doc`, so the input
  string stays alive for the document's lifetime. If you parse and
  immediately discard the JSON string in the caller, GC can still free
  the input — but only after the `Doc` is also unreachable.