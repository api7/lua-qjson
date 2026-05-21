# Third-party JSON fixture sources

qjson reuses mature upstream JSON test data through git submodules instead of
copying large C/C++ test harnesses into this repository.

- `tests/vendor/cJSON`: `DaveGamble/cJSON`, MIT licensed. Rust and Lua tests
  consume every `tests/inputs/test*` JSON fixture with matching `.expected`
  files, the JSON files from `tests/json-patch-tests/`, and Rust ports parser
  literals from cJSON number/string/array tests. `tests/inputs/test6` is an
  upstream HTML error page, so qjson keeps it as a negative parse case.
- `tests/vendor/simdjson`: `simdjson/simdjson`, dual Apache-2.0/MIT licensed.
  qjson uses the MIT option and consumes every single-document `.json` file in
  `jsonexamples/`; the `.ndjson` streaming example is split by line so every
  record is parsed as an individual JSON document.

The upstream submodules carry their own license files:

- `tests/vendor/cJSON/LICENSE`
- `tests/vendor/simdjson/LICENSE-MIT`

The local Rust and Lua harnesses are qjson tests; the upstream C/C++ harnesses
are left in the submodules as source material rather than compiled here.
