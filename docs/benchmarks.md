# Benchmarks

Throughput and memory comparison of `quickdecode` (this library) against
`lua-cjson` and `lua-resty-simdjson` on a multimodal chat-completion payload
ladder from 2 KB to 10 MB.

`quickdecode` is optimized for *parse + read a small part of the document*;
the data below quantifies how the lazy structural scan behaves when the caller
reads request metadata plus every chat message `content`, without eagerly
building the whole Lua table. `lua-cjson` and `lua-resty-simdjson` are eager
Lua-table baselines.

## Environment

| | |
|---|---|
| Host CPU | Intel Xeon (Skylake, IBRS), 4 cores |
| Memory | 15 GiB |
| OS | Linux x86_64 |
| Runtime | OpenResty `resty` 0.29 / LuaJIT 2.1.1723681758 |
| `quickdecode` | this repo, release build, AVX2 + PCLMUL scanner active |
| `lua-cjson` | vendored `openresty/lua-cjson` |
| `lua-resty-simdjson` | OpenResty lualib `resty.simdjson` |

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

The payload is a synthetic multimodal chat-completion request with multiple
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
| `quickdecode.parse + access fields` | `qd.parse(s)`, read `model` / `temperature`, then touch every `messages[*].content` path | Lazy structural scan; explicit path reads |
| `qd.decode + access content` | `qd.decode(s)`, read `model` / `temperature`, then read every `messages[*].content` | Lazy table proxy; reads go through `__index` |
| `qd.decode + qd.encode (unmodified)` | `qd.decode(s)` then re-emit as JSON | Substring fast path — no fields touched, so the proxy re-emits the original byte range via `memcpy` |

## Reproducing

Run the full comparison with one command:

```sh
make bench
```

This builds `quickdecode`, builds the vendored `lua-cjson` against OpenResty's
LuaJIT, then invokes `benches/lua_bench.lua` through OpenResty's `resty` so
`lua-resty-simdjson` runs in its normal `ngx` environment.

Numbers below come from one such run.

## Results — throughput (median ops/s)

Each row is "parse + access request fields" on the named payload.

| Scenario | Size | cjson | simdjson | `qd.parse` | `qd.decode + access content` | `qd.decode + qd.encode` |
|---|---:|---:|---:|---:|---:|---:|
| small      |   2.1 KB | 106,646 | 137,427 | 135,296 |  97,574 | 202,388 |
| medium     |  60.4 KB |  10,086 |  86,029 | 189,970 | 198,098 | 175,562 |
| github-100k |   100 KB |   2,208 |   2,880 |   4,496 |   4,479 |   4,809 |
| 100k       |   100 KB |   6,045 |  46,577 | 137,931 | 134,590 | 153,139 |
| 200k       |   200 KB |   3,025 |  22,563 |  78,247 |  75,873 |  81,433 |
| 500k       |   500 KB |   1,216 |   9,128 |  33,058 |  32,680 |  34,188 |
| 1m         |  1.00 MB |     594 |   4,408 |  16,447 |  16,340 |  16,722 |
| 2m         |  2.00 MB |     296 |   1,966 |   8,247 |   8,224 |   8,055 |
| 5m         |  5.00 MB |     118 |     600 |   2,869 |   2,945 |   2,992 |
| 10m        | 10.00 MB |      59 |     356 |   1,035 |   1,028 |   1,050 |
| interleaved (100k/200k/500k/1m, cycled) | — | 1,318 | 9,116 | 33,342 | 32,752 | 34,031 |

### Speed-up vs. baselines

| Scenario | `qd.parse` / cjson | `qd.parse` / simdjson | `qd.decode + access content` / cjson | `qd.decode + access content` / simdjson |
|---|---:|---:|---:|---:|
| small  |  1.3× |  1.0× |  0.9× |  0.7× |
| medium | 18.8× |  2.2× | 19.6× |  2.3× |
| github-100k | 2.0× |  1.6× | 2.0× |  1.6× |
| 100k   | 22.8× |  3.0× | 22.3× |  2.9× |
| 200k   | 25.9× |  3.5× | 25.1× |  3.4× |
| 500k   | 27.2× |  3.6× | 26.9× |  3.6× |
| 1m     | 27.7× |  3.7× | 27.5× |  3.7× |
| 2m     | 27.9× |  4.2× | 27.8× |  4.2× |
| 5m     | 24.3× |  4.8× | 25.0× |  4.9× |
| 10m    | 17.5× |  2.9× | 17.4× |  2.9× |

## Results — memory delta (KB retained after 5 rounds)

Post-run `collectgarbage("count")` minus baseline. Captures heap usage after
the timing rounds without forcing a final collection, so short-lived garbage
from the last round may still be included.

| Scenario | cjson | simdjson | `qd.parse` | `qd.decode + access content` | `qd.decode + qd.encode` |
|---|---:|---:|---:|---:|---:|
| small      | +15,464 | +15,447 | +4,094 | +15,251 | +11,908 |
| medium     |  +1,955 |  +2,660 |   +160 |  +1,210 |  +1,216 |
| github-100k | +13,187 | +3,362 |   +29 |    +548 |    +242 |
| 100k       |    +484 |   +748 |   +79 |    +704 |    +241 |
| 200k       |    +392 |   +523 |   +40 |    +352 |    +124 |
| 500k       |    +577 |   +630 |   +17 |    +142 |     +48 |
| 1m         |  +1,082 | +1,121 |   +13 |    +107 |     +37 |
| 2m         |  +1,155 | +1,248 |   +21 |    +211 |     +48 |
| 5m         |  +1,316 | +1,538 |   +17 |    +403 |     +48 |
| 10m        |  +1,583 | +2,014 |   +16 |    +844 |     +48 |
| interleaved | +3,355 | +4,404 |  +314 |  +2,825 |    +945 |

`qd.parse` retention is essentially constant across payload size: the only
GC-rooted state is the reusable `indices: Vec<u32>` and `scratch` buffers.
The `qd.decode + ...` paths retain a bit more — a few Lua tables for the
lazy proxy and any cached child views — but still allocate one to two
orders of magnitude less than the eager parsers, which materialize every
key into the Lua table heap.

## Observations

1. **`quickdecode` is fastest once payloads move beyond tiny inputs.**
   The small 2 KB row is dominated by fixed Lua/FFI overhead, but medium and
   larger multimodal payloads show roughly 18–28× higher throughput than
   `cjson` and roughly 3–5× higher throughput than `lua-resty-simdjson`
   for request-field access.
2. **Reading every `messages[*].content` is still access-light for large
   multimodal bodies.** The benchmark touches the top-level request fields and
   one `content` field per message; the payload size comes from image data
   inside each message.
3. **The win drops at 10 MB.** `qd.parse` is L3-bandwidth-bound at that
   size, and the `qd.decode` proxy's per-`__index` dispatch starts to
   amortize less well against the cheaper structural scan. `cjson` is still
   allocating into the table heap at that size, so the ratio remains large.
4. **`qd.decode + qd.encode (unmodified)` is the headline number for
   passthrough workloads** — e.g. an LLM gateway re-emitting the original
   JSON after light-touch inspection. The substring fast path means
   re-emit is `memcpy`, not re-serialize, and the throughput tracks
   `qd.parse` very closely.
5. **Memory retention** for `quickdecode` is essentially flat in payload
   size; the eager parsers retain more Lua heap after the first run
   because the Lua table tree stays GC-rooted until the next collection.
   The 10 MB case retains ~1.5 MB for `cjson`, ~2.0 MB for simdjson,
   and ~16 KB for `qd.parse`.
6. **REST API payloads (github-100k) show a smaller speedup** because their
   structural density is higher than the multimodal request ladder. Memory
   savings remain dramatic because `cjson` must materialize every nested
   object and string into the Lua heap.

## When to pick which

- **Read most/all fields** → `cjson`.
- **Parse, read selected fields, discard / re-emit** → `quickdecode`. The
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
  per byte and less raw `memcpy`, while the table-build cost on the eager
  side rises.
- `quickdecode` retains the source buffer on the `Doc`, so the input
  string stays alive for the document's lifetime. If you parse and
  immediately discard the JSON string in the caller, GC can still free
  the input — but only after the `Doc` is also unreachable.
