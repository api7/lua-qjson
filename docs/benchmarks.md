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

1. Warmup pass (≥ 50 iterations, or `iters / 5`) to let LuaJIT compile hot
   traces and the `qjson` `indices` / `scratch` buffers grow to their
   working size. Warmup is excluded from timing and the memory delta.
2. `collectgarbage("collect")` baseline.
3. 5 rounds × N iterations of the workload; report the **median** ops/s
   across rounds (mean + range also reported in the raw output).
4. Final `collectgarbage("count")` to capture the post-run memory delta in
   KB. The harness does not force a final collection after timing, so
   short-lived garbage from the last round may still be included.

**Fresh-process isolation (post PR #54).** `make bench` now launches a
separate `resty` process for each payload size (small, medium, 100k, …,
interleaved). This avoids accumulated GC state and JIT trace-cache pressure
from earlier payloads bleeding into later scenarios.

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
| `qjson.decode + modify top + encode` | `qjson.decode(s)`, mutate a top-level field, `qjson.encode()` | Triggers materialization of the root container + full re-encode |
| `qjson.decode + add field + encode` | `qjson.decode(s)`, add a new top-level field, `qjson.encode()` | Same as modify-top, plus a new key shaping the encode output |
| `qjson.decode + modify nested + encode` | `qjson.decode(s)`, mutate a deeply nested field, `qjson.encode()` | Only materializes the modified subtree branch; unmodified siblings stay on the fast path |

The new modify+encode scenarios were added in [#54](https://github.com/api7/lua-qjson/pull/54) to exercise the decode → mutate → re-encode pipeline end-to-end.

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
| small      |   2.1 KB |  92,716 | 102,602 | 128,005 | 125,815 | 260,322 |
| medium     |  60.4 KB |   9,007 |  82,699 | 116,198 | 219,491 | 141,563 |
| github-100k |   100 KB |   1,834 |   1,909 |   4,591 |   5,643 |   6,207 |
| 100k       |   100 KB |   2,769 |  40,437 |  84,034 | 121,803 | 105,374 |
| 200k       |   200 KB |   2,543 |  20,593 |  45,704 |  91,408 |  67,114 |
| 500k       |   500 KB |   1,047 |   8,218 |  28,852 |  37,580 |  29,334 |
| 1m         |  1.00 MB |     512 |   4,020 |  16,056 |  15,400 |  16,269 |
| 2m         |  2.00 MB |     251 |   2,105 |   9,145 |   9,137 |   9,634 |
| 5m         |  5.00 MB |     102 |     791 |   3,543 |   3,747 |   3,679 |
| 10m        | 10.00 MB |      51 |     363 |   1,830 |   1,783 |   1,749 |
| interleaved (100k/200k/500k/1m, cycled) | — | 1,125 | 9,701 | 34,173 | 36,278 | 36,456 |

### Modify + encode throughput (PR #54)

One-shot modify-then-encode benchmarks. Exercises the decode → mutate →
re-encode pipeline. Numbers below come from a 3-round per-scenario
fresh-process run on x86_64 Linux (AMD EPYC Rome, Zen 2).

| Scenario | modify top + encode | add field + encode | modify nested + encode |
|---|---|---:|---:|---:|
| small (2 KB)    | 58,242  | 58,190  | 43,003  |
| medium (60 KB)  | 37,498  | 45,364  | 134,590 |
| github-100k      |  4,419  |  3,964  |  4,359  |
| 100k (100 KB)   | 28,114  | 34,364  | 71,942  |
| 200k (200 KB)   | 18,282  | 16,932  | 55,127  |
| 500k (500 KB)   |  6,850  |  4,841  | 19,001  |
| 1m              |  3,125  |  2,998  | 13,649  |
| 2m              |  1,788  |  1,076  |  1,555  |
| 5m              |    366  |    283  |    215  |
| 10m             |    120  |     92  |     83  |
| interleaved     |  7,712  |  8,178  | 29,123  |

For a before/after comparison against the pre-#54 baseline, see the
[PR #54 benchmark comment](https://github.com/api7/lua-qjson/pull/54#issuecomment-4525477361).

### Speed-up vs. baselines

| Scenario | `qjson.parse` / cjson | `qjson.parse` / simdjson | `qjson.decode + access content` / cjson | `qjson.decode + access content` / simdjson |
|---|---:|---:|---:|---:|
| small  |  1.4× |  1.2× |  1.4× |  1.2× |
| medium | 12.9× |  1.4× | 24.4× |  2.7× |
| github-100k | 2.5× |  2.4× | 3.1× |  3.0× |
| 100k   | 30.3× |  2.1× | 44.0× |  3.0× |
| 200k   | 18.0× |  2.2× | 35.9× |  4.4× |
| 500k   | 27.6× |  3.5× | 35.9× |  4.6× |
| 1m     | 31.4× |  4.0× | 30.1× |  3.8× |
| 2m     | 36.4× |  4.3× | 36.4× |  4.3× |
| 5m     | 34.7× |  4.5× | 36.7× |  4.7× |
| 10m    | 35.9× |  5.0× | 35.0× |  4.9× |

## Results — memory delta (KB retained after 5 rounds)

Post-run `collectgarbage("count")` minus baseline. Captures heap usage after
the timing rounds without forcing a final collection, so short-lived garbage
from the last round may still be included.

| Scenario | cjson | simdjson | `qjson.parse` | `qjson.decode + access content` | `qjson.decode + qjson.encode` |
|---|---|---:|---:|---:|---:|---:|
| small      | +15,474 | +15,482 | +4,070 | +15,111 | +4,892 |
| medium     |  +1,955 |  +2,661 |   +158 |    +502 |    +558 |
| github-100k |  +4,218 |  +3,035 |    +28 |    +560 |     +96 |
| 100k       |    +485 |    +812 |    +39 |    +721 |     +96 |
| 200k       |    +393 |    +709 |    +22 |    +373 |     +54 |
| 500k       |    +885 |  +1,169 |    +30 |    +721 |     +96 |
| 1m         |  +1,255 |  +1,415 |    +26 |    +444 |     +69 |
| 2m         |  +1,155 |  +1,251 |    +19 |    +271 |     +27 |
| 5m         |  +1,316 |  +1,562 |    +20 |    +405 |     +31 |
| 10m        |  +1,584 |  +2,017 |    +24 |    +731 |     +47 |
| interleaved | +3,357 | +4,406 |   +100 |  +2,796 |    +354 |

`qjson.parse` retention is essentially constant across payload size: the only
GC-rooted state is the reusable `indices: Vec<u32>` and `scratch` buffers.
The `qjson.decode + ...` paths retain a bit more — a few Lua tables for the
lazy proxy and any cached child views — but still allocate one to two
orders of magnitude less than the eager parsers, which materialize every
key into the Lua table heap.

## Observations

1. **`qjson` is fastest once payloads move beyond tiny inputs.**
   The small 2 KB row is dominated by fixed Lua/FFI overhead, but medium and
   larger multimodal payloads show roughly 13–36× higher throughput than
   `cjson` and roughly 1.4–5× higher throughput than `lua-resty-simdjson`
   for request-field access.
2. **Reading every `messages[*].content` is still access-light for large
   multimodal bodies.** The benchmark touches the top-level request fields and
   one `content` field per message; the payload size comes from image data
   inside each message.
3. **Speedup remains high at 10 MB.** The eager-decode optimization
   keeps `qjson.parse` throughput scaling well even at the 10 MB level,
   maintaining ~36× over cjson and ~5× over simdjson.
4. **`qjson.decode + qjson.encode (unmodified)` is the headline number for
   passthrough workloads** — e.g. an LLM gateway re-emitting the original
   JSON after light-touch inspection. The substring fast path means
   re-emit is `memcpy`, not re-serialize, and the throughput tracks
   `qjson.parse` very closely.
5. **Memory retention** for `qjson` is essentially flat in payload
   size; the eager parsers retain more Lua heap after the first run
   because the Lua table tree stays GC-rooted until the next collection.
   The 10 MB case retains ~1.6 MB for `cjson`, ~2.0 MB for simdjson,
   and ~24 KB for `qjson.parse`.
6. **REST API payloads (github-100k) show a smaller speedup** because their
   structural density is higher than the multimodal request ladder. Memory
   savings remain dramatic because `cjson` must materialize every nested
   object and string into the Lua heap.
7. **Modify + encode pipeline (PR #54)** shows the lazy-table API in
   mutation mode. Small/medium payloads reach 43k–135k median ops/s.
   The `_dirty` flag and `TABLE_TYPE_HINT` side-table eliminate
   redundant tree walks and array/object re-scans inside the encoder.
   Large payloads (≥5 MB) are dominated by the root-container
   materialization cost, which copies all fields into a plain table.
8. **Fresh-process isolation** removes accumulated GC and JIT trace-cache
   interference between payload sizes. Each size now runs in its own
   `resty` process, eliminating the systemic cross-scenario variance
   observed in earlier benchmark runs.

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