//! C ABI surface. Every public function is `unsafe extern "C"`.
//! All public symbols use the `qjd_` prefix.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int};
use std::ptr;

use crate::doc::Document;
use crate::error::qjd_err;

/// Opaque type exported to C as `qjd_doc*`.
#[allow(dead_code)]
pub struct qjd_doc(pub(crate) Document<'static>);

#[no_mangle]
pub unsafe extern "C" fn qjd_strerror(code: c_int) -> *const c_char {
    // Hardcoded NUL-terminated map; avoids runtime allocation and lifetime issues.
    let s: &'static [u8] = match code {
        0 => b"ok\0",
        1 => b"JSON parse error\0",
        2 => b"path not found\0",
        3 => b"type mismatch at path\0",
        4 => b"numeric out of range\0",
        5 => b"decode failed\0",
        6 => b"invalid path syntax\0",
        7 => b"invalid argument\0",
        8 => b"out of memory\0",
        _ => b"unknown error code\0",
    };
    s.as_ptr() as *const c_char
}

#[no_mangle]
pub unsafe extern "C" fn qjd_parse(
    buf:     *const u8,
    len:     usize,
    err_out: *mut c_int,
) -> *mut qjd_doc {
    if buf.is_null() || err_out.is_null() {
        if !err_out.is_null() { *err_out = qjd_err::QJD_INVALID_ARG as c_int; }
        return ptr::null_mut();
    }
    let slice: &'static [u8] = std::slice::from_raw_parts(buf, len);
    match Document::parse(slice) {
        Ok(d) => {
            *err_out = qjd_err::QJD_OK as c_int;
            Box::into_raw(Box::new(qjd_doc(d)))
        }
        Err(e) => {
            *err_out = e as c_int;
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn qjd_free(doc: *mut qjd_doc) {
    if doc.is_null() { return; }
    let _ = Box::from_raw(doc);
}
