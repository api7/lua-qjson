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

/// Walk a sequence of already-emitted structural offsets and verify that
/// `{`/`}` and `[`/`]` are properly paired. String quotes toggle an
/// `in_string` flag and are otherwise skipped. This pass trusts the emit
/// phase: a forged quote in the index list would flip `in_string` and
/// mask subsequent bracket mismatches, so the function is correctness-
/// coupled with the scanner that produced `indices`, not defensive
/// against arbitrary inputs.
///
/// On the first mismatch, returns `Err(offset_in_buf)`. On unmatched
/// openers at end of input, returns `Err(buf.len())`.
pub(crate) fn validate_brackets(buf: &[u8], indices: &[u32]) -> Result<(), usize> {
    let mut stack: Vec<u8> = Vec::with_capacity(32);
    let mut in_string = false;

    for &idx in indices {
        let pos = idx as usize;
        let b = buf[pos];

        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }

        match b {
            b'{' | b'[' => stack.push(b),
            b'}' if stack.pop() != Some(b'{') => return Err(pos),
            b']' if stack.pop() != Some(b'[') => return Err(pos),
            _ => {}
        }
    }

    if !stack.is_empty() {
        return Err(buf.len());
    }
    Ok(())
}
