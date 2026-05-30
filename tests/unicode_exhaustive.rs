use std::os::raw::c_int;
use std::ptr;

use qjson::error::qjson_err;
use qjson::ffi::{qjson_doc, qjson_error, qjson_free, qjson_get_str, qjson_parse_ex};
use qjson::options::{Options, QJSON_MODE_EAGER, QJSON_MODE_LAZY};

const OK: c_int = qjson_err::QJSON_OK as c_int;
const PARSE_ERROR: c_int = qjson_err::QJSON_PARSE_ERROR as c_int;
const INVALID_STRING: c_int = qjson_err::QJSON_INVALID_STRING as c_int;
const INVALID_UTF8: c_int = qjson_err::QJSON_INVALID_UTF8 as c_int;

fn parse_with_mode(json: &[u8], mode: u32) -> Result<*mut qjson_doc, c_int> {
    let opts = Options { mode, max_depth: 0 };
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(json.as_ptr(), json.len(), &opts, &mut err) };
    if doc.is_null() {
        Err(err.code)
    } else {
        assert_eq!(err.code, OK);
        Ok(doc)
    }
}

fn decode_root_string(doc: *mut qjson_doc) -> Result<Vec<u8>, c_int> {
    let mut ptr: *const u8 = ptr::null();
    let mut len: usize = 0;
    let rc = unsafe { qjson_get_str(doc, ptr::null(), 0, &mut ptr, &mut len) };
    if rc != OK {
        return Err(rc);
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec())
}

fn root_string_json(payload: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.push(b'"');
    out.extend_from_slice(payload);
    out.push(b'"');
}

fn push_u_escape(unit: u16, out: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.extend_from_slice(b"\\u");
    out.push(HEX[((unit >> 12) & 0xF) as usize]);
    out.push(HEX[((unit >> 8) & 0xF) as usize]);
    out.push(HEX[((unit >> 4) & 0xF) as usize]);
    out.push(HEX[(unit & 0xF) as usize]);
}

fn push_json_escape_for_scalar(cp: u32, out: &mut Vec<u8>) {
    if cp <= 0xFFFF {
        push_u_escape(cp as u16, out);
        return;
    }

    let v = cp - 0x1_0000;
    let high = 0xD800 | ((v >> 10) as u16);
    let low = 0xDC00 | ((v as u16) & 0x03FF);
    push_u_escape(high, out);
    push_u_escape(low, out);
}

fn push_utf8_for_scalar(cp: u32, out: &mut Vec<u8>) {
    let ch = char::from_u32(cp).expect("test skips surrogate range");
    let mut buf = [0u8; 4];
    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
}

#[test]
fn unicode_escape_exhaustively_decodes_every_scalar_value() {
    let mut escaped = Vec::with_capacity(12);
    let mut json = Vec::with_capacity(14);
    let mut expected = Vec::with_capacity(4);

    for cp in 0..=0x10_FFFF {
        if (0xD800..=0xDFFF).contains(&cp) {
            continue;
        }

        escaped.clear();
        push_json_escape_for_scalar(cp, &mut escaped);
        root_string_json(&escaped, &mut json);

        expected.clear();
        push_utf8_for_scalar(cp, &mut expected);

        let doc = parse_with_mode(&json, QJSON_MODE_EAGER)
            .unwrap_or_else(|err| panic!("eager parse rejected U+{cp:04X}: err={err}"));
        let got = decode_root_string(doc)
            .unwrap_or_else(|err| panic!("decode rejected U+{cp:04X}: err={err}"));
        unsafe { qjson_free(doc) };

        assert_eq!(got, expected, "decoded escape for U+{cp:04X}");
    }
}

#[test]
fn illegal_surrogate_escapes_reject_eager_and_defer_in_lazy() {
    let cases: &[(&str, &str)] = &[
        ("lone high surrogate", "\\uD800"),
        ("last lone high surrogate", "\\uDBFF"),
        ("lone low surrogate", "\\uDC00"),
        ("last lone low surrogate", "\\uDFFF"),
        ("high followed by scalar escape", "\\uD800\\u0041"),
        ("high followed by another high", "\\uD800\\uD800"),
        ("high followed by non-surrogate BMP", "\\uDBFF\\uE000"),
        ("high followed by raw byte", "\\uD800x"),
    ];

    let mut json = Vec::new();
    for &(name, escaped) in cases {
        root_string_json(escaped.as_bytes(), &mut json);

        let eager_err = parse_with_mode(&json, QJSON_MODE_EAGER)
            .map(|doc| unsafe {
                qjson_free(doc);
                OK
            })
            .unwrap_err();
        assert_eq!(eager_err, INVALID_STRING, "{name} should fail during eager parse");

        let lazy_doc = parse_with_mode(&json, QJSON_MODE_LAZY)
            .unwrap_or_else(|err| panic!("{name} should parse in lazy mode, err={err}"));
        let lazy_err = decode_root_string(lazy_doc)
            .map(|_| OK)
            .unwrap_err();
        unsafe { qjson_free(lazy_doc) };
        assert_ne!(lazy_err, OK, "{name} should fail during lazy access");
    }
}

#[test]
fn raw_string_byte_exhaustively_covers_validation_outcomes() {
    let mut json = Vec::with_capacity(5);
    let mut expected = Vec::with_capacity(3);

    for byte in 0u8..=255 {
        json.clear();
        json.extend_from_slice(b"\"x");
        json.push(byte);
        json.extend_from_slice(b"y\"");

        expected.clear();
        expected.push(b'x');
        expected.push(byte);
        expected.push(b'y');

        match byte {
            b'"' => {
                assert_eq!(
                    parse_with_mode(&json, QJSON_MODE_EAGER).unwrap_err(),
                    PARSE_ERROR,
                    "raw byte 0x{byte:02X} must break string structure in eager mode",
                );
                assert_eq!(
                    parse_with_mode(&json, QJSON_MODE_LAZY).unwrap_err(),
                    PARSE_ERROR,
                    "raw byte 0x{byte:02X} must break string structure in lazy mode",
                );
            }
            b'\\' | 0x00..=0x1F | 0x80..=0xFF => {
                let want = if byte >= 0x80 { INVALID_UTF8 } else { INVALID_STRING };
                assert_eq!(
                    parse_with_mode(&json, QJSON_MODE_EAGER).unwrap_err(),
                    want,
                    "raw byte 0x{byte:02X} should fail eager validation",
                );

                let lazy_doc = parse_with_mode(&json, QJSON_MODE_LAZY)
                    .unwrap_or_else(|err| panic!("raw byte 0x{byte:02X} lazy parse err={err}"));
                let lazy_err = decode_root_string(lazy_doc)
                    .map(|_| OK)
                    .unwrap_err();
                unsafe { qjson_free(lazy_doc) };
                assert_eq!(
                    lazy_err, want,
                    "raw byte 0x{byte:02X} should fail on lazy access",
                );
            }
            _ => {
                let eager_doc = parse_with_mode(&json, QJSON_MODE_EAGER)
                    .unwrap_or_else(|err| panic!("raw byte 0x{byte:02X} eager err={err}"));
                let eager_got = decode_root_string(eager_doc)
                    .unwrap_or_else(|err| panic!("raw byte 0x{byte:02X} eager decode err={err}"));
                unsafe { qjson_free(eager_doc) };
                assert_eq!(eager_got, expected, "raw byte 0x{byte:02X} eager decode");

                let lazy_doc = parse_with_mode(&json, QJSON_MODE_LAZY)
                    .unwrap_or_else(|err| panic!("raw byte 0x{byte:02X} lazy err={err}"));
                let lazy_got = decode_root_string(lazy_doc)
                    .unwrap_or_else(|err| panic!("raw byte 0x{byte:02X} lazy decode err={err}"));
                unsafe { qjson_free(lazy_doc) };
                assert_eq!(lazy_got, expected, "raw byte 0x{byte:02X} lazy decode");
            }
        }
    }
}
