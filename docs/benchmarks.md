# Benchmarks

Throughput and memory comparison of `qjson` (this library) against
`lua-cjson` on a multimodal chat-completion payload ladder from 2 KB to 10 MB.
(`lua-resty-simdjson` was not available on the benchmark host; rows are marked
"n/a" where it would appear.)

`qjson` is optimized for *parse + read a small part of the document*;
the data below quantifies how the lazy structural scan behaves when the caller
reads request metadata plus every chat message `content`, without eagerly
building the whole Lua table. `lua-cjson` is the eager Lua-table baseline.

## Environment

| | |
|---|---|
| Host CPU | AMD EPYC Rome (Zen 2), 4 vCPUs, AVX2 + PCLMUL |
| Memory | 8 GiB |
| OS | Ubuntu 24.04, x86_64 |
| Runtime | OpenResty `resty` 0.29 / OpenResty 1.21.4.4 / LuaJIT 2.1.1723681758 |
| `qjson` | this repo, release build, AVX2 + PCLMUL scanner active |
| `lua-cjson` | vendored `openresty/lua-cjson` |
| `lua-resty-simdjson` | not installed on benchmark host |

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
| `qjson.parse + access fields` | `qjson.parse(s)`, read `model` / `temperature`, then touch every `messages[*].content` path | Lazy structural scan; explicit path reads |
| `qjson.decode + access content` | `qjson.decode(s)`, read `model` / `temperature`, then read every `messages[*].content` | Lazy table proxy; reads go through `__index` |
| `qjson.decode + qjson.encode (unmodified)` | `qjson.decode(s)` then re-emit as JSON | Substring fast path — no fields touched, so the proxy re-emits the original byte range via `memcpy` |

## Reproducing

Run the full comparison with one command:

```sh
make bench
```

This builds `qjson`, builds the vendored `lua-cjson` against OpenResty's
LuaJIT, then invokes `benches/lua_bench.lua` through OpenResty's `resty`.
If `resty.simdjson` is not available on `package.path` / `package.cpath`, the
harness prints a skip message and omits the simdjson rows.

Numbers below come from one such run.

## Results — throughput (median ops/s)

Each row is "parse + access request fields" on the named payload.

| Scenario | Size | cjson | `qjson.parse` | `qjson.decode + access content` | `qjson.decode + qjson.encode` |
|---|---:|---:|---:|---:|---:|
| small      |   2.1 KB |  96,665 | 128,218 |  89,259 | 215,183 |
| medium     |  60.4 KB |   8,668 | 186,289 | 197,316 | 223,814 |
| github-100k |   100 KB |   2,090 |   6,170 |   5,857 |   6,581 |
| 100k       |   100 KB |   4,587 | 150,602 | 144,300 | 175,747 |
| 200k       |   200 KB |   2,581 |  87,719 |  84,746 |  99,206 |
| 500k       |   500 KB |   1,025 |  32,310 |  33,898 |  37,106 |
| 1m         |  1.00 MB |     507 |  16,722 |  15,448 |  14,327 |
| 2m         |  2.00 MB |     249 |   7,567 |   8,258 |   8,961 |
| 5m         |  5.00 MB |      99 |   3,549 |   3,660 |   3,878 |
| 10m        | 10.00 MB |      48 |   1,531 |   1,615 |   1,637 |
| interleaved (100k/200k/500k/1m, cycled) | — | 1,100 | 32,383 | 30,644 | 34,686 |

### Speed-up vs. cjson

| Scenario | `qjson.parse` / cjson | `qjson.decode + access content` / cjson |
|---|---:|---:|
| small  |  1.3× |  0.9× |
| medium | 21.5× | 22.8× |
| github-100k | 3.0× | 2.8× |
| 100k   | 32.8× | 31.5× |
| 200k   | 34.0× | 32.8× |
| 500k   | 31.5× | 33.1× |
| 1m     | 33.0× | 30.5× |
| 2m     | 30.4× | 33.2× |
| 5m     | 35.8× | 37.0× |
| 10m    | 31.9× | 33.6× |

## Results — memory delta (KB retained after 5 rounds)

Post-run `collectgarbage("count")` minus baseline. Captures heap usage after
the timing rounds without forcing a final collection, so short-lived garbage
from the last round may still be included.

| Scenario | cjson | `qjson.parse` | `qjson.decode + access content` | `qjson.decode + qjson.encode` |
|---|---:|---:|---:|---:|
| small      | +15,570 | +4,073 | +2,417 | +11,139 |
| medium     |  +1,955 |    +65 | +1,114 |  +1,120 |
| github-100k | +12,123 |    +19 |   +536 |    +230 |
| 100k       |    +484 |    +71 |   +692 |    +229 |
| 200k       |    +392 |    +34 |   +346 |    +112 |
| 500k       |    +577 |    +15 |   +140 |     +45 |
| 1m         |  +1,082 |    +10 |   +104 |     +34 |
| 2m         |  +1,155 |    +18 |   +208 |     +45 |
| 5m         |  +1,316 |    +14 |   +442 |     +45 |
| 10m        |  +1,583 |    +14 |   +762 |     +45 |
| interleaved | +3,356 |   +270 | +2,777 |    +897 |

`qjson.parse` retention is essentially constant across payload size: the only
GC-rooted state is the reusable `indices: Vec<u32>` and `scratch` buffers.
The `qjson.decode + ...` paths retain a bit more — a few Lua tables for the
lazy proxy and any cached child views — but still allocate one to two
orders of magnitude less than cjson, which materializes every key into the
Lua table heap.

## Observations

1. **`qjson` is fastest once payloads move beyond tiny inputs.**
   The small 2 KB row is dominated by fixed Lua/FFI overhead, but medium and
   larger multimodal payloads show roughly 21–36× higher throughput than
   `cjson` for request-field access.
2. **Reading every `messages[*].content` is still access-light for large
   multimodal bodies.** The benchmark touches the top-level request fields and
   one `content` field per message; the payload size comes from image data
   inside each message.
3. **Speedup remains high at 10 MB.** Unlike earlier versions, the
   eager-decode optimization keeps `qjson.parse` throughput scaling well
   even at the 10 MB level, maintaining ~32× over cjson.
4. **`qjson.decode + qjson.encode (unmodified)` is the headline number for
   passthrough workloads** — e.g. an LLM gateway re-emitting the original
   JSON after light-touch inspection. The substring fast path means
   re-emit is `memcpy`, not re-serialize, and the throughput tracks
   `qjson.parse` very closely.
5. **Memory retention** for `qjson` is essentially flat in payload
   size; cjson retains more Lua heap after the first run
   because the Lua table tree stays GC-rooted until the next collection.
   The 10 MB case retains ~1.5 MB for cjson
   and ~14 KB for `qjson.parse`.
6. **REST API payloads (github-100k) show a smaller speedup** because their
   structural density is higher than the multimodal request ladder. Memory
   savings remain dramatic because `cjson` must materialize every nested
   object and string into the Lua heap.

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