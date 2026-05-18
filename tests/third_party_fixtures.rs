use std::os::raw::c_int;
use std::ptr;

use qjson::doc::Document;
use qjson::error::qjson_err;
use qjson::ffi::*;
use qjson::options::{Options, QJSON_MODE_EAGER, QJSON_MODE_LAZY};

const CJSON_FIXTURES: &[(&str, &[u8])] = &[
    (
        "cjson/test1.json",
        include_bytes!("fixtures/third_party/cjson/test1.json"),
    ),
    (
        "cjson/test2.json",
        include_bytes!("fixtures/third_party/cjson/test2.json"),
    ),
    (
        "cjson/test9.json",
        include_bytes!("fixtures/third_party/cjson/test9.json"),
    ),
    (
        "cjson/test10.json",
        include_bytes!("fixtures/third_party/cjson/test10.json"),
    ),
    (
        "cjson/test11.json",
        include_bytes!("fixtures/third_party/cjson/test11.json"),
    ),
];

const SIMDJSON_EXAMPLE_CONFIG: &[u8] =
    include_bytes!("fixtures/third_party/simdjson/example_config.json");

fn parse(s: &[u8]) -> *mut qjson_doc {
    let mut err: c_int = -1;
    let d = unsafe { qjson_parse(s.as_ptr(), s.len(), &mut err) };
    assert_eq!(err, qjson_err::QJSON_OK as c_int);
    assert!(!d.is_null());
    d
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
fn cjson_fixtures_parse_in_both_modes() {
    let eager = Options {
        mode: QJSON_MODE_EAGER,
        max_depth: 0,
    };
    let lazy = Options {
        mode: QJSON_MODE_LAZY,
        max_depth: 0,
    };

    for (name, data) in CJSON_FIXTURES {
        Document::parse_with_options(data, &eager)
            .unwrap_or_else(|e| panic!("{name} rejected in eager mode: {e:?}"));
        Document::parse_with_options(data, &lazy)
            .unwrap_or_else(|e| panic!("{name} rejected in lazy mode: {e:?}"));
    }
}

#[test]
fn cjson_nested_object_fixture_paths_are_accessible() {
    let doc = parse(include_bytes!("fixtures/third_party/cjson/test1.json"));

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
    let menu_doc = parse(include_bytes!("fixtures/third_party/cjson/test2.json"));
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

    let matrix_doc = parse(include_bytes!("fixtures/third_party/cjson/test9.json"));
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
    let doc = parse(include_bytes!("fixtures/third_party/cjson/test11.json"));

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
    let doc = parse(SIMDJSON_EXAMPLE_CONFIG);

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
fn simdjson_big_integer_literals_parse_but_do_not_fit_i64() {
    let cases = [
        br#"{"val":123456789012345678901}"#.as_slice(),
        br#"{"val":-12345678901234567890}"#.as_slice(),
        br#"[1, 123456789012345678901, 3]"#.as_slice(),
    ];

    for data in cases {
        Document::parse(data).unwrap();
    }

    let doc = parse(cases[0]);
    let mut v: i64 = 0;
    let rc = unsafe { qjson_get_i64(doc, b"val".as_ptr() as *const i8, b"val".len(), &mut v) };
    assert_eq!(rc, qjson_err::QJSON_OUT_OF_RANGE as c_int);
    unsafe { qjson_free(doc) };
}
