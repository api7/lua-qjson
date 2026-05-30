#![no_main]

use libfuzzer_sys::fuzz_target;
use qjson::doc::Document;
use qjson::options::{Options, QJSON_MODE_EAGER};
use serde_json::Value;

const FUZZ_MAX_DEPTH: u32 = 128;
const DEPTH_SKIP_LIMIT: u32 = 64;

fuzz_target!(|data: &[u8]| {
    if exceeds_container_depth(data, DEPTH_SKIP_LIMIT) {
        return;
    }

    let opts = Options { mode: QJSON_MODE_EAGER, max_depth: FUZZ_MAX_DEPTH };
    let qjson_ok = Document::parse_with_options(data, &opts).is_ok();
    let serde_result = serde_json::from_slice::<Value>(data);
    let serde_ok = serde_result.is_ok();

    if qjson_ok != serde_ok && !(qjson_ok && serde_rejected_allowed_value(data, &serde_result)) {
        panic!(
            "qjson eager / serde_json accept-reject mismatch: qjson_ok={qjson_ok} serde_ok={serde_ok} input={:?}",
            data,
        );
    }
});

fn exceeds_container_depth(data: &[u8], limit: u32) -> bool {
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;

    for &byte in data {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = depth.saturating_add(1);
                if depth > limit {
                    return true;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    false
}

fn serde_rejected_allowed_value(data: &[u8], result: &Result<Value, serde_json::Error>) -> bool {
    match result {
        Ok(_) => false,
        Err(err) => {
            let msg = err.to_string();
            // qjson eager validates JSON number grammar, while
            // serde_json::Value also requires representability in its number
            // model. qjson also accepts escaped lone UTF-16 surrogates at
            // parse time; lazy string decode rejects them when accessed. Serde
            // reports some leading-surrogate cases as "unexpected end of hex
            // escape", so gate that message on an input-level surrogate check.
            msg.contains("number out of range")
                || ((msg.contains("surrogate") || msg.contains("unexpected end of hex escape"))
                    && contains_escaped_unpaired_surrogate(data))
        }
    }
}

fn contains_escaped_unpaired_surrogate(data: &[u8]) -> bool {
    let mut i = 0usize;
    let mut in_string = false;

    while i < data.len() {
        if !in_string {
            if data[i] == b'"' {
                in_string = true;
            }
            i += 1;
            continue;
        }

        match data[i] {
            b'"' => {
                in_string = false;
                i += 1;
            }
            b'\\' => {
                if i + 1 >= data.len() {
                    return false;
                }
                if data[i + 1] != b'u' {
                    i += 2;
                    continue;
                }

                let Some(first) = decode_hex_escape(data, i + 2) else {
                    return false;
                };

                if is_low_surrogate(first) {
                    return true;
                }

                if is_high_surrogate(first) {
                    let next = i + 6;
                    if next + 1 < data.len() && data[next] == b'\\' && data[next + 1] == b'u' {
                        let Some(second) = decode_hex_escape(data, next + 2) else {
                            return false;
                        };
                        if is_low_surrogate(second) {
                            i = next + 6;
                            continue;
                        }
                    }
                    return true;
                }

                i += 6;
            }
            _ => i += 1,
        }
    }

    false
}

fn decode_hex_escape(data: &[u8], pos: usize) -> Option<u16> {
    if pos + 4 > data.len() {
        return None;
    }

    let mut value = 0u16;
    for &byte in &data[pos..pos + 4] {
        value = (value << 4) | u16::from(hex_value(byte)?);
    }
    Some(value)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn is_high_surrogate(value: u16) -> bool {
    (0xD800..=0xDBFF).contains(&value)
}

fn is_low_surrogate(value: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&value)
}
