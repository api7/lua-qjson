pub(crate) mod scalar;
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
pub(crate) mod avx2;

use once_cell::sync::OnceCell;

/// A structural scanner: given a JSON byte buffer, append the byte offset of
/// every structural character (`{` `}` `[` `]` `:` `,` `"`) that is NOT inside
/// a string literal to `out`. On shallow validation failure (unclosed string,
/// unmatched bracket), returns `Err(offset)` where `offset` is the byte
/// position the failure was detected at. The offset is informational and not
/// exposed via FFI in v1.
pub trait Scanner {
    fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize>;
}

pub use scalar::ScalarScanner;

type ScanFn = fn(&[u8], &mut Vec<u32>) -> Result<(), usize>;
static SCAN_FN: OnceCell<ScanFn> = OnceCell::new();

pub(crate) fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize> {
    let f = *SCAN_FN.get_or_init(|| {
        #[cfg(all(target_arch = "x86_64", feature = "avx2"))]
        {
            if std::is_x86_feature_detected!("avx2")
                && std::is_x86_feature_detected!("pclmulqdq")
            {
                return <avx2::Avx2Scanner as Scanner>::scan;
            }
        }
        <ScalarScanner as Scanner>::scan
    });
    f(buf, out)
}
