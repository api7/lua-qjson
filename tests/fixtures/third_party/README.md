# Third-party JSON fixtures

This directory contains a small, selected corpus adapted from upstream JSON
parser test data. The C/C++ test harnesses are not vendored; qjson consumes
these files through its own Rust and Lua test suites.

## DaveGamble/cJSON

- Source: https://github.com/DaveGamble/cJSON
- Upstream paths: `tests/inputs/test1`, `test2`, `test9`, `test10`, `test11`
- License: MIT
- Copyright: Copyright (c) 2009-2017 Dave Gamble and cJSON contributors

The MIT license notice from upstream requires preserving the copyright and
permission notice with copied substantial portions.

## simdjson/simdjson

- Source: https://github.com/simdjson/simdjson
- Upstream paths: `jsonexamples/example_config.json` plus selected DOM test
  literals from `tests/dom/big_integer_tests.cpp`
- License choice for copied material: MIT
- Copyright: Copyright 2018-2025 The simdjson authors

simdjson is dual-licensed under Apache-2.0 and MIT; qjson uses the MIT option
for this copied test material.
