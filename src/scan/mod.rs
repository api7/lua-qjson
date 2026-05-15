pub(crate) mod scalar;
#[cfg(target_arch = "x86_64")]
pub(crate) mod avx2;

/// A structural scanner: given a JSON byte buffer, append the byte offset of
/// every structural character (`{` `}` `[` `]` `:` `,` `"`) that is NOT inside
/// a string literal to `out`. On shallow validation failure (unclosed string,
/// unmatched bracket), returns `Err(offset)` where `offset` is the byte
/// position the failure was detected at. The offset is informational and not
/// exposed via FFI in v1.
pub(crate) trait Scanner {
    fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize>;
}

pub(crate) use scalar::ScalarScanner;
