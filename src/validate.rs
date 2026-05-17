//! Post-scan validators invoked by Document::parse_with_options.
//!
//! Walking the already-emitted `indices` array is intentionally
//! decoupled from the SIMD/scalar scanner paths so the structural
//! scanner code stays untouched.

use crate::error::qjd_err;

/// Verify that the maximum bracket-stack depth implied by `indices`
/// does not exceed `max_depth`. Walks indices once; assumes scan() has
/// already validated bracket pairing.
///
/// `indices` is the post-scan vector with the trailing u32::MAX sentinel.
pub(crate) fn validate_depth(
    buf: &[u8],
    indices: &[u32],
    max_depth: u32,
) -> Result<(), qjd_err> {
    let mut depth: u32 = 0;
    for &idx in indices {
        if idx == u32::MAX { break; }
        match buf[idx as usize] {
            b'{' | b'[' => {
                depth += 1;
                if depth > max_depth {
                    return Err(qjd_err::QJD_NESTING_TOO_DEEP);
                }
            }
            b'}' | b']' => {
                // Cannot underflow: scan() already validated pairing.
                depth -= 1;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ix(buf: &[u8]) -> Vec<u32> {
        let mut v = Vec::new();
        crate::scan::scan(buf, &mut v).unwrap();
        v.push(u32::MAX);
        v
    }

    #[test]
    fn under_limit_ok() {
        let buf = b"[[1]]";
        assert!(validate_depth(buf, &ix(buf), 2).is_ok());
    }

    #[test]
    fn over_limit_rejected() {
        let buf = b"[[[1]]]";
        assert_eq!(
            validate_depth(buf, &ix(buf), 2),
            Err(qjd_err::QJD_NESTING_TOO_DEEP),
        );
    }
}
