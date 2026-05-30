#![no_main]

use std::hint::black_box;
use std::os::raw::c_char;
use std::ptr;

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use qjson::ffi::{
    qjson_cursor, qjson_cursor_field, qjson_cursor_get_bool, qjson_cursor_get_f64,
    qjson_cursor_get_i64, qjson_cursor_get_str, qjson_cursor_index,
    qjson_cursor_object_entry_at, qjson_doc, qjson_free, qjson_get_bool, qjson_get_f64,
    qjson_error, qjson_get_i64, qjson_get_str, qjson_open, qjson_parse,
};

const MAX_OPS: usize = 256;
const MAX_CURSORS: usize = 16;

#[derive(Arbitrary, Debug)]
struct Case<'a> {
    json: &'a [u8],
    ops: Vec<Op<'a>>,
}

#[derive(Arbitrary, Debug)]
enum Op<'a> {
    Parse { json: &'a [u8] },
    GetStr { cursor_slot: u8, path: &'a [u8] },
    GetI64 { cursor_slot: u8, path: &'a [u8] },
    GetF64 { cursor_slot: u8, path: &'a [u8] },
    GetBool { cursor_slot: u8, path: &'a [u8] },
    CursorField { cursor_slot: u8, key: &'a [u8] },
    CursorIndex { cursor_slot: u8, index: u32 },
    ObjectEntryAt { cursor_slot: u8, index: u32 },
    Free,
}

fuzz_target!(|data: &[u8]| {
    let Ok(case) = Case::arbitrary(&mut Unstructured::new(data)) else {
        return;
    };

    let mut state = State::default();
    unsafe {
        state.parse(case.json);
        for op in case.ops.iter().take(MAX_OPS) {
            state.apply(op);
        }
    }
});

#[derive(Default)]
struct State {
    doc: *mut qjson_doc,
    cursors: Vec<qjson_cursor>,
}

impl State {
    unsafe fn parse(&mut self, json: &[u8]) {
        self.free();

        let mut err = qjson_error::default();
        self.doc = qjson_parse(json.as_ptr(), json.len(), &mut err);
        black_box(err);

        if self.doc.is_null() {
            return;
        }

        let mut root = zero_cursor();
        let rc = qjson_open(self.doc, ptr::null(), 0, &mut root);
        black_box(rc);
        if rc == 0 {
            self.store_cursor(root, 0);
        }
    }

    unsafe fn apply(&mut self, op: &Op<'_>) {
        match *op {
            Op::Parse { json } => self.parse(json),
            Op::GetStr { cursor_slot, path } => self.get_str(cursor_slot, path),
            Op::GetI64 { cursor_slot, path } => self.get_i64(cursor_slot, path),
            Op::GetF64 { cursor_slot, path } => self.get_f64(cursor_slot, path),
            Op::GetBool { cursor_slot, path } => self.get_bool(cursor_slot, path),
            Op::CursorField { cursor_slot, key } => self.cursor_field(cursor_slot, key),
            Op::CursorIndex { cursor_slot, index } => self.cursor_index(cursor_slot, index),
            Op::ObjectEntryAt { cursor_slot, index } => {
                self.object_entry_at(cursor_slot, index);
            }
            Op::Free => self.free(),
        }
    }

    unsafe fn get_str(&self, cursor_slot: u8, path: &[u8]) {
        let (path_ptr, path_len) = ffi_bytes(path);

        let mut ptr_out: *const u8 = ptr::null();
        let mut len_out = 0usize;
        let rc = qjson_get_str(self.doc, path_ptr, path_len, &mut ptr_out, &mut len_out);
        black_box(rc);
        if rc == 0 {
            consume_ffi_bytes(ptr_out, len_out);
        }

        ptr_out = ptr::null();
        len_out = 0;
        let rc = qjson_cursor_get_str(
            self.cursor_ptr(cursor_slot),
            path_ptr,
            path_len,
            &mut ptr_out,
            &mut len_out,
        );
        black_box(rc);
        if rc == 0 {
            consume_ffi_bytes(ptr_out, len_out);
        }
    }

    unsafe fn get_i64(&self, cursor_slot: u8, path: &[u8]) {
        let (path_ptr, path_len) = ffi_bytes(path);

        let mut out = 0i64;
        let rc = qjson_get_i64(self.doc, path_ptr, path_len, &mut out);
        black_box((rc, out));

        out = 0;
        let rc = qjson_cursor_get_i64(self.cursor_ptr(cursor_slot), path_ptr, path_len, &mut out);
        black_box((rc, out));
    }

    unsafe fn get_f64(&self, cursor_slot: u8, path: &[u8]) {
        let (path_ptr, path_len) = ffi_bytes(path);

        let mut out = 0.0f64;
        let rc = qjson_get_f64(self.doc, path_ptr, path_len, &mut out);
        black_box((rc, out));

        out = 0.0;
        let rc = qjson_cursor_get_f64(self.cursor_ptr(cursor_slot), path_ptr, path_len, &mut out);
        black_box((rc, out));
    }

    unsafe fn get_bool(&self, cursor_slot: u8, path: &[u8]) {
        let (path_ptr, path_len) = ffi_bytes(path);

        let mut out = 0;
        let rc = qjson_get_bool(self.doc, path_ptr, path_len, &mut out);
        black_box((rc, out));

        out = 0;
        let rc = qjson_cursor_get_bool(self.cursor_ptr(cursor_slot), path_ptr, path_len, &mut out);
        black_box((rc, out));
    }

    unsafe fn cursor_field(&mut self, cursor_slot: u8, key: &[u8]) {
        let (key_ptr, key_len) = ffi_bytes(key);
        let mut out = zero_cursor();
        let rc = qjson_cursor_field(self.cursor_ptr(cursor_slot), key_ptr, key_len, &mut out);
        black_box(rc);
        if rc == 0 {
            self.store_cursor(out, cursor_slot);
        }
    }

    unsafe fn cursor_index(&mut self, cursor_slot: u8, index: u32) {
        let mut out = zero_cursor();
        let rc = qjson_cursor_index(self.cursor_ptr(cursor_slot), index as usize, &mut out);
        black_box(rc);
        if rc == 0 {
            self.store_cursor(out, cursor_slot);
        }
    }

    unsafe fn object_entry_at(&mut self, cursor_slot: u8, index: u32) {
        let mut key_ptr: *const u8 = ptr::null();
        let mut key_len = 0usize;
        let mut value = zero_cursor();
        let rc = qjson_cursor_object_entry_at(
            self.cursor_ptr(cursor_slot),
            index as usize,
            &mut key_ptr,
            &mut key_len,
            &mut value,
        );
        black_box(rc);
        if rc == 0 {
            consume_ffi_bytes(key_ptr, key_len);
            self.store_cursor(value, cursor_slot);
        }
    }

    unsafe fn free(&mut self) {
        qjson_free(self.doc);
        self.doc = ptr::null_mut();
        self.cursors.clear();
    }

    fn cursor_ptr(&self, slot: u8) -> *const qjson_cursor {
        if self.cursors.is_empty() {
            return ptr::null();
        }
        &self.cursors[slot as usize % self.cursors.len()]
    }

    fn store_cursor(&mut self, cursor: qjson_cursor, slot: u8) {
        if self.cursors.len() < MAX_CURSORS {
            self.cursors.push(cursor);
        } else {
            self.cursors[slot as usize % MAX_CURSORS] = cursor;
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        unsafe {
            self.free();
        }
    }
}

fn ffi_bytes(bytes: &[u8]) -> (*const c_char, usize) {
    if bytes.is_empty() {
        (ptr::null(), 0)
    } else {
        (bytes.as_ptr() as *const c_char, bytes.len())
    }
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

unsafe fn consume_ffi_bytes(ptr: *const u8, len: usize) {
    if len == 0 {
        black_box(0u8);
        return;
    }

    assert!(!ptr.is_null(), "qjson returned NULL pointer with non-zero length");

    let bytes = std::slice::from_raw_parts(ptr, len);
    let acc = bytes[0] ^ bytes[len / 2] ^ bytes[len - 1];
    black_box(acc);
}
