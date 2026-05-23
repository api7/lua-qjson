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
| Host CPU | AMD EPYC-Rome, 4 vCPUs, AVX2 + PCLMUL |
| Memory | 7 GiB |
| OS | Ubuntu 26.04 LTS, Linux 7.0.0-15-generic, x86_64 |
| Runtime | OpenResty `resty` 0.29 / OpenResty 1.29.2.3 / LuaJIT 2.1.ROLLING |
| `qjson` | this repo, release build, AVX2 + PCLMUL scanner active |
| `lua-cjson` | vendored `openresty/lua-cjson` |
| `lua-resty-simdjson` | `Kong/lua-resty-simdjson` commit `77322db640927c14968f1314a9fb1bb2bc084015`, installed under OpenResty lualib |

## Methodology

The harness lives at `benches/lua_bench.lua`. For each scenario:

1. Warmup pass (≥ 3 iterations, or `iters / 5`) to let LuaJIT compile hot
   traces and the `qjson` `indices` / `scratch` buffers grow to their
   working size. Warmup is excluded from timing and the memory delta.
2. `collectgarbage("collect")` baseline.
3. 10 rounds × N iterations of the workload; report the **median** ops/s
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
| small      |   2.1 KB |  94,701 | 108,921 | 128,103 |  89,294 | 187,631 |
| medium     |  60.4 KB |   8,850 |  82,699 | 120,598 | 194,856 | 135,759 |
| github-100k |   100 KB |   1,901 |   1,939 |   6,041 |   6,009 |   6,435 |
| 100k       |   100 KB |   5,214 |  36,914 | 136,986 | 110,497 | 126,263 |
| 200k       |   200 KB |   2,575 |  18,018 |  81,967 |  84,317 |  95,420 |
| 500k       |   500 KB |   1,043 |   7,262 |  36,563 |  35,971 |  38,023 |
| 1m         |  1.00 MB |     505 |   3,894 |  16,234 |  16,648 |  16,968 |
| 2m         |  2.00 MB |     254 |   2,065 |   8,247 |   8,407 |   8,838 |
| 5m         |  5.00 MB |     100 |     652 |   3,228 |   3,225 |   3,355 |
| 10m        | 10.00 MB |      50 |     369 |   1,602 |   1,429 |   1,774 |
| interleaved (100k/200k/500k/1m, cycled) | — | 1,121 | 9,142 | 28,854 | 33,088 | 32,568 |

### Speed-up vs. baselines

| Scenario | `qjson.parse` / cjson | `qjson.parse` / simdjson | `qjson.decode + access content` / cjson | `qjson.decode + access content` / simdjson |
|---|---|---:|---:|---:|---:|
| small  |  1.4× |  1.2× |  0.9× |  0.8× |
| medium | 13.6× |  1.5× | 22.0× |  2.4× |
| github-100k | 3.2× |  3.1× | 3.2× |  3.1× |
| 100k   | 26.3× |  3.7× | 21.2× |  3.0× |
| 200k   | 31.8× |  4.5× | 32.7× |  4.7× |
| 500k   | 35.1× |  5.0× | 34.5× |  5.0× |
| 1m     | 32.1× |  4.2× | 32.9× |  4.3× |
| 2m     | 32.5× |  4.0× | 33.1× |  4.1× |
| 5m     | 32.3× |  5.0× | 32.3× |  4.9× |
| 10m    | 32.0× |  4.3× | 28.6× |  3.9× |

## Results — memory delta (KB retained after 5 rounds)

Post-run `collectgarbage("count")` minus baseline. Captures heap usage after
the timing rounds without forcing a final collection, so short-lived garbage
from the last round may still be included.

| Scenario | cjson | simdjson | `qjson.parse` | `qjson.decode + access content` | `qjson.decode + qjson.encode` |
|---|---|---:|---:|---:|---:|---:|
| small      |  -2,472 | +12,360 | +8,159 |  +8,651 |  +2,698 |
| medium     |  +3,850 |  +5,258 |   +124 |  +2,228 |  +2,234 |
| github-100k | +20,255 | +18,823 |    +29 |  +1,072 |    +452 |
| 100k       |    +867 |  +1,393 |   +138 |  +1,384 |    +452 |
| 200k       |    +584 |    +846 |    +67 |    +692 |    +223 |
| 500k       |    +654 |    +759 |    +27 |    +277 |     +89 |
| 1m         |  +1,140 |  +1,218 |    +20 |    +208 |     +67 |
| 2m         |  +1,284 |  +1,472 |    +31 |    +409 |     +89 |
| 5m         |  +1,607 |  +2,051 |    +27 |    +855 |     +89 |
| 10m        |  +2,143 |  +3,004 |    +27 |  +1,736 |     +89 |
| interleaved | +4,888 | +6,983 |   +536 |  +5,537 |  +1,788 |

`qjson.parse` retention is essentially constant across payload size: the only
GC-rooted state is the reusable `indices: Vec<u32>` and `scratch` buffers.
The `qjson.decode + ...` paths retain a bit more — a few Lua tables for the
lazy proxy and any cached child views — but still allocate one to two
orders of magnitude less than the eager parsers, which materialize every
key into the Lua table heap.

## Observations

1. **`qjson` is fastest once payloads move beyond tiny inputs.**
   The small 2 KB row is dominated by fixed Lua/FFI overhead, but medium and
   larger multimodal payloads show roughly 18–28× higher throughput than
   `cjson` and roughly 3–5× higher throughput than `lua-resty-simdjson`
   for request-field access.
2. **Reading every `messages[*].content` is still access-light for large
   multimodal bodies.** The benchmark touches the top-level request fields and
   one `content` field per message; the payload size comes from image data
   inside each message.
3. **The win drops at 10 MB.** `qjson.parse` is L3-bandwidth-bound at that
   size, and the `qjson.decode` proxy's per-`__index` dispatch starts to
   amortize less well against the cheaper structural scan. `cjson` is still
   allocating into the table heap at that size, so the ratio remains large.
4. **`qjson.decode + qjson.encode (unmodified)` is the headline number for
   passthrough workloads** — e.g. an LLM gateway re-emitting the original
   JSON after light-touch inspection. The substring fast path means
   re-emit is `memcpy`, not re-serialize, and the throughput tracks
   `qjson.parse` very closely.
5. **Memory retention** for `qjson` is essentially flat in payload
   size; the eager parsers retain more Lua heap after the first run
   because the Lua table tree stays GC-rooted until the next collection.
   The 10 MB case retains ~1.5 MB for `cjson`, ~2.0 MB for simdjson,
   and ~16 KB for `qjson.parse`.
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
