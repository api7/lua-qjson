# Contributing

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
