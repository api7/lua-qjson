#![no_main]

use libfuzzer_sys::fuzz_target;
use qjson::ffi::*;
use qjson::options::{Options, QJSON_MODE_LAZY};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::os::raw::{c_char, c_int};
use std::ptr;

const FUZZ_MAX_DEPTH: u32 = 128;
const DEPTH_SKIP_LIMIT: u32 = 64;

fuzz_target!(|data: &[u8]| {
    fuzz_one(data);
});

fn fuzz_one(data: &[u8]) {
    if exceeds_container_depth(data, DEPTH_SKIP_LIMIT) {
        return;
    }

    let expected = match serde_json::from_slice::<Value>(data) {
        Ok(value) => normalize_numbers(value),
        Err(_) => return,
    };

    let opts = Options { mode: QJSON_MODE_LAZY, max_depth: FUZZ_MAX_DEPTH };
    let mut err: c_int = -1;
    let doc = unsafe { qjson_parse_ex(data.as_ptr(), data.len(), &opts, &mut err) };
    assert!(!doc.is_null(), "qjson lazy rejected serde-accepted input with err={err}: {data:?}");

    let actual = unsafe {
        let mut root: qjson_cursor = std::mem::zeroed();
        let rc = qjson_open(doc, ptr::null(), 0, &mut root);
        assert_eq!(rc, 0, "qjson_open root failed with rc={rc}: {data:?}");

        let actual = cursor_to_value(&root, 0);
        qjson_free(doc);
        actual
    };

    assert_eq!(actual, expected, "lazy value mismatch for input={data:?}");
}

fn normalize_numbers(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(normalize_numbers).collect()),
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, normalize_numbers(v))).collect())
        }
        Value::Number(number) => {
            let value = number.as_f64().expect("serde accepted number must convert to f64");
            Value::Number(Number::from_f64(value).expect("serde accepted number must be finite"))
        }
        other => other,
    }
}

unsafe fn cursor_to_value(cur: &qjson_cursor, depth: u32) -> Value {
    assert!(depth <= FUZZ_MAX_DEPTH, "walker exceeded depth limit");
    match cursor_type(cur) {
        0 => Value::Null,
        1 => Value::Bool(cursor_bool(cur)),
        2 => Value::Number(cursor_number(cur)),
        3 => Value::String(cursor_string(cur)),
        4 => Value::Array(cursor_array(cur, depth + 1)),
        5 => Value::Object(cursor_object(cur, depth + 1)),
        other => panic!("unknown qjson type {other}"),
    }
}

unsafe fn cursor_type(cur: &qjson_cursor) -> c_int {
    let mut ty: c_int = -1;
    let rc = qjson_cursor_typeof(cur, ptr::null(), 0, &mut ty);
    assert_eq!(rc, 0, "qjson_cursor_typeof failed with rc={rc}");
    ty
}

unsafe fn cursor_bool(cur: &qjson_cursor) -> bool {
    let mut value: c_int = -1;
    let rc = qjson_cursor_get_bool(cur, ptr::null(), 0, &mut value);
    assert_eq!(rc, 0, "qjson_cursor_get_bool failed with rc={rc}");
    value != 0
}

unsafe fn cursor_number(cur: &qjson_cursor) -> Number {
    let mut value = 0.0f64;
    let rc = qjson_cursor_get_f64(cur, ptr::null(), 0, &mut value);
    assert_eq!(rc, 0, "qjson_cursor_get_f64 failed with rc={rc}");
    Number::from_f64(value).expect("qjson finite JSON number")
}

unsafe fn cursor_string(cur: &qjson_cursor) -> String {
    let mut ptr_out: *const u8 = ptr::null();
    let mut len_out: usize = 0;
    let rc = qjson_cursor_get_str(cur, ptr::null(), 0, &mut ptr_out, &mut len_out);
    assert_eq!(rc, 0, "qjson_cursor_get_str failed with rc={rc}");
    let bytes = std::slice::from_raw_parts(ptr_out, len_out).to_vec();
    String::from_utf8(bytes).expect("serde accepted string must decode as UTF-8")
}

unsafe fn cursor_len(cur: &qjson_cursor) -> usize {
    let mut len = 0usize;
    let rc = qjson_cursor_len(cur, ptr::null(), 0, &mut len);
    assert_eq!(rc, 0, "qjson_cursor_len failed with rc={rc}");
    len
}

unsafe fn cursor_array(cur: &qjson_cursor, depth: u32) -> Vec<Value> {
    let len = cursor_len(cur);
    let mut values = Vec::with_capacity(len);
    for i in 0..len {
        values.push(cursor_index_value(cur, i, depth));
    }

    for i in varied_order(len) {
        let _ = cursor_index_value(cur, i, depth);
    }

    values
}

unsafe fn cursor_index_value(cur: &qjson_cursor, index: usize, depth: u32) -> Value {
    let mut child: qjson_cursor = std::mem::zeroed();
    let rc = qjson_cursor_index(cur, index, &mut child);
    assert_eq!(rc, 0, "qjson_cursor_index({index}) failed with rc={rc}");
    cursor_to_value(&child, depth)
}

unsafe fn cursor_object(cur: &qjson_cursor, depth: u32) -> Map<String, Value> {
    let len = cursor_len(cur);
    let mut map = Map::new();
    let mut entries = Vec::with_capacity(len);
    let mut counts: HashMap<String, usize> = HashMap::with_capacity(len);

    for i in 0..len {
        let (key, value_cur) = cursor_object_entry_at(cur, i);
        let value = cursor_to_value(&value_cur, depth);
        *counts.entry(key.clone()).or_insert(0) += 1;
        map.insert(key.clone(), value.clone());
        entries.push((key, value));
    }

    for i in varied_order(entries.len()) {
        let (key, expected) = &entries[i];
        if counts.get(key).copied().unwrap_or(0) != 1 {
            continue;
        }

        let mut child: qjson_cursor = std::mem::zeroed();
        let rc = qjson_cursor_field(cur, key.as_ptr() as *const c_char, key.len(), &mut child);
        if rc != 0 {
            continue;
        }

        let actual = cursor_to_value(&child, depth);
        assert_eq!(actual, *expected, "qjson_cursor_field warm lookup mismatch for key {key:?}");
    }

    map
}

unsafe fn cursor_object_entry_at(cur: &qjson_cursor, index: usize) -> (String, qjson_cursor) {
    let mut key_ptr: *const u8 = ptr::null();
    let mut key_len = 0usize;
    let mut value_cur: qjson_cursor = std::mem::zeroed();
    let rc = qjson_cursor_object_entry_at(cur, index, &mut key_ptr, &mut key_len, &mut value_cur);
    assert_eq!(rc, 0, "qjson_cursor_object_entry_at({index}) failed with rc={rc}");

    let key_bytes = std::slice::from_raw_parts(key_ptr, key_len).to_vec();
    let key = String::from_utf8(key_bytes).expect("serde accepted object key must decode as UTF-8");
    (key, value_cur)
}

fn varied_order(len: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }

    let mut order = Vec::with_capacity(len + 3);
    order.push(len - 1);
    order.push(0);
    order.push(len / 2);
    for i in (0..len).rev() {
        order.push(i);
    }
    order
}

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
