# Real-world fixture manifest

`manifest.json` is the single source of truth for qjson's real-world test
corpus. Both the Rust correctness gate (`tests/manifest_fixtures.rs`) and the
Lua benchmark harness (`benches/lua_bench.lua` via `benches/manifest.lua`) read
it, so fixture paths, access paths and expected values are declared in exactly
one place instead of being duplicated across the two suites.

Originating issue: [#139](https://github.com/api7/lua-qjson/issues/139).

## Format

`manifest.json` is plain JSON so both consumers can parse it with tools they
already have — `serde_json` (a Rust dev-dependency) on the Rust side and
`lua-cjson` on the Lua side. The manifest is deliberately **not** parsed with
qjson itself, to avoid a bug in qjson breaking the harness that is supposed to
catch it.

All `path` values are **repo-root relative**. Both consumers run from the
repository root (`CARGO_MANIFEST_DIR` for Rust, the `Makefile` working directory
for Lua).

## Schema

```jsonc
{
  "version": 1,
  "fixtures": [
    {
      "id": "rest_api_small",          // unique identifier
      "path": "benches/fixtures/small_api.json",
      "source": "qjson bench fixture", // human-readable origin
      "payload_type": "rest_api",      // rest_api | unicode_heavy | wide_object
                                       // | deep_nesting | ndjson
      "format": "json",                // json | ndjson
      "size_bytes": 2115,              // informational
      "structural_density": "medium",  // optional: low | medium | high
      "workloads": ["parse_access", "decode_access",
                    "decode_encode", "modify_encode"],
      "ci": ["pr", "scheduled", "bench"], // where the fixture is exercised
      "bench_iters": 5000,             // optional: benchmark iteration count
      "checks": [ /* see below */ ]
    }
  ]
}
```

### Check entries

Each check declares one access path and its expected result as a
`type` + optional `value` + optional `len` triple:

```jsonc
{ "path": "info.version", "type": "string", "value": "1.0.0" }
{ "path": "messages",     "type": "array",  "len": 4 }
{ "record": 2, "path": "[1]", "type": "string", "value": "Motorola" }
```

- `path` — qjson path syntax: dotted keys plus `[i]` indices
  (e.g. `choices[0].finish_reason`). The empty string `""` addresses the
  document root (useful for scalar roots and for `len` on the top-level
  container).
- `type` — one of `string | number | bool | null | object | array`. Always
  checked via `qjson_typeof`.
- `value` — optional expected value for scalars (`string` / `number` / `bool`).
  Omitted for `null` and containers. See the number note below.
- `len` — optional. For `string` it is the decoded **byte** length; for
  `object` / `array` it is the member / element count.
- `record` — **NDJSON only**: 0-based line index selecting which record the
  `path` is resolved against.

### Numbers

`value` for a `number` check is stored as a JSON number and compared as `f64`
with exact equality. Pick fixture values that are exactly representable as
`f64` (integers, short decimals like `0.5`). Avoid values such as `2.9` that
have no exact binary representation; check the type only, or assert on a
neighbouring exact field instead.

### NDJSON

A fixture with `"format": "ndjson"` is split on `\n` (a trailing `\r` is
trimmed) and each non-empty line is parsed as an independent qjson document.
Checks select a line with `record` (default `0`) and resolve `path` within that
record. NDJSON fixtures are correctness-only; the Lua latency benchmark skips
them.

## Adding a fixture

1. Prefer reusing an existing corpus file: `benches/fixtures/*`,
   `tests/vendor/simdjson/jsonexamples/*`, `tests/vendor/cJSON/tests/*`, or a
   `JSONTestSuite` sample. Only add a new file under `tests/fixtures/data/` when
   no existing payload fits (e.g. the generated `wide_object.json` /
   `deep_nesting.json`).
2. Add a fixture entry to `manifest.json` with its `id`, `path`,
   `payload_type`, `format`, `ci`, `workloads` and at least one `check`.
3. Add checks for the key access paths you want guarded, using values read from
   the actual file.
4. Run the Rust gate:

   ```sh
   cargo test --release --test manifest_fixtures
   ```

   It fails loudly if a path, type, value or length does not match the file.
5. To exercise the fixture in the benchmark, include `"bench"` in `ci` and set
   `bench_iters`; `make bench` (or `... lua_bench.lua manifest`) will pick it up.

## Fixture sources and licenses

| Fixture | Source | License |
|---------|--------|---------|
| `twitter.json` | [simdjson/jsonexamples](https://github.com/simdjson/simdjson/tree/master/jsonexamples) | Apache-2.0 |
| `amazon_cellphones.ndjson` | simdjson/jsonexamples | Apache-2.0 |
| `citm_catalog.json` | simdjson/jsonexamples (CitmCatalog benchmark) | Apache-2.0 |
| `k8s_openapi.json` | [Kubernetes OpenAPI spec](https://github.com/kubernetes/kubernetes/tree/master/api/openapi-spec) | Apache-2.0 |
| `github_prs.json` | [GitHub REST API v3](https://docs.github.com/en/rest) (api7/lua-qjson public repo) | Public data ([GitHub ToS](https://docs.github.com/en/site-policy/github-terms/github-terms-of-service)) |
| `small_api.json` | qjson synthetic | Project license |
| `medium_resp.json` | qjson synthetic | Project license |
| `wide_object.json` | qjson generated | Project license |
| `deep_nesting.json` | qjson generated | Project license |
