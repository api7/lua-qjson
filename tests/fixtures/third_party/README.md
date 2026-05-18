# Third-party JSON fixture sources

qjson reuses mature upstream JSON test data through git submodules instead of
copying large C/C++ test harnesses into this repository.

- `tests/vendor/cJSON`: `DaveGamble/cJSON`, MIT licensed. Rust and Lua tests
  consume `tests/inputs/*` fixtures with matching `.expected` files, and Rust
  ports selected parser literals from cJSON number/string/array tests.
- `tests/vendor/simdjson`: `simdjson/simdjson`, dual Apache-2.0/MIT licensed.
  qjson uses the MIT option and consumes the single-document `.json` files in
  `jsonexamples/`; the `.ndjson` streaming example is intentionally excluded.

The upstream submodules carry their own license files:

- `tests/vendor/cJSON/LICENSE`
- `tests/vendor/simdjson/LICENSE-MIT`

The local Rust and Lua harnesses are qjson tests; the upstream C/C++ harnesses
are left in the submodules as source material rather than compiled here.
