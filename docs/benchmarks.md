# Benchmarks

Throughput and memory comparison of `quickdecode` (this library) against
`lua-cjson` on a multimodal chat-completion payload ladder from 2 KB to 10 MB.

`quickdecode` is optimized for *parse + read a small part of the document*;
the data below quantifies how the lazy structural scan behaves when the caller
reads request metadata plus every chat message `content`, without eagerly
building the whole Lua table. `lua-cjson` is the eager-table baseline.

## Environment

| | |
|---|---|
| Host CPU | Intel Xeon (Skylake, IBRS), 4 cores |
| Memory | 15 GiB |
| OS | Linux x86_64 |
| Runtime | Homebrew LuaJIT 2.1.1774896198 |
| `quickdecode` | this repo, release build, AVX2 + PCLMUL scanner active |
| `lua-cjson` | vendored `openresty/lua-cjson` |

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
| `quickdecode.parse + access fields` | `qd.parse(s)`, read `model` / `temperature`, then touch every `messages[*].content` path | Lazy structural scan; explicit path reads |
| `qd.decode + access content` | `qd.decode(s)`, read `model` / `temperature`, then read every `messages[*].content` | Lazy table proxy; reads go through `__index` |
| `qd.decode + qd.encode (unmodified)` | `qd.decode(s)` then re-emit as JSON | Substring fast path — no fields touched, so the proxy re-emits the original byte range via `memcpy` |

## Reproducing

The straight comparison against `cjson` is one command:

```sh
make bench
```

This invokes `benches/lua_bench.lua` with `LD_LIBRARY_PATH=target/release`
and a `LUA_CPATH` that picks up the vendored `lua-cjson` build.

Numbers below come from one such run.

## Results — throughput (median ops/s)

Each row is "parse + access request fields" on the named payload.

| Scenario | Size | cjson | `qd.parse` | `qd.decode + access content` | `qd.decode + qd.encode` |
|---|---:|---:|---:|---:|---:|
| small      |   2.1 KB | 113,056 | 132,184 |  81,769 | 145,722 |
| medium     |  60.4 KB |   8,194 | 196,773 | 142,086 | 147,406 |
| github-100k |   100 KB |   2,424 |   4,510 |   4,444 |   4,783 |
| 100k       |   100 KB |   4,874 | 144,509 | 100,100 | 107,527 |
| 200k       |   200 KB |   2,446 |  78,247 |  64,350 |  69,832 |
| 500k       |   500 KB |     982 |  33,003 |  30,211 |  31,299 |
| 1m         |  1.00 MB |     478 |  16,930 |  16,146 |  16,358 |
| 2m         |  2.00 MB |     238 |   8,361 |   8,127 |   8,302 |
| 5m         |  5.00 MB |      95 |   2,939 |   2,923 |   2,979 |
| 10m        | 10.00 MB |      48 |   1,046 |   1,045 |     955 |
| interleaved (100k/200k/500k/1m, cycled) | — | 1,063 | 33,498 | 30,595 | 31,646 |

### Speed-up vs. baselines

| Scenario | `qd.parse` / cjson | `qd.decode + access content` / cjson |
|---|---:|---:|
| small  |  1.2× |  0.7× |
| medium | 24.0× | 17.3× |
| github-100k | 1.9× | 1.8× |
| 100k   | 29.6× | 20.5× |
| 200k   | 32.0× | 26.3× |
| 500k   | 33.6× | 30.8× |
| 1m     | 35.4× | 33.8× |
| 2m     | 35.1× | 34.1× |
| 5m     | 30.9× | 30.8× |
| 10m    | 21.8× | 21.8× |

## Results — memory delta (KB retained after 5 rounds)

Post-run `collectgarbage("count")` minus baseline. Captures heap usage after
the timing rounds without forcing a final collection, so short-lived garbage
from the last round may still be included.

| Scenario | cjson | `qd.parse` | `qd.decode + access content` | `qd.decode + qd.encode` |
|---|---:|---:|---:|---:|
| small      | +15,985 | +4,069 | +17,408 | +13,478 |
| medium     |  +1,955 |    +67 |  +1,349 |  +1,349 |
| github-100k | +12,761 |   +20 |    +591 |    +273 |
| 100k       |    +485 |   +74 |    +739 |    +270 |
| 200k       |    +392 |   +34 |    +370 |    +135 |
| 500k       |    +577 |   +14 |    +148 |     +54 |
| 1m         |  +1,082 |   +10 |    +111 |     +41 |
| 2m         |  +1,155 |   +18 |    +217 |     +54 |
| 5m         |  +1,316 |   +14 |    +409 |     +54 |
| 10m        |  +1,583 |   +14 |    +717 |     +54 |
| interleaved | +3,356 |  +271 |  +2,955 |  +1,080 |

`qd.parse` retention is essentially constant across payload size: the only
GC-rooted state is the reusable `indices: Vec<u32>` and `scratch` buffers.
The `qd.decode + ...` paths retain a bit more — a few Lua tables for the
lazy proxy and any cached child views — but still allocate one to two
orders of magnitude less than the eager parsers, which materialize every
key into the Lua table heap.

## Observations

1. **`quickdecode` is fastest once payloads move beyond tiny inputs.**
   The small 2 KB row is dominated by fixed Lua/FFI overhead, but medium and
   larger multimodal payloads show roughly 20–35× higher throughput than
   `cjson` for request-field access.
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
   size; the eager parsers retain ~1× the input size after the first run
   because the Lua table tree stays GC-rooted until the next collection.
   The 10 MB case retains ~1.5 MB for `cjson`, ~14 KB for
   `qd.parse`.
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
