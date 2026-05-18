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
| `cjson.decode + access fields` | `cjson.decode(s)`, read `model` / `temperature`, then read every `message.content` | Eager Lua table |
| `quickdecode.parse + access fields` | `qd.parse(s)`, read `model` / `temperature`, then read every `messages[i].content` type | Lazy structural scan; explicit path-based reads |
| `qd.decode + access content` | `qd.decode(s)`, read `model` / `temperature`, then read every `message.content` | Lazy table proxy; reads go through `__index` |
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

| Scenario | Size | cjson | `qd.parse` | `qd.decode + access` | `qd.decode + qd.encode` |
|---|---:|---:|---:|---:|---:|
| small      |   2.1 KB | 114,427 | 133,536 |  82,816 | 146,101 |
| medium     |  60.4 KB |   8,178 | 161,342 | 143,719 | 147,275 |
| github-100k |   100 KB |   2,431 |   4,474 |   4,449 |   4,774 |
| 100k       |   100 KB |   4,865 | 135,501 | 102,987 | 114,943 |
| 200k       |   200 KB |   2,443 |  72,780 |  62,189 |  67,295 |
| 500k       |   500 KB |     979 |  32,000 |  29,412 |  30,534 |
| 1m         |  1.00 MB |     478 |  16,538 |  15,723 |  16,043 |
| 2m         |  2.00 MB |     238 |   8,319 |   8,055 |   8,183 |
| 5m         |  5.00 MB |      94 |   2,933 |   2,926 |   2,982 |
| 10m        | 10.00 MB |      47 |   1,015 |   1,042 |   1,065 |
| interleaved (100k/200k/500k/1m, cycled) | — | 1,066 | 32,717 | 29,089 | 30,969 |

### Speed-up vs. baselines

| Scenario | `qd.parse` / cjson | `qd.decode + access` / cjson |
|---|---:|---:|
| small  |  1.2× |  0.7× |
| medium | 19.7× | 17.6× |
| github-100k | 1.8× | 1.8× |
| 100k   | 27.9× | 21.2× |
| 200k   | 29.8× | 25.5× |
| 500k   | 32.7× | 30.0× |
| 1m     | 34.6× | 32.9× |
| 2m     | 35.0× | 33.8× |
| 5m     | 31.2× | 31.1× |
| 10m    | 21.6× | 22.2× |

## Results — memory delta (KB retained after 5 rounds)

Post-run `collectgarbage("count")` minus baseline. Captures GC-rooted state
the parser retains across iterations; transient per-call allocations are
collected before the snapshot.

| Scenario | cjson | `qd.parse` | `qd.decode + access` | `qd.decode + qd.encode` |
|---|---:|---:|---:|---:|
| small      | +15,974 | +4,069 | +17,425 | +13,478 |
| medium     |  +1,955 |    +81 |  +1,349 |  +1,349 |
| github-100k | +12,655 |   +83 |    +592 |    +273 |
| 100k       |    +601 |   +77 |    +739 |    +270 |
| 200k       |    +506 |   +34 |    +370 |    +135 |
| 500k       |    +648 |   +14 |    +149 |     +54 |
| 1m         |  +1,151 |   +10 |    +111 |     +41 |
| 2m         |  +2,312 |   +14 |    +148 |     +55 |
| 5m         |  +5,723 |   +14 |    +148 |     +55 |
| 10m        | +11,262 |   +14 |    +148 |     +54 |
| interleaved | +4,508 |   +70 |  +3,002 |  +1,079 |

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
2. **Reading every message `content` is still access-light for large
   multimodal bodies.** The benchmark touches the top-level request fields and
   one `content` field per message, but it does not materialize every nested
   image part or base64 string unless that field is read through the lazy table
   API.
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
   The 10 MB case retains ~11 MB for `cjson`, ~14 KB for
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
