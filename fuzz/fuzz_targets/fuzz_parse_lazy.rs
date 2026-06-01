#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
use libfuzzer_sys::fuzz_target;
use qjson::ffi::*;
use qjson::options::{Options, QJSON_MODE_LAZY};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::os::raw::{c_char, c_int};
use std::ptr;

const FUZZ_MAX_DEPTH: u32 = 128;
const DEPTH_SKIP_LIMIT: u32 = 64;
#[allow(dead_code)]
const MAX_PATH_CHECKS: usize = 256;

#[allow(dead_code)]
const T_NULL: c_int = 0;
#[allow(dead_code)]
const T_BOOL: c_int = 1;
#[allow(dead_code)]
const T_NUM: c_int = 2;
#[allow(dead_code)]
const T_STR: c_int = 3;
#[allow(dead_code)]
const T_ARR: c_int = 4;
#[allow(dead_code)]
const T_OBJ: c_int = 5;

#[cfg(not(test))]
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
    let mut err = qjson_error::default();
    let doc = unsafe { qjson_parse_ex(data.as_ptr(), data.len(), &opts, &mut err) };
    assert!(!doc.is_null(), "qjson lazy rejected serde-accepted input with err={err:?}: {data:?}");

    let (actual, path_checks) = unsafe {
        let mut root: qjson_cursor = std::mem::zeroed();
        let rc = qjson_open(doc, ptr::null(), 0, &mut root);
        assert_eq!(rc, 0, "qjson_open root failed with rc={rc}: {data:?}");

        let mut path = String::new();
        let mut path_checks = Vec::new();
        let actual = cursor_to_value(&root, 0, &mut path, true, &mut path_checks);
        assert!(
            !path_checks.is_empty(),
            "semantic replay must record at least the root path for input={data:?}",
        );
        qjson_free(doc);
        (actual, path_checks)
    };

    assert!(!path_checks.is_empty(), "path checks should include at least the root value");
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

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
struct PathCheck {
    path: Vec<u8>,
    expected: PathExpected,
}

#[derive(Clone, Debug, PartialEq)]
enum PathExpected {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    ArrayLen(usize),
    ObjectLen(usize),
}

impl PathExpected {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(*value),
            Value::Number(number) => {
                Self::Number(number.as_f64().expect("normalized qjson number must fit f64"))
            }
            Value::String(value) => Self::String(value.clone()),
            Value::Array(items) => Self::ArrayLen(items.len()),
            Value::Object(map) => Self::ObjectLen(map.len()),
        }
    }

    #[allow(dead_code)]
    fn type_tag(&self) -> c_int {
        match self {
            Self::Null => T_NULL,
            Self::Bool(_) => T_BOOL,
            Self::Number(_) => T_NUM,
            Self::String(_) => T_STR,
            Self::ArrayLen(_) => T_ARR,
            Self::ObjectLen(_) => T_OBJ,
        }
    }
}

fn path_key_can_be_in_qjson_path(key: &str) -> bool {
    !key.as_bytes().iter().any(|&byte| matches!(byte, b'.' | b'[' | b']'))
}

fn append_key_segment(path: &mut String, key: &str) -> usize {
    let old_len = path.len();
    if old_len != 0 {
        path.push('.');
    }
    path.push_str(key);
    old_len
}

fn append_index_segment(path: &mut String, index: usize) -> usize {
    let old_len = path.len();
    write!(path, "[{index}]").expect("writing to String cannot fail");
    old_len
}

#[allow(dead_code)]
fn record_path_check(path_checks: &mut Vec<PathCheck>, path: &str, value: &Value) {
    if path_checks.len() >= MAX_PATH_CHECKS {
        return;
    }
    path_checks.push(PathCheck {
        path: path.as_bytes().to_vec(),
        expected: PathExpected::from_value(value),
    });
}

unsafe fn cursor_to_value(
    cur: &qjson_cursor,
    depth: u32,
    path: &mut String,
    path_safe: bool,
    path_checks: &mut Vec<PathCheck>,
) -> Value {
    assert!(depth <= FUZZ_MAX_DEPTH, "walker exceeded depth limit");
    let value = match cursor_type(cur) {
        T_NULL => Value::Null,
        T_BOOL => Value::Bool(cursor_bool(cur)),
        T_NUM => Value::Number(cursor_number(cur)),
        T_STR => Value::String(cursor_string(cur)),
        T_ARR => Value::Array(cursor_array(cur, depth + 1, path, path_safe, path_checks)),
        T_OBJ => Value::Object(cursor_object(cur, depth + 1, path, path_safe, path_checks)),
        other => panic!("unknown qjson type {other}"),
    };

    if path_safe {
        record_path_check(path_checks, path, &value);
    }

    value
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

unsafe fn cursor_array(
    cur: &qjson_cursor,
    depth: u32,
    path: &mut String,
    path_safe: bool,
    path_checks: &mut Vec<PathCheck>,
) -> Vec<Value> {
    let len = cursor_len(cur);
    let mut values = Vec::with_capacity(len);
    for i in 0..len {
        values.push(cursor_index_value(cur, i, depth, path, path_safe, path_checks));
    }

    for i in varied_order(len) {
        let mut scratch_path = String::new();
        let _ = cursor_index_value(cur, i, depth, &mut scratch_path, false, path_checks);
    }

    values
}

unsafe fn cursor_index_value(
    cur: &qjson_cursor,
    index: usize,
    depth: u32,
    path: &mut String,
    path_safe: bool,
    path_checks: &mut Vec<PathCheck>,
) -> Value {
    let mut child: qjson_cursor = std::mem::zeroed();
    let rc = qjson_cursor_index(cur, index, &mut child);
    assert_eq!(rc, 0, "qjson_cursor_index({index}) failed with rc={rc}");

    let old_len = if path_safe {
        append_index_segment(path, index)
    } else {
        path.len()
    };
    let value = cursor_to_value(&child, depth, path, path_safe, path_checks);
    path.truncate(old_len);
    value
}

unsafe fn cursor_object(
    cur: &qjson_cursor,
    depth: u32,
    path: &mut String,
    path_safe: bool,
    path_checks: &mut Vec<PathCheck>,
) -> Map<String, Value> {
    let len = cursor_len(cur);
    let mut map = Map::new();
    let mut raw_entries = Vec::with_capacity(len);
    let mut entries = Vec::with_capacity(len);
    let mut counts: HashMap<String, usize> = HashMap::with_capacity(len);

    for i in 0..len {
        let (key, value_cur) = cursor_object_entry_at(cur, i);
        *counts.entry(key.clone()).or_insert(0) += 1;
        raw_entries.push((key, value_cur));
    }

    for (key, value_cur) in raw_entries {
        let child_path_safe = path_safe
            && counts.get(&key).copied().unwrap_or(0) == 1
            && path_key_can_be_in_qjson_path(&key);
        let old_len = if child_path_safe {
            append_key_segment(path, &key)
        } else {
            path.len()
        };

        let value = cursor_to_value(&value_cur, depth, path, child_path_safe, path_checks);
        path.truncate(old_len);

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
        assert_eq!(rc, 0, "qjson_cursor_field({key:?}) failed with rc={rc}");

        let mut scratch_path = String::new();
        let actual = cursor_to_value(&child, depth, &mut scratch_path, false, path_checks);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_key_safety_rejects_qjson_path_delimiters() {
        assert!(path_key_can_be_in_qjson_path("plain"));
        assert!(path_key_can_be_in_qjson_path("emoji"));
        assert!(!path_key_can_be_in_qjson_path("a.b"));
        assert!(!path_key_can_be_in_qjson_path("arr[0]"));
        assert!(!path_key_can_be_in_qjson_path("bad]key"));
    }

    #[test]
    fn path_segment_builders_match_qjson_path_syntax() {
        let mut path = String::new();

        let root_len = path.len();
        append_key_segment(&mut path, "body");
        assert_eq!(path, "body");

        let body_len = path.len();
        append_key_segment(&mut path, "messages");
        assert_eq!(path, "body.messages");

        let messages_len = path.len();
        append_index_segment(&mut path, 12);
        assert_eq!(path, "body.messages[12]");

        path.truncate(messages_len);
        assert_eq!(path, "body.messages");
        path.truncate(body_len);
        assert_eq!(path, "body");
        path.truncate(root_len);
        assert_eq!(path, "");
    }

    #[test]
    fn expected_summary_records_getter_observable_values() {
        assert_eq!(PathExpected::from_value(&Value::Null), PathExpected::Null);
        assert_eq!(PathExpected::from_value(&Value::Bool(true)), PathExpected::Bool(true));
        assert_eq!(PathExpected::from_value(&serde_json::json!(1.5)), PathExpected::Number(1.5));
        assert_eq!(PathExpected::from_value(&serde_json::json!("x")), PathExpected::String("x".to_string()));
        assert_eq!(PathExpected::from_value(&serde_json::json!([1, 2, 3])), PathExpected::ArrayLen(3));
        assert_eq!(PathExpected::from_value(&serde_json::json!({"a": 1, "b": 2})), PathExpected::ObjectLen(2));
    }

    #[test]
    fn deterministic_replay_cases_cover_path_safe_and_ambiguous_keys() {
        fuzz_one(br#"{"body":{"model":"gpt","temperature":0.5,"ok":true,"none":null,"items":[{"id":1},{"id":2}]}}"#);
        fuzz_one(br#"{"dup":1,"dup":2,"a.b":3,"arr[0]":4,"nested":{"x":true}}"#);
        fuzz_one(br#"[{"name":"first"},{"name":"second","values":[1,2,3]}]"#);
    }
}
