//! qjson: Rust JSON decoder for LuaJIT FFI consumers.

pub mod error;
pub mod options;
pub(crate) mod scan;
mod skip_cache;
pub mod doc;
mod path;
mod cursor;
mod decode;
mod validate;
pub mod ffi;

#[doc(hidden)]
pub mod __test_api {
    pub use crate::scan::{Scanner, ScalarScanner};
    #[cfg(all(target_arch = "x86_64", feature = "avx2"))]
    pub use crate::scan::avx2::Avx2Scanner;
    #[cfg(target_arch = "aarch64")]
    pub use crate::scan::neon::NeonScanner;
}

#[doc(hidden)]
pub mod __bench_api {
    pub use crate::doc::Document;
    pub use crate::options::{Options, QJSON_MODE_LAZY};
}
