//! lua-quick-decode: Rust JSON decoder for LuaJIT FFI consumers.
//! See docs/superpowers/specs/2026-05-15-rust-quick-json-decode-design.md

pub mod error;
mod scan;
mod skip_cache;
mod doc;
mod path;
mod cursor;
mod decode;
pub mod ffi;
