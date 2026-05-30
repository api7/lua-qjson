# Contributing

## Changelog policy

Any pull request that changes public behavior must add an entry under the
`[Unreleased]` section in `CHANGELOG.md`. Public behavior includes changes to
the Lua API, FFI surface, documented semantics, validation behavior, error/type
codes, release packaging, or any user-visible compatibility/performance
contract.

Pure internal refactors do not need a changelog entry. If a pull request touches
`src/`, `lua/`, or `include/` without changing public behavior, call that out in
the PR description so reviewers can intentionally skip the changelog update.

## Fuzzing

qjson parses arbitrary bytes at an API gateway trust boundary. The Rust decoder
has a cargo-fuzz harness for parser regression checks.

Install the local fuzzing tools:

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz
```

Run the PR-length regression guard:

```sh
cargo +nightly fuzz run fuzz_parse_eager -- -max_total_time=60
cargo +nightly fuzz run fuzz_depth -- -max_total_time=60
cargo +nightly fuzz run fuzz_parse_lazy -- -max_total_time=60
```

The `fuzz_parse_eager` target compares qjson EAGER parse accept/reject behavior
against `serde_json::Value`. It skips inputs deeper than 64 containers because
deep nesting has its own target. It also documents two `serde_json::Value`
rejections that qjson accepts at parse time: numbers outside serde's numeric
range and escaped unpaired UTF-16 surrogates. The latter is guarded by an
input-level surrogate check because serde may report a leading surrogate as an
unexpected end of hex escape; qjson rejects those strings later if decoded.

The `fuzz_depth` target is non-differential. It pins qjson's nesting contract:
depth `N` is accepted and `N+1` returns `QJSON_NESTING_TOO_DEEP` at both the
default depth (`1024`) and the clamped ceiling (`4096`). Accepted boundary
inputs are also walked through the FFI cursor API to exercise Phase 2 without
recursive descent.

The `fuzz_parse_lazy` target compares serde-accepted inputs by reconstructing a
whole `serde_json::Value` through qjson's public cursor FFI APIs. It normalizes
numbers through qjson's `f64` getter semantics, with serde_json's
`float_roundtrip` parser enabled for bit-exact `f64` oracle comparisons, and
performs repeated varied-order sibling lookups so both cold and warm skip-cache
paths are covered.

The committed corpus under `fuzz/corpus/fuzz_parse_eager/` is seeded from
JSONTestSuite `y_*`/`n_*`, cJSON fuzzing inputs, and benchmark fixtures. Crash
artifacts and coverage output are ignored; minimize and promote only useful
regression cases.

Before releases, run the same target much longer than the CI guard, for example:

```sh
cargo +nightly fuzz run fuzz_parse_eager -- -max_total_time=3600
cargo +nightly fuzz run fuzz_depth -- -max_total_time=3600
cargo +nightly fuzz run fuzz_parse_lazy -- -max_total_time=3600
```

CI intentionally runs only a short 60-second fuzzing pass so pull requests get a
quick regression signal without pretending to be a deep fuzz campaign.

## Lua encode property tests

`qjson.encode` and `qjson.materialize` live in Lua (`lua/qjson/table.lua`), so
they are outside cargo-fuzz's Rust decoder targets. Lua-side round-trip coverage
uses deterministic busted property tests instead of luzer for now: busted is
already installed in CI and gives a portable PR regression guard without adding
a LuaJIT/libFuzzer binding dependency. luzer can still be revisited later for
long-running coverage-guided Lua fuzzing.

Run the default PR-length guard:

```sh
make lua-property-test
```

Increase the generated case count or pin a different deterministic seed when
investigating locally:

```sh
make lua-property-test QJSON_PROP_CASES=1000 QJSON_PROP_SEED=12345
```

The property suite generates valid JSON containers, runs
`decode -> materialize -> encode -> decode -> materialize`, checks structural
equality, and probes the encoder max-depth boundary around 1000 nested
containers.
