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

    let actual = unsafe {
        let mut root: qjson_cursor = std::mem::zeroed();
        let rc = qjson_open(doc, ptr::null(), 0, &mut root);
        assert_eq!(rc, 0, "qjson_open root failed with rc={rc}: {data:?}");

        let mut path = String::new();
        let mut path_checks = Vec::new();
        let actual = cursor_to_value(&root, data, 0, &mut path, true, &mut path_checks);
        assert!(
            !path_checks.is_empty(),
            "semantic replay must record at least the root path for input={data:?}",
        );
        verify_path_getter_consistency(doc, &root, &path_checks);
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
    !key.is_empty() && !key.as_bytes().iter().any(|&byte| matches!(byte, b'.' | b'[' | b']'))
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
fn record_path_check(path_checks: &mut Vec<PathCheck>, path: &str, expected: PathExpected) {
    if path_checks.len() >= MAX_PATH_CHECKS {
        return;
    }
    path_checks.push(PathCheck { path: path.as_bytes().to_vec(), expected });
}

unsafe fn cursor_to_value(
    cur: &qjson_cursor,
    data: &[u8],
    depth: u32,
    path: &mut String,
    path_safe: bool,
    path_checks: &mut Vec<PathCheck>,
) -> Value {
    assert!(depth <= FUZZ_MAX_DEPTH, "walker exceeded depth limit");
    let ty = cursor_type(cur);
    let value = match ty {
        T_NULL => Value::Null,
        T_BOOL => Value::Bool(cursor_bool(cur)),
        T_NUM => Value::Number(cursor_number(cur)),
        T_STR => Value::String(cursor_string(cur)),
        T_ARR => Value::Array(cursor_array(cur, data, depth + 1, path, path_safe, path_checks)),
        T_OBJ => Value::Object(cursor_object(cur, data, depth + 1, path, path_safe, path_checks)),
        other => panic!("unknown qjson type {other}"),
    };

    if path_safe {
        let expected = match ty {
            T_ARR => PathExpected::ArrayLen(cursor_len(cur)),
            T_OBJ => PathExpected::ObjectLen(cursor_len(cur)),
            _ => PathExpected::from_value(&value),
        };
        record_path_check(path_checks, path, expected);
    }

    value
}

unsafe fn cursor_type(cur: &qjson_cursor) -> c_int {
    cursor_type_at(cur, &[])
}

unsafe fn cursor_type_at(cur: &qjson_cursor, path: &[u8]) -> c_int {
    let mut ty: c_int = -1;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_cursor_typeof(cur, path_ptr, path_len, &mut ty);
    assert_eq!(rc, 0, "qjson_cursor_typeof({:?}) failed with rc={rc}", path_debug(path));
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
    data: &[u8],
    depth: u32,
    path: &mut String,
    path_safe: bool,
    path_checks: &mut Vec<PathCheck>,
) -> Vec<Value> {
    let len = cursor_len(cur);
    let mut values = Vec::with_capacity(len);
    for i in 0..len {
        values.push(cursor_index_value(cur, data, i, depth, path, path_safe, path_checks));
    }

    for i in varied_order(len) {
        let mut scratch_path = String::new();
        let _ = cursor_index_value(cur, data, i, depth, &mut scratch_path, false, path_checks);
    }

    values
}

unsafe fn cursor_index_value(
    cur: &qjson_cursor,
    data: &[u8],
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
    let value = cursor_to_value(&child, data, depth, path, path_safe, path_checks);
    path.truncate(old_len);
    value
}

unsafe fn cursor_object(
    cur: &qjson_cursor,
    data: &[u8],
    depth: u32,
    path: &mut String,
    path_safe: bool,
    path_checks: &mut Vec<PathCheck>,
) -> Map<String, Value> {
    let len = cursor_len(cur);
    let mut map = Map::new();
    let raw_keys = cursor_raw_object_keys(cur, data, len);
    let mut raw_entries = Vec::with_capacity(len);
    let mut entries = Vec::with_capacity(len);
    let mut counts: HashMap<String, usize> = HashMap::with_capacity(len);

    for (i, raw_key) in raw_keys.into_iter().enumerate() {
        let (key, value_cur) = cursor_object_entry_at(cur, i);
        *counts.entry(key.clone()).or_insert(0) += 1;
        raw_entries.push((key, raw_key, value_cur));
    }

    for (key, raw_key, value_cur) in raw_entries {
        let key_is_unique = counts.get(&key).copied().unwrap_or(0) == 1;
        let field_lookup_key = field_lookup_key_for_replay(&key, &raw_key, key_is_unique);
        let child_path_safe =
            path_safe && field_lookup_key.is_some() && path_key_can_be_in_qjson_path(&key);
        let old_len = if child_path_safe {
            append_key_segment(path, &key)
        } else {
            path.len()
        };

        let value = cursor_to_value(&value_cur, data, depth, path, child_path_safe, path_checks);
        path.truncate(old_len);

        map.insert(key.clone(), value.clone());
        entries.push((key, value, field_lookup_key, value_cur));
    }

    for i in varied_order(entries.len()) {
        let (key, expected, field_lookup_key, expected_cur) = &entries[i];
        let Some(field_lookup_key) = field_lookup_key else { continue };

        let mut child: qjson_cursor = std::mem::zeroed();
        let rc = qjson_cursor_field(
            cur,
            field_lookup_key.as_ptr() as *const c_char,
            field_lookup_key.len(),
            &mut child,
        );
        assert_eq!(rc, 0, "qjson_cursor_field({key:?}) failed with rc={rc}");
        assert_eq!(
            child.idx_start, expected_cur.idx_start,
            "qjson_cursor_field returned wrong idx_start for key {key:?}",
        );
        assert_eq!(
            child.idx_end, expected_cur.idx_end,
            "qjson_cursor_field returned wrong idx_end for key {key:?}",
        );

        let mut scratch_path = String::new();
        let actual = cursor_to_value(&child, data, depth, &mut scratch_path, false, path_checks);
        assert_eq!(actual, *expected, "qjson_cursor_field warm lookup mismatch for key {key:?}");
    }

    map
}

fn field_lookup_key_for_replay(decoded_key: &str, raw_key: &[u8], unique_decoded_key: bool) -> Option<Vec<u8>> {
    if unique_decoded_key && decoded_key.as_bytes() == raw_key {
        Some(raw_key.to_vec())
    } else {
        None
    }
}

unsafe fn cursor_raw_object_keys(cur: &qjson_cursor, data: &[u8], expected_len: usize) -> Vec<Vec<u8>> {
    let mut byte_start = 0usize;
    let mut byte_end = 0usize;
    let rc = qjson_cursor_bytes(cur, &mut byte_start, &mut byte_end);
    assert_eq!(rc, 0, "qjson_cursor_bytes for object failed with rc={rc}");
    assert!(
        byte_start <= byte_end && byte_end <= data.len(),
        "qjson_cursor_bytes returned invalid object range {byte_start}..{byte_end} for input len {}",
        data.len(),
    );

    let keys = raw_object_keys(&data[byte_start..byte_end]);
    assert_eq!(
        keys.len(),
        expected_len,
        "raw object key scan disagreed with qjson_cursor_len for object range {byte_start}..{byte_end}",
    );
    keys
}

fn raw_object_keys(object: &[u8]) -> Vec<Vec<u8>> {
    let mut keys = Vec::new();
    let mut pos = skip_json_ws(object, 0);
    assert_eq!(object.get(pos), Some(&b'{'), "raw object scan must start at object opener");
    pos += 1;

    loop {
        pos = skip_json_ws(object, pos);
        match object.get(pos) {
            Some(b'}') => return keys,
            Some(b'"') => {}
            other => panic!("raw object scan expected key string, got {other:?}"),
        }

        let key_start = pos + 1;
        let key_end = scan_json_string_end(object, pos).expect("raw object key string must terminate");
        keys.push(object[key_start..key_end].to_vec());

        pos = skip_json_ws(object, key_end + 1);
        assert_eq!(object.get(pos), Some(&b':'), "raw object scan expected ':' after key");
        pos = skip_json_value(object, pos + 1).expect("raw object value must terminate");
        pos = skip_json_ws(object, pos);

        match object.get(pos) {
            Some(b',') => pos += 1,
            Some(b'}') => return keys,
            other => panic!("raw object scan expected ',' or '}}', got {other:?}"),
        }
    }
}

fn skip_json_ws(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t' | b'\n' | b'\r') {
        pos += 1;
    }
    pos
}

fn scan_json_string_end(bytes: &[u8], quote_pos: usize) -> Option<usize> {
    if bytes.get(quote_pos).copied() != Some(b'"') {
        return None;
    }

    let mut pos = quote_pos + 1;
    let mut escaped = false;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return Some(pos);
        }
        pos += 1;
    }
    None
}

fn skip_json_value(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut pos = skip_json_ws(bytes, pos);
    match bytes.get(pos).copied()? {
        b'"' => scan_json_string_end(bytes, pos).map(|end| end + 1),
        b'{' | b'[' => skip_json_container(bytes, pos),
        _ => {
            while pos < bytes.len() && !matches!(bytes[pos], b',' | b'}' | b']') {
                pos += 1;
            }
            Some(pos)
        }
    }
}

fn skip_json_container(bytes: &[u8], mut pos: usize) -> Option<usize> {
    let mut depth = 0usize;
    while pos < bytes.len() {
        match bytes[pos] {
            b'"' => pos = scan_json_string_end(bytes, pos)? + 1,
            b'{' | b'[' => {
                depth += 1;
                pos += 1;
            }
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                pos += 1;
                if depth == 0 {
                    return Some(pos);
                }
            }
            _ => pos += 1,
        }
    }
    None
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

unsafe fn verify_path_getter_consistency(
    doc: *mut qjson_doc,
    root: &qjson_cursor,
    path_checks: &[PathCheck],
) {
    for check in path_checks {
        let root_ty = root_type(doc, &check.path);
        let cursor_ty = cursor_type_at(root, &check.path);
        assert_eq!(
            root_ty,
            cursor_ty,
            "root/cursor typeof mismatch at path {:?}",
            path_debug(&check.path),
        );
        assert_eq!(
            root_ty,
            check.expected.type_tag(),
            "getter type mismatch against replay model at path {:?}",
            path_debug(&check.path),
        );

        match &check.expected {
            PathExpected::Null => {}
            PathExpected::Bool(expected) => {
                let root_value = root_bool(doc, &check.path);
                let cursor_value = cursor_bool_at(root, &check.path);
                assert_eq!(
                    root_value,
                    cursor_value,
                    "root/cursor bool mismatch at path {:?}",
                    path_debug(&check.path),
                );
                assert_eq!(
                    root_value,
                    *expected,
                    "bool mismatch against replay model at path {:?}",
                    path_debug(&check.path),
                );
            }
            PathExpected::Number(expected) => {
                let root_value = root_number(doc, &check.path);
                let cursor_value = cursor_number_at(root, &check.path);
                assert_eq!(
                    root_value.to_bits(),
                    cursor_value.to_bits(),
                    "root/cursor number mismatch at path {:?}",
                    path_debug(&check.path),
                );
                assert_eq!(
                    root_value.to_bits(),
                    expected.to_bits(),
                    "number mismatch against replay model at path {:?}",
                    path_debug(&check.path),
                );
            }
            PathExpected::String(expected) => {
                let root_value = root_string(doc, &check.path);
                let cursor_value = cursor_string_at(root, &check.path);
                assert_eq!(
                    root_value,
                    cursor_value,
                    "root/cursor string mismatch at path {:?}",
                    path_debug(&check.path),
                );
                assert_eq!(
                    root_value,
                    *expected,
                    "string mismatch against replay model at path {:?}",
                    path_debug(&check.path),
                );
            }
            PathExpected::ArrayLen(expected) | PathExpected::ObjectLen(expected) => {
                let root_value = root_len(doc, &check.path);
                let cursor_value = cursor_len_at(root, &check.path);
                assert_eq!(
                    root_value,
                    cursor_value,
                    "root/cursor len mismatch at path {:?}",
                    path_debug(&check.path),
                );
                assert_eq!(
                    root_value,
                    *expected,
                    "len mismatch against replay model at path {:?}",
                    path_debug(&check.path),
                );
            }
        }
    }
}

fn ffi_path(path: &[u8]) -> (*const c_char, usize) {
    if path.is_empty() {
        (ptr::null(), 0)
    } else {
        (path.as_ptr() as *const c_char, path.len())
    }
}

fn path_debug(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

unsafe fn root_type(doc: *mut qjson_doc, path: &[u8]) -> c_int {
    let mut ty: c_int = -1;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_typeof(doc, path_ptr, path_len, &mut ty);
    assert_eq!(rc, 0, "qjson_typeof({:?}) failed with rc={rc}", path_debug(path));
    ty
}

unsafe fn root_bool(doc: *mut qjson_doc, path: &[u8]) -> bool {
    let mut value: c_int = -1;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_get_bool(doc, path_ptr, path_len, &mut value);
    assert_eq!(rc, 0, "qjson_get_bool({:?}) failed with rc={rc}", path_debug(path));
    value != 0
}

unsafe fn cursor_bool_at(cur: &qjson_cursor, path: &[u8]) -> bool {
    let mut value: c_int = -1;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_cursor_get_bool(cur, path_ptr, path_len, &mut value);
    assert_eq!(rc, 0, "qjson_cursor_get_bool({:?}) failed with rc={rc}", path_debug(path));
    value != 0
}

unsafe fn root_number(doc: *mut qjson_doc, path: &[u8]) -> f64 {
    let mut value = 0.0f64;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_get_f64(doc, path_ptr, path_len, &mut value);
    assert_eq!(rc, 0, "qjson_get_f64({:?}) failed with rc={rc}", path_debug(path));
    value
}

unsafe fn cursor_number_at(cur: &qjson_cursor, path: &[u8]) -> f64 {
    let mut value = 0.0f64;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_cursor_get_f64(cur, path_ptr, path_len, &mut value);
    assert_eq!(rc, 0, "qjson_cursor_get_f64({:?}) failed with rc={rc}", path_debug(path));
    value
}

unsafe fn root_string(doc: *mut qjson_doc, path: &[u8]) -> String {
    let mut ptr_out: *const u8 = ptr::null();
    let mut len_out: usize = 0;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_get_str(doc, path_ptr, path_len, &mut ptr_out, &mut len_out);
    assert_eq!(rc, 0, "qjson_get_str({:?}) failed with rc={rc}", path_debug(path));
    let bytes = std::slice::from_raw_parts(ptr_out, len_out).to_vec();
    String::from_utf8(bytes).expect("serde accepted string must decode as UTF-8")
}

unsafe fn cursor_string_at(cur: &qjson_cursor, path: &[u8]) -> String {
    let mut ptr_out: *const u8 = ptr::null();
    let mut len_out: usize = 0;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_cursor_get_str(cur, path_ptr, path_len, &mut ptr_out, &mut len_out);
    assert_eq!(rc, 0, "qjson_cursor_get_str({:?}) failed with rc={rc}", path_debug(path));
    let bytes = std::slice::from_raw_parts(ptr_out, len_out).to_vec();
    String::from_utf8(bytes).expect("serde accepted string must decode as UTF-8")
}

unsafe fn root_len(doc: *mut qjson_doc, path: &[u8]) -> usize {
    let mut len = 0usize;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_len(doc, path_ptr, path_len, &mut len);
    assert_eq!(rc, 0, "qjson_len({:?}) failed with rc={rc}", path_debug(path));
    len
}

unsafe fn cursor_len_at(cur: &qjson_cursor, path: &[u8]) -> usize {
    let mut len = 0usize;
    let (path_ptr, path_len) = ffi_path(path);
    let rc = qjson_cursor_len(cur, path_ptr, path_len, &mut len);
    assert_eq!(rc, 0, "qjson_cursor_len({:?}) failed with rc={rc}", path_debug(path));
    len
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
        assert!(!path_key_can_be_in_qjson_path(""));
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
    fn raw_object_keys_preserve_field_lookup_bytes() {
        let keys = raw_object_keys(
            br#"{"plain":1,"a\nb":2,"a.b":{"nested":[true,false]},"arr":[{"k":3}]}"#,
        );

        assert_eq!(
            keys,
            vec![
                b"plain".to_vec(),
                br#"a\nb"#.to_vec(),
                b"a.b".to_vec(),
                b"arr".to_vec(),
            ],
        );
    }

    #[test]
    fn field_lookup_key_requires_unique_raw_decoded_identity() {
        assert_eq!(field_lookup_key_for_replay("plain", b"plain", true), Some(b"plain".to_vec()));
        assert_eq!(field_lookup_key_for_replay("a.b", b"a.b", true), Some(b"a.b".to_vec()));
        assert_eq!(field_lookup_key_for_replay("dup", b"dup", false), None);
        assert_eq!(field_lookup_key_for_replay("a\nb", br#"a\nb"#, true), None);
        assert_eq!(field_lookup_key_for_replay("emoji", br#"\u0065moji"#, true), None);
    }

    #[test]
    fn deterministic_replay_cases_cover_path_safe_and_ambiguous_keys() {
        fuzz_one(br#"{"body":{"model":"gpt","temperature":0.5,"ok":true,"none":null,"items":[{"id":1},{"id":2}]}}"#);
        fuzz_one(br#"{"dup":1,"dup":2,"a.b":3,"arr[0]":4,"nested":{"x":true}}"#);
        fuzz_one(br#"{"a\nb":"line\nvalue","\u0064up":1,"dup":2,"emoji":"\uD83D\uDE00","nul":"\u0000","inner":{"same":3,"same":4}}"#);
        fuzz_one(br#"{"d":{"e":[{"":"g"},4.5,-0]},"h":[]}"#);
        fuzz_one(br#"[{"name":"first"},{"name":"second","values":[1,2,3]}]"#);
    }
}
