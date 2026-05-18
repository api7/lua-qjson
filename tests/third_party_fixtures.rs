use std::fs;
use std::os::raw::c_int;
use std::path::{Path, PathBuf};
use std::ptr;

use qjson::doc::Document;
use qjson::error::qjson_err;
use qjson::ffi::*;
use qjson::options::{Options, QJSON_MODE_EAGER, QJSON_MODE_LAZY};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn parse(s: &[u8]) -> *mut qjson_doc {
    let mut err: c_int = -1;
    let d = unsafe { qjson_parse(s.as_ptr(), s.len(), &mut err) };
    assert_eq!(err, qjson_err::QJSON_OK as c_int);
    assert!(!d.is_null());
    d
}

fn parse_file(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn assert_parses_in_both_modes(name: &str, data: &[u8]) {
    let eager = Options {
        mode: QJSON_MODE_EAGER,
        max_depth: 0,
    };
    let lazy = Options {
        mode: QJSON_MODE_LAZY,
        max_depth: 0,
    };

    Document::parse_with_options(data, &eager)
        .unwrap_or_else(|e| panic!("{name} rejected in eager mode: {e:?}"));
    Document::parse_with_options(data, &lazy)
        .unwrap_or_else(|e| panic!("{name} rejected in lazy mode: {e:?}"));
}

fn cjson_input_cases() -> Vec<PathBuf> {
    let dir = repo_root().join("tests/vendor/cJSON/tests/inputs");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("missing cJSON submodule at {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                return false;
            };
            name.starts_with("test")
                && !name.ends_with(".expected")
                && path.with_file_name(format!("{name}.expected")).is_file()
        })
        .collect();
    paths.sort();
    paths
}

fn cjson_json_patch_cases() -> Vec<PathBuf> {
    let dir = repo_root().join("tests/vendor/cJSON/tests/json-patch-tests");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("missing cJSON json-patch-tests at {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    paths.sort();
    paths
}

fn simdjson_example_cases() -> Vec<PathBuf> {
    let dir = repo_root().join("tests/vendor/simdjson/jsonexamples");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("missing simdjson submodule at {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    paths.sort();
    paths
}

fn simdjson_ndjson_cases() -> Vec<PathBuf> {
    let dir = repo_root().join("tests/vendor/simdjson/jsonexamples");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("missing simdjson submodule at {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("ndjson"))
        .collect();
    paths.sort();
    paths
}

fn get_str(doc: *mut qjson_doc, path: &[u8]) -> String {
    let mut p: *const u8 = ptr::null();
    let mut n: usize = 0;
    let rc = unsafe { qjson_get_str(doc, path.as_ptr() as *const i8, path.len(), &mut p, &mut n) };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    String::from_utf8(unsafe { std::slice::from_raw_parts(p, n) }.to_vec()).unwrap()
}

fn get_i64(doc: *mut qjson_doc, path: &[u8]) -> i64 {
    let mut v: i64 = 0;
    let rc = unsafe { qjson_get_i64(doc, path.as_ptr() as *const i8, path.len(), &mut v) };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    v
}

fn get_bool(doc: *mut qjson_doc, path: &[u8]) -> bool {
    let mut v: c_int = -1;
    let rc = unsafe { qjson_get_bool(doc, path.as_ptr() as *const i8, path.len(), &mut v) };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    v != 0
}

fn is_null(doc: *mut qjson_doc, path: &[u8]) -> bool {
    let mut v: c_int = -1;
    let rc = unsafe { qjson_is_null(doc, path.as_ptr() as *const i8, path.len(), &mut v) };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    v != 0
}

fn len(doc: *mut qjson_doc, path: &[u8]) -> usize {
    let mut n: usize = 0;
    let rc = unsafe { qjson_len(doc, path.as_ptr() as *const i8, path.len(), &mut n) };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    n
}

fn open(doc: *mut qjson_doc, path: &[u8]) -> qjson_cursor {
    let mut cur = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let rc = unsafe {
        qjson_open(
            doc,
            path.as_ptr() as *const i8,
            path.len(),
            cur.as_mut_ptr(),
        )
    };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    unsafe { cur.assume_init() }
}

fn cursor_index(cur: &qjson_cursor, index: usize) -> qjson_cursor {
    let mut sub = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let rc = unsafe { qjson_cursor_index(cur, index, sub.as_mut_ptr()) };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    unsafe { sub.assume_init() }
}

fn cursor_get_str(cur: &qjson_cursor) -> String {
    let empty = b"";
    let mut p: *const u8 = ptr::null();
    let mut n: usize = 0;
    let rc = unsafe { qjson_cursor_get_str(cur, empty.as_ptr() as *const i8, 0, &mut p, &mut n) };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    String::from_utf8(unsafe { std::slice::from_raw_parts(p, n) }.to_vec()).unwrap()
}

#[test]
fn cjson_input_corpus_parses_in_both_modes() {
    let cases = cjson_input_cases();
    assert!(
        cases.len() >= 10,
        "expected the cJSON input corpus, got {} files",
        cases.len()
    );

    for path in cases {
        let name = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        assert_parses_in_both_modes(&name, &parse_file(&path));

        let expected = path.with_file_name(format!(
            "{}.expected",
            path.file_name().unwrap().to_string_lossy()
        ));
        let expected_name = expected
            .strip_prefix(repo_root())
            .unwrap_or(&expected)
            .display()
            .to_string();
        assert_parses_in_both_modes(&expected_name, &parse_file(&expected));
    }
}

#[test]
fn cjson_json_patch_corpus_parses_in_both_modes() {
    let cases = cjson_json_patch_cases();
    assert!(
        cases.len() >= 4,
        "expected the cJSON json-patch-tests corpus, got {} files",
        cases.len()
    );

    for path in cases {
        let name = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        assert_parses_in_both_modes(&name, &parse_file(&path));
    }
}

#[test]
fn cjson_non_json_input_is_rejected() {
    let path = repo_root().join("tests/vendor/cJSON/tests/inputs/test6");
    let data = parse_file(&path);
    assert!(
        Document::parse(&data).is_err(),
        "cJSON test6 is an HTML error page and must not parse as JSON"
    );
}

#[test]
fn simdjson_jsonexamples_parse_in_both_modes() {
    let cases = simdjson_example_cases();
    assert!(
        !cases.is_empty(),
        "expected at least one single-document JSON file in simdjson jsonexamples"
    );

    for path in cases {
        let name = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .display()
            .to_string();
        assert_parses_in_both_modes(&name, &parse_file(&path));
    }
}

#[test]
fn simdjson_ndjson_examples_parse_each_record_in_both_modes() {
    let cases = simdjson_ndjson_cases();
    assert!(
        !cases.is_empty(),
        "expected at least one simdjson ndjson example file"
    );

    let mut records = 0;
    for path in cases {
        let data = parse_file(&path);
        for (line_no, line) in data.split(|b| *b == b'\n').enumerate() {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            records += 1;
            let name = format!(
                "{}:{}",
                path.strip_prefix(repo_root()).unwrap_or(&path).display(),
                line_no + 1
            );
            assert_parses_in_both_modes(&name, line);
        }
    }

    assert!(records >= 793, "expected simdjson NDJSON records, got {records}");
}

#[test]
fn cjson_nested_object_fixture_paths_are_accessible() {
    let data = parse_file(&repo_root().join("tests/vendor/cJSON/tests/inputs/test1"));
    let doc = parse(&data);

    assert_eq!(get_str(doc, b"glossary.title"), "example glossary");
    assert_eq!(
        get_str(doc, b"glossary.GlossDiv.GlossList.GlossEntry.ID"),
        "SGML"
    );
    assert_eq!(
        get_str(doc, b"glossary.GlossDiv.GlossList.GlossEntry.GlossDef.para"),
        "A meta-markup language, used to create markup languages such as DocBook."
    );

    let see_also = b"glossary.GlossDiv.GlossList.GlossEntry.GlossDef.GlossSeeAlso";
    assert_eq!(len(doc, see_also), 2);
    let cur = open(doc, see_also);
    assert_eq!(cursor_get_str(&cursor_index(&cur, 0)), "GML");
    assert_eq!(cursor_get_str(&cursor_index(&cur, 1)), "XML");

    unsafe { qjson_free(doc) };
}

#[test]
fn cjson_menu_and_matrix_fixtures_keep_array_shape() {
    let menu_data = parse_file(&repo_root().join("tests/vendor/cJSON/tests/inputs/test2"));
    let menu_doc = parse(&menu_data);
    assert_eq!(get_str(menu_doc, b"menu.id"), "file");

    let items = open(menu_doc, b"menu.popup.menuitem");
    assert_eq!(len(menu_doc, b"menu.popup.menuitem"), 3);
    let second = cursor_index(&items, 1);
    let mut onclick = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let rc = unsafe {
        qjson_cursor_field(
            &second,
            b"onclick".as_ptr() as *const i8,
            b"onclick".len(),
            onclick.as_mut_ptr(),
        )
    };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    assert_eq!(
        cursor_get_str(&unsafe { onclick.assume_init() }),
        "OpenDoc()"
    );
    unsafe { qjson_free(menu_doc) };

    let matrix_data = parse_file(&repo_root().join("tests/vendor/cJSON/tests/inputs/test9"));
    let matrix_doc = parse(&matrix_data);
    let root = open(matrix_doc, b"");
    assert_eq!(len(matrix_doc, b""), 3);
    let middle_row = cursor_index(&root, 1);
    let first = cursor_index(&middle_row, 0);
    let mut v: i64 = 0;
    let empty = b"";
    let rc = unsafe { qjson_cursor_get_i64(&first, empty.as_ptr() as *const i8, 0, &mut v) };
    assert_eq!(rc, qjson_err::QJSON_OK as c_int);
    assert_eq!(v, 1);
    unsafe { qjson_free(matrix_doc) };
}

#[test]
fn cjson_escaped_string_and_spaced_key_fixture_paths_are_accessible() {
    let data = parse_file(&repo_root().join("tests/vendor/cJSON/tests/inputs/test11"));
    let doc = parse(&data);

    assert_eq!(get_str(doc, b"name"), "Jack (\"Bee\") Nimble");
    assert_eq!(get_str(doc, b"format.type"), "rect");
    assert_eq!(get_i64(doc, b"format.width"), 1920);
    assert_eq!(get_i64(doc, b"format.height"), 1080);
    assert!(!get_bool(doc, b"format.interlace"));
    assert_eq!(get_i64(doc, b"format.frame rate"), 24);

    unsafe { qjson_free(doc) };
}

#[test]
fn simdjson_example_config_fixture_paths_are_accessible() {
    let data =
        parse_file(&repo_root().join("tests/vendor/simdjson/jsonexamples/example_config.json"));
    let doc = parse(&data);

    assert_eq!(get_str(doc, b"app_name"), "MyApp");
    assert_eq!(get_str(doc, b"version"), "1.0.0");
    assert_eq!(get_i64(doc, b"port"), 8080);
    assert!(get_bool(doc, b"debug"));
    assert_eq!(len(doc, b"features"), 2);
    assert_eq!(get_str(doc, b"database.host"), "localhost");
    assert_eq!(get_i64(doc, b"database.port"), 5432);

    unsafe { qjson_free(doc) };
}

#[test]
fn simdjson_twitter_fixture_paths_are_accessible() {
    let data = parse_file(&repo_root().join("tests/vendor/simdjson/jsonexamples/twitter.json"));
    let doc = parse(&data);

    assert_eq!(get_i64(doc, b"search_metadata.count"), 100);
    assert_eq!(len(doc, b"statuses"), 100);

    unsafe { qjson_free(doc) };
}

#[test]
fn cjson_parser_literals_parse_with_qjson() {
    let valid_values = [
        "null",
        "true",
        "false",
        "1.5",
        "\"\"",
        "\"hello\"",
        "[]",
        "{}",
        "[1, null, true, false, [], \"hello\", {}]",
        "{\"one\":1, \"NULL\":null, \"TRUE\":true, \"FALSE\":false, \"array\":[], \"world\":\"hello\", \"object\":{}}",
    ];

    for json in valid_values {
        assert_parses_in_both_modes(json, json.as_bytes());
    }
}

#[test]
fn cjson_empty_and_null_literals_keep_shape() {
    let empty_array = parse(b"[]");
    assert_eq!(len(empty_array, b""), 0);
    unsafe { qjson_free(empty_array) };

    let empty_object = parse(b"{}");
    assert_eq!(len(empty_object, b""), 0);
    unsafe { qjson_free(empty_object) };

    let array_with_null = parse(b"[null]");
    assert!(is_null(array_with_null, b"[0]"));
    unsafe { qjson_free(array_with_null) };

    let object_with_null = parse(br#"{"null":null}"#);
    assert!(is_null(object_with_null, b"null"));
    unsafe { qjson_free(object_with_null) };
}

#[test]
fn cjson_malformed_container_literals_are_rejected() {
    let invalid = [
        b"".as_slice(),
        b"[".as_slice(),
        b"]".as_slice(),
        b"{".as_slice(),
        b"}".as_slice(),
        b"[1,]".as_slice(),
        br#"{"one"}"#.as_slice(),
        br#"{"one":1,}"#.as_slice(),
    ];

    for json in invalid {
        assert!(
            Document::parse(json).is_err(),
            "malformed cJSON parser literal was accepted: {:?}",
            String::from_utf8_lossy(json)
        );
    }
}

#[test]
fn cjson_fixture_invalid_paths_and_type_mismatches_fail() {
    let data = parse_file(&repo_root().join("tests/vendor/cJSON/tests/inputs/test11"));
    let doc = parse(&data);

    let mut wrong_type_p: *const u8 = ptr::null();
    let mut wrong_type_n: usize = 0;
    let rc = unsafe {
        qjson_get_str(
            doc,
            b"format.width".as_ptr() as *const i8,
            b"format.width".len(),
            &mut wrong_type_p,
            &mut wrong_type_n,
        )
    };
    assert_eq!(rc, qjson_err::QJSON_TYPE_MISMATCH as c_int);

    let mut p: *const u8 = ptr::null();
    let mut n: usize = 0;
    let rc = unsafe {
        qjson_get_str(
            doc,
            b"format.missing".as_ptr() as *const i8,
            b"format.missing".len(),
            &mut p,
            &mut n,
        )
    };
    assert_eq!(rc, qjson_err::QJSON_NOT_FOUND as c_int);

    let mut nested = std::mem::MaybeUninit::<qjson_cursor>::uninit();
    let format_type = open(doc, b"format.type");
    let rc = unsafe {
        qjson_cursor_field(
            &format_type,
            b"child".as_ptr() as *const i8,
            b"child".len(),
            nested.as_mut_ptr(),
        )
    };
    assert_eq!(rc, qjson_err::QJSON_TYPE_MISMATCH as c_int);

    unsafe { qjson_free(doc) };
}

#[test]
fn cjson_number_literals_parse_with_qjson() {
    let numbers = [
        "0",
        "0.0",
        "-0",
        "-1",
        "-32768",
        "-2147483648",
        "1",
        "32767",
        "2147483647",
        "0.001",
        "10e-10",
        "10E-10",
        "10e10",
        "123e+127",
        "123e-128",
        "-0.001",
        "-10e-10",
        "-10E-10",
        "-10e20",
        "-123e+127",
        "-123e-128",
        "9999999999999999999999999999999999999999999999912345678901234567",
        "9999999999999999999999999999999999999999999999912345678901234567E10",
        "999999999999999999999999999999999999999999999991234567890.1234567",
    ];

    for number in numbers {
        let json = format!("[{number}]");
        assert_parses_in_both_modes(&json, json.as_bytes());
    }
}

#[test]
fn cjson_string_literals_decode_with_qjson() {
    let cases: &[(&[u8], &[u8])] = &[
        (br#""""#, b""),
        (
            br##"" !\"#$%&'()*+,-./\/0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_'abcdefghijklmnopqrstuvwxyz{|}~""##,
            b" !\"#$%&'()*+,-.//0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_'abcdefghijklmnopqrstuvwxyz{|}~",
        ),
        (
            br#""\"\\\/\b\f\n\r\t\u20AC\u732b""#,
            b"\"\\/\x08\x0c\n\r\t\xe2\x82\xac\xe7\x8c\xab",
        ),
        (br#""\uD83D\udc31""#, b"\xf0\x9f\x90\xb1"),
        (
            br#""~!@\\#$%^&*()\\\\-\\+{}[]:\\;\\\"\\<\\>?/.,DC=ad,DC=com""#,
            b"~!@\\#$%^&*()\\\\-\\+{}[]:\\;\\\"\\<\\>?/.,DC=ad,DC=com",
        ),
    ];

    for (json_string, expected) in cases {
        let mut json = Vec::from(br#"{"s":"#.as_slice());
        json.extend_from_slice(json_string);
        json.extend_from_slice(b"}");
        let doc = parse(&json);
        assert_eq!(get_str(doc, b"s").as_bytes(), *expected);
        unsafe { qjson_free(doc) };
    }
}

#[test]
fn simdjson_big_integer_literals_parse_but_do_not_fit_i64() {
    let cases = [
        br#"{"val":123456789012345678901}"#.as_slice(),
        br#"{"val":-12345678901234567890}"#.as_slice(),
        br#"[1, 123456789012345678901, 3]"#.as_slice(),
    ];

    for data in cases {
        Document::parse(data).unwrap();
    }

    for data in &cases[..2] {
        let doc = parse(data);
        let mut v: i64 = 0;
        let rc = unsafe { qjson_get_i64(doc, b"val".as_ptr() as *const i8, b"val".len(), &mut v) };
        assert_eq!(rc, qjson_err::QJSON_OUT_OF_RANGE as c_int);
        unsafe { qjson_free(doc) };
    }

    let doc = parse(cases[2]);
    let root = open(doc, b"");
    let big_integer = cursor_index(&root, 1);
    let empty = b"";
    let mut v: i64 = 0;
    let rc = unsafe { qjson_cursor_get_i64(&big_integer, empty.as_ptr() as *const i8, 0, &mut v) };
    assert_eq!(rc, qjson_err::QJSON_OUT_OF_RANGE as c_int);
    unsafe { qjson_free(doc) };
}
