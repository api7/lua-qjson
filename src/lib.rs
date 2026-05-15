//! lua-quick-decode: Rust JSON decoder for LuaJIT FFI consumers.
//! See docs/superpowers/specs/2026-05-15-rust-quick-json-decode-design.md

pub mod error;
pub(crate) mod scan;
mod skip_cache;
mod doc;
mod path;
mod cursor;
mod decode;
pub mod ffi;

#[doc(hidden)]
pub mod __test_api {
    pub use crate::scan::{Scanner, ScalarScanner};
    #[cfg(target_arch = "x86_64")]
    pub use crate::scan::avx2::Avx2Scanner;
}
