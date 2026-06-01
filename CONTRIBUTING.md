# Contributing

Thanks for helping improve qjson. The project is a Rust JSON decoder exposed to
LuaJIT through FFI, so changes often need both Rust and Lua verification.

## Development setup

Clone submodules before running the full test suite. The tests use vendored
JSON fixture corpora, and the benchmark target can build the vendored lua-cjson
module.

```sh
git submodule update --init --recursive
make build
```

Lua integration tests require LuaJIT, busted, and lua-cjson. You can either
install busted/lua-cjson with LuaRocks for Lua 5.1/LuaJIT, or build the vendored
lua-cjson module when you only need a local `cjson.so`:

```sh
make vendor/lua-cjson/cjson.so
```

If multiple Lua versions are installed, make sure the `busted` executable comes
from the LuaJIT or Lua 5.1 LuaRocks tree, not from a different Lua version.

## Running tests

The canonical PR gate is:

```sh
make test
```

That runs `cargo build --release`, `cargo test --release`, and the Lua busted
suite with the release cdylib on the dynamic loader path.

For narrower checks, use:

```sh
cargo test --release
cargo test --release --no-default-features
cargo test --features test-panic --release
```

Run a single Rust integration test with:

```sh
cargo test --release --test ffi_smoke parse_and_free_roundtrip
```

Run the Lua suite directly with:

```sh
cargo build --release
LD_LIBRARY_PATH=./target/release \
  busted --lua="$(command -v luajit)" tests/lua --lpath='./lua/?.lua'
```

On macOS, the release cdylib is named `libqjson.dylib`; if the direct Lua test
command cannot find qjson, add the dylib template to `LUA_CPATH`:

```sh
DYLD_LIBRARY_PATH=./target/release \
LUA_CPATH='./vendor/lua-cjson/?.so;./target/release/lib?.dylib;./target/release/lib?.so;./?.so;;' \
  busted --lua="$(command -v luajit)" tests/lua --lpath='./lua/?.lua'
```

## Linting and formatting

Run clippy with warnings denied:

```sh
make lint
```

`cargo fmt --check` is intentionally not part of the lint gate. Some Rust files
use manual column alignment in struct definitions and compact literals that
default rustfmt would reflow. Keep formatting consistent with nearby code.

## Commit messages

Use concise conventional-style prefixes when they fit the change, for example:

```text
docs: add cjson migration guide
test: cover lazy string validation
fix: preserve cursor byte spans
```

Release commits are stricter because `.github/workflows/release.yml` validates
the title. A release commit must be:

```text
feat: release vX.Y.Z
```

or, for prereleases:

```text
feat: release vX.Y.Z-prerelease
```

## Issues and discussions

Use GitHub Issues for actionable bugs, regressions, feature requests, missing
documentation, and follow-up work with concrete acceptance criteria. Use GitHub
Discussions for open-ended API design or migration questions when Discussions
are enabled; otherwise open an issue and make the exploratory status clear in
the description.

## FFI enum sync rule

The public error and type codes are duplicated for Rust, C, and Lua consumers.
When adding, removing, or renumbering any code, keep these files in sync in the
same change:

- `src/error.rs`
- `include/qjson.h`
- `lua/qjson.lua`

Add or update tests that prove the new value can cross the FFI boundary and is
visible from Lua when applicable. Renumbering existing codes is a breaking
change and must be called out in `CHANGELOG.md`.

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
cargo +nightly fuzz run fuzz_ffi_ops -- -max_total_time=60
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

The `fuzz_ffi_ops` target drives the public FFI surface with arbitrary
parse/get/cursor/free operation sequences. It focuses on panic-barrier and
pointer-safety regressions around null docs/cursors, path/key bytes, repeated
parses/frees, and mixed root/cursor accessors.

The `fuzz_parse_lazy` target is the Phase 2 semantic replay target. It compares
serde-accepted inputs by reconstructing a whole `serde_json::Value` through
qjson's public cursor FFI APIs, including ordered `object_entry_at` replay,
varied-order `cursor_field` / `cursor_index` lookups, and root getter vs cursor
getter consistency for path-safe unique-key paths. It normalizes numbers through
qjson's `f64` getter semantics, with serde_json's `float_roundtrip` parser
enabled for bit-exact `f64` oracle comparisons. Duplicate keys and path-like
keys are covered by ordered entry replay; they are not used for path getter
consistency because qjson path syntax cannot express those object members
unambiguously. Repeated varied-order sibling lookups exercise both cold and warm
skip-cache paths.

The committed corpus under `fuzz/corpus/fuzz_parse_eager/` is seeded from
JSONTestSuite `y_*`/`n_*`, cJSON fuzzing inputs, and benchmark fixtures. Crash
artifacts and coverage output are ignored; minimize and promote only useful
regression cases.

Before releases, run the same target much longer than the CI guard, for example:

```sh
cargo +nightly fuzz run fuzz_parse_eager -- -max_total_time=3600
cargo +nightly fuzz run fuzz_depth -- -max_total_time=3600
cargo +nightly fuzz run fuzz_ffi_ops -- -max_total_time=3600
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

## Lua lazy mutation property tests

Lazy mutation path coverage is in `tests/lua/lazy_mutation_property_spec.lua`
and uses deterministic defaults in the Makefile so PR runs are reproducible:

```sh
make lua-mutation-property-test
```

Stress this focused suite locally by overriding case/step count and seed:

```sh
make lua-mutation-property-test \
  QJSON_MUT_PROP_CASES=4000 \
  QJSON_MUT_PROP_STEPS=200 \
  QJSON_MUT_PROP_SEED=12345
```
