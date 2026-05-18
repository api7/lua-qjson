# RFC 8259 conformance: implementation-defined cases

JSONTestSuite categorizes some inputs as `i_*` — the spec allows either
acceptance or rejection. This file records `lua-quick-decode`'s behavior on
each, so changes show up in `git diff`.

Behavior is recorded for the default **EAGER** mode unless noted.

| File pattern | Our verdict | Rationale |
|---|---|---|
| `i_number_huge_exp` | REJECT (`QJD_NUMBER_OUT_OF_RANGE`) | f64 overflow surfaces at decode. |
| `i_number_very_big_negative_int` | varies — see below | ABNF-valid; representational, not structural. |
| `i_string_*` (UTF-16 surrogate halves in `\u` escapes) | REJECT (`QJD_DECODE_FAILED`) | We require well-formed surrogate pairs. |
| `i_structure_500_nested_arrays` | ACCEPT (within default 1024 max_depth) | Configurable. |

Run `cargo test --release --test json_test_suite -- --nocapture` to print the
live verdict for every `i_*` file via the `document_i_files_behavior` test.
That is the source of truth for these entries; update this table when a
verdict changes (e.g. after a validator gap is closed).
