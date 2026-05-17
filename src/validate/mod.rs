//! Post-scan validators invoked by Document::parse_with_options.
//!
//! Walking the already-emitted `indices` array is intentionally
//! decoupled from the SIMD/scalar scanner paths so the structural
//! scanner code stays untouched.

pub(crate) mod number;
pub(crate) use number::validate_number;

pub(crate) mod strings;
pub(crate) use strings::validate_string_span;

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

/// Verify there is no non-whitespace content after the root value.
///
/// The root value's closer is the last non-sentinel structural offset
/// in `indices` for a container, or the start of the scalar's trailing
/// whitespace for a top-level scalar value. We locate the position
/// `end_of_root` past which only whitespace is allowed.
pub(crate) fn validate_trailing(
    buf: &[u8],
    indices: &[u32],
) -> Result<(), qjd_err> {
    // Find the last real offset (skip the u32::MAX sentinel).
    let last = indices.iter().rev()
        .find(|&&i| i != u32::MAX)
        .copied();

    let root_end = match last {
        // No structural chars at all: input is whitespace or a bare scalar.
        // Bare scalar: locate the end by scanning until whitespace or EOF.
        None => {
            // Strip leading whitespace, then find the scalar's terminator.
            let mut p = 0;
            while p < buf.len() && is_ws(buf[p]) { p += 1; }
            let start = p;
            // Scan until next whitespace (end of scalar token).
            while p < buf.len() && !is_ws(buf[p]) { p += 1; }
            if start == p { return Ok(()); } // input was only whitespace
            // Advance past trailing whitespace so `42   ` is accepted.
            while p < buf.len() && is_ws(buf[p]) { p += 1; }
            p
        }
        // Structural close (`}` or `]`) of root container, OR root quote
        // close, OR last structural (`,`/`:`/`{`/`[`) — in which case the
        // parse should already have failed at scan(). The only "valid root
        // ending in a structural" cases are a closing `}` / `]` / `"`.
        Some(last_idx) => {
            let mut p = last_idx as usize + 1;
            // Advance past any trailing whitespace.
            while p < buf.len() && is_ws(buf[p]) { p += 1; }
            p
        }
    };

    if root_end < buf.len() {
        return Err(qjd_err::QJD_TRAILING_CONTENT);
    }
    Ok(())
}

#[inline(always)]
fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
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

    #[test]
    fn trailing_clean_container() {
        let buf = b"{}";
        assert!(validate_trailing(buf, &ix(buf)).is_ok());
    }

    #[test]
    fn trailing_whitespace_accepted() {
        let buf = b"{}   \n\t";
        assert!(validate_trailing(buf, &ix(buf)).is_ok());
    }

    #[test]
    fn trailing_garbage_rejected() {
        let buf = b"{}garbage";
        assert_eq!(
            validate_trailing(buf, &ix(buf)),
            Err(qjd_err::QJD_TRAILING_CONTENT),
        );
    }

    #[test]
    fn bare_scalar_trailing_ws_accepted() {
        let buf = b"42 \n\t";
        assert!(validate_trailing(buf, &ix(buf)).is_ok());
    }

    #[test]
    fn two_root_scalars_rejected() {
        let buf = b"1 2";
        assert_eq!(
            validate_trailing(buf, &ix(buf)),
            Err(qjd_err::QJD_TRAILING_CONTENT),
        );
    }
}
