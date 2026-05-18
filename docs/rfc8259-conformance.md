# RFC 8259 conformance: implementation-defined cases

JSONTestSuite categorizes some inputs as `i_*` — the spec allows either
acceptance or rejection. This file records `qjson`'s behavior on
each, so changes show up in `git diff`.

Behavior is recorded for the default **EAGER** mode unless noted.

| File pattern | Our verdict | Rationale |
|---|---|---|
| `i_number_*` | ACCEPT | Eager validation checks JSON number grammar, not numeric representability. Overflow can still surface during typed access. |
| `i_object_key_lone_2nd_surrogate`, `i_string_*surrogate*` escaped with `\u` | ACCEPT | Eager parse validates escape syntax. Unicode scalar decoding is deferred until typed string access. |
| `i_string_*invalid*`, `i_string_*overlong*`, `i_string_*utf-8*`, `i_string_iso_latin_1`, `i_string_lone_utf8_continuation_byte`, `i_string_not_in_unicode_range` | REJECT (`QJSON_INVALID_UTF8`) | Raw string bytes must be valid UTF-8. |
| `i_string_UTF-16LE_with_BOM`, `i_string_utf16LE_no_BOM` | REJECT (`QJSON_TRAILING_CONTENT`) | UTF-16 input is outside the supported UTF-8 JSON text encoding. |
| `i_string_utf16BE_no_BOM`, `i_structure_UTF-8_BOM_empty_object` | REJECT (`QJSON_PARSE_ERROR`) | UTF-16 and leading BOM inputs are outside the accepted JSON text form. |
| `i_structure_500_nested_arrays` | ACCEPT (within default 1024 max_depth) | Configurable. |

Run `cargo test --release --test json_test_suite -- --nocapture` to print the
live verdict for every `i_*` file via the `document_i_files_behavior` test.
That is the source of truth for these entries; update this table when a
verdict changes (e.g. after a validator gap is closed).
