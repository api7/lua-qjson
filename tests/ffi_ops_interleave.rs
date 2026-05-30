use std::os::raw::{c_char, c_int};
use std::ptr;

use qjson::ffi::{
    qjson_cursor, qjson_cursor_field, qjson_cursor_get_i64, qjson_cursor_get_str,
    qjson_cursor_index, qjson_cursor_object_entry_at, qjson_doc, qjson_free, qjson_get_bool,
    qjson_get_str, qjson_open, qjson_parse,
};

const OK: c_int = 0;

#[derive(Clone, Copy)]
enum Op<'a> {
    OpenRoot {
        out: usize,
    },
    CursorField {
        from: usize,
        key: &'a [u8],
        out: usize,
    },
    CursorIndex {
        from: usize,
        index: usize,
        out: usize,
    },
    GetStr {
        path: &'a [u8],
        expect: &'a [u8],
    },
    CursorGetStr {
        cursor: usize,
        path: &'a [u8],
        expect: &'a [u8],
    },
    CursorObjectEntryAt {
        cursor: usize,
        index: usize,
        expect_key: &'a [u8],
        value_out: usize,
    },
    CursorGetI64 {
        cursor: usize,
        path: &'a [u8],
        expect: i64,
    },
    GetBool {
        path: &'a [u8],
        expect: c_int,
    },
}

#[derive(Clone, Copy)]
struct StrCapture {
    ptr: *const u8,
    len: usize,
}

struct State {
    json: &'static [u8],
    doc: *mut qjson_doc,
    cursors: [qjson_cursor; 8],
}

impl State {
    fn parse(json: &'static [u8]) -> Self {
        let mut err: c_int = -1;
        let doc = unsafe { qjson_parse(json.as_ptr(), json.len(), &mut err) };
        assert!(!doc.is_null(), "qjson_parse failed with err={err}");
        assert_eq!(err, OK);
        Self {
            json,
            doc,
            cursors: std::array::from_fn(|_| zero_cursor()),
        }
    }

    fn apply(&mut self, op: Op<'_>) -> Option<StrCapture> {
        match op {
            Op::OpenRoot { out } => {
                let mut cursor = zero_cursor();
                let rc = unsafe { qjson_open(self.doc, ptr::null(), 0, &mut cursor) };
                assert_eq!(rc, OK);
                self.cursors[out] = cursor;
                None
            }
            Op::CursorField { from, key, out } => {
                let (key_ptr, key_len) = ffi_bytes(key);
                let mut cursor = zero_cursor();
                let rc = unsafe {
                    qjson_cursor_field(&self.cursors[from], key_ptr, key_len, &mut cursor)
                };
                assert_eq!(rc, OK);
                self.cursors[out] = cursor;
                None
            }
            Op::CursorIndex { from, index, out } => {
                let mut cursor = zero_cursor();
                let rc = unsafe { qjson_cursor_index(&self.cursors[from], index, &mut cursor) };
                assert_eq!(rc, OK);
                self.cursors[out] = cursor;
                None
            }
            Op::GetStr { path, expect } => {
                let (path_ptr, path_len) = ffi_bytes(path);
                let capture = unsafe { capture_root_str(self.doc, path_ptr, path_len, expect) };
                Some(capture)
            }
            Op::CursorGetStr {
                cursor,
                path,
                expect,
            } => {
                let (path_ptr, path_len) = ffi_bytes(path);
                let capture = unsafe {
                    capture_cursor_str(&self.cursors[cursor], path_ptr, path_len, expect)
                };
                Some(capture)
            }
            Op::CursorObjectEntryAt {
                cursor,
                index,
                expect_key,
                value_out,
            } => {
                let mut key_ptr: *const u8 = ptr::null();
                let mut key_len = 0usize;
                let mut value = zero_cursor();
                let rc = unsafe {
                    qjson_cursor_object_entry_at(
                        &self.cursors[cursor],
                        index,
                        &mut key_ptr,
                        &mut key_len,
                        &mut value,
                    )
                };
                assert_eq!(rc, OK);
                let key = unsafe { std::slice::from_raw_parts(key_ptr, key_len) };
                assert_eq!(key, expect_key);
                self.cursors[value_out] = value;
                Some(StrCapture {
                    ptr: key_ptr,
                    len: key_len,
                })
            }
            Op::CursorGetI64 {
                cursor,
                path,
                expect,
            } => {
                let (path_ptr, path_len) = ffi_bytes(path);
                let mut out = 0i64;
                let rc = unsafe {
                    qjson_cursor_get_i64(&self.cursors[cursor], path_ptr, path_len, &mut out)
                };
                assert_eq!(rc, OK);
                assert_eq!(out, expect);
                None
            }
            Op::GetBool { path, expect } => {
                let (path_ptr, path_len) = ffi_bytes(path);
                let mut out = -1;
                let rc = unsafe { qjson_get_bool(self.doc, path_ptr, path_len, &mut out) };
                assert_eq!(rc, OK);
                assert_eq!(out, expect);
                None
            }
        }
    }

    fn input_contains(&self, capture: StrCapture) -> bool {
        let start = self.json.as_ptr() as usize;
        let end = start + self.json.len();
        let ptr = capture.ptr as usize;
        ptr >= start && ptr.saturating_add(capture.len) <= end
    }
}

impl Drop for State {
    fn drop(&mut self) {
        unsafe { qjson_free(self.doc) };
        self.doc = ptr::null_mut();
    }
}

#[test]
fn interleaved_get_str_calls_copy_before_scratch_reuse() {
    // Read the longest escaped value first so scratch capacity is established;
    // the later shorter escaped reads should reuse the same base pointer.
    let mut state = State::parse(
        br#"{"plain":"input backed","escaped":"first\nline with enough scratch room","nested":{"escaped":"second\tline","items":[10,"third\nline"],"ok":true},"object":{"long\nkey\twith\tescapes":"v\n1","plain":"x"}}"#,
    );

    let plain = state
        .apply(Op::GetStr {
            path: b"plain",
            expect: b"input backed",
        })
        .unwrap();
    assert!(
        state.input_contains(plain),
        "plain strings should be returned from the borrowed input buffer"
    );

    let first_scratch = state
        .apply(Op::GetStr {
            path: b"escaped",
            expect: b"first\nline with enough scratch room",
        })
        .unwrap();
    assert!(
        !state.input_contains(first_scratch),
        "escaped strings should be decoded through doc scratch"
    );

    state.apply(Op::OpenRoot { out: 0 });
    state.apply(Op::CursorField {
        from: 0,
        key: b"nested",
        out: 1,
    });

    let second_scratch = state
        .apply(Op::CursorGetStr {
            cursor: 1,
            path: b"escaped",
            expect: b"second\tline",
        })
        .unwrap();
    assert_eq!(
        first_scratch.ptr, second_scratch.ptr,
        "escaped root and cursor reads should reuse the same doc scratch buffer"
    );

    state.apply(Op::CursorField {
        from: 1,
        key: b"items",
        out: 2,
    });
    state.apply(Op::CursorIndex {
        from: 2,
        index: 0,
        out: 3,
    });
    state.apply(Op::CursorGetI64 {
        cursor: 3,
        path: b"",
        expect: 10,
    });

    let third_scratch = state
        .apply(Op::CursorGetStr {
            cursor: 2,
            path: b"[1]",
            expect: b"third\nline",
        })
        .unwrap();
    assert_eq!(
        second_scratch.ptr, third_scratch.ptr,
        "subsequent cursor string reads continue to reuse doc scratch"
    );

    state.apply(Op::GetBool {
        path: b"nested.ok",
        expect: 1,
    });
}

#[test]
fn object_entry_keys_share_the_get_str_scratch_contract() {
    let mut state = State::parse(br#"{"object":{"long\nkey\twith\tescapes":"v\n1","plain":"x"}}"#);

    state.apply(Op::OpenRoot { out: 0 });
    state.apply(Op::CursorField {
        from: 0,
        key: b"object",
        out: 1,
    });

    let key_scratch = state
        .apply(Op::CursorObjectEntryAt {
            cursor: 1,
            index: 0,
            expect_key: b"long\nkey\twith\tescapes",
            value_out: 2,
        })
        .unwrap();
    assert!(
        !state.input_contains(key_scratch),
        "escaped object keys should be decoded through doc scratch"
    );

    let value_scratch = state
        .apply(Op::CursorGetStr {
            cursor: 2,
            path: b"",
            expect: b"v\n1",
        })
        .unwrap();
    assert_eq!(
        key_scratch.ptr, value_scratch.ptr,
        "object-entry key reads and value string reads share doc scratch"
    );
}

fn ffi_bytes(bytes: &[u8]) -> (*const c_char, usize) {
    if bytes.is_empty() {
        (ptr::null(), 0)
    } else {
        (bytes.as_ptr() as *const c_char, bytes.len())
    }
}

unsafe fn capture_root_str(
    doc: *mut qjson_doc,
    path_ptr: *const c_char,
    path_len: usize,
    expect: &[u8],
) -> StrCapture {
    let mut out_ptr: *const u8 = ptr::null();
    let mut out_len = 0usize;
    let rc = qjson_get_str(doc, path_ptr, path_len, &mut out_ptr, &mut out_len);
    assert_eq!(rc, OK);
    copy_and_assert(out_ptr, out_len, expect)
}

unsafe fn capture_cursor_str(
    cursor: *const qjson_cursor,
    path_ptr: *const c_char,
    path_len: usize,
    expect: &[u8],
) -> StrCapture {
    let mut out_ptr: *const u8 = ptr::null();
    let mut out_len = 0usize;
    let rc = qjson_cursor_get_str(cursor, path_ptr, path_len, &mut out_ptr, &mut out_len);
    assert_eq!(rc, OK);
    copy_and_assert(out_ptr, out_len, expect)
}

unsafe fn copy_and_assert(ptr: *const u8, len: usize, expect: &[u8]) -> StrCapture {
    assert!(!ptr.is_null(), "qjson returned NULL for a string result");
    let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
    assert_eq!(bytes, expect);
    StrCapture { ptr, len }
}

fn zero_cursor() -> qjson_cursor {
    qjson_cursor {
        doc: ptr::null(),
        idx_start: 0,
        idx_end: 0,
        _reserved0: 0,
        _reserved1: 0,
    }
}
