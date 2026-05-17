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

/// Walk `indices` and validate every scalar value (numbers + strings).
/// Called only in EAGER mode.
pub(crate) fn validate_eager_values(
    buf: &[u8],
    indices: &[u32],
) -> Result<(), qjd_err> {
    let mut i = 0;
    while i + 1 < indices.len() {
        let idx = indices[i];
        if idx == u32::MAX { break; }
        let pos = idx as usize;
        let b = buf[pos];

        // Strings: opening quote here, closing quote at indices[i+1].
        // (The scanner emits BOTH quotes of a string in order.)
        if b == b'"' {
            let close = indices[i + 1] as usize;
            // Defensive: scanner pairs quotes correctly, but guard anyway.
            if close <= pos || close >= buf.len() || buf[close] != b'"' {
                return Err(qjd_err::QJD_PARSE_ERROR);
            }
            let span = &buf[pos + 1 .. close];
            strings::validate_string_span(span)?;
            i += 2;
            continue;
        }

        // Container brackets and `:`/`,` are not values; skip.
        if matches!(b, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            i += 1;
            continue;
        }

        // Should not happen: scanner only emits the 7 structural chars.
        return Err(qjd_err::QJD_PARSE_ERROR);
    }

    // Scalar values (numbers, true, false, null) live in the gaps between
    // structural offsets. Walk those gaps and dispatch.
    validate_scalars_in_gaps(buf, indices)
}

/// For each consecutive pair of structural offsets, examine the bytes
/// between them. If the gap contains a scalar (anything other than
/// whitespace), validate its grammar.
fn validate_scalars_in_gaps(buf: &[u8], indices: &[u32]) -> Result<(), qjd_err> {
    let mut prev_end: usize = 0;
    let mut in_str = false;
    for &idx in indices {
        if idx == u32::MAX { break; }
        let pos = idx as usize;
        let b = buf[pos];

        if b == b'"' {
            // Toggle: the bytes between two quotes are the string interior
            // (already validated above). Skip gap-scanning across them.
            if in_str {
                in_str = false;
                prev_end = pos + 1;
            } else {
                // Validate any scalar in the gap leading up to this quote.
                check_gap(buf, prev_end, pos)?;
                in_str = true;
            }
            continue;
        }
        if in_str { continue; }

        check_gap(buf, prev_end, pos)?;
        prev_end = pos + 1;
    }
    // Tail gap (top-level scalar like "42")
    check_gap(buf, prev_end, buf.len())
}

fn check_gap(buf: &[u8], start: usize, end: usize) -> Result<(), qjd_err> {
    // Strip surrounding whitespace.
    let mut s = start;
    while s < end && is_ws(buf[s]) { s += 1; }
    let mut e = end;
    while e > s && is_ws(buf[e - 1]) { e -= 1; }
    if s == e { return Ok(()); }
    let scalar = &buf[s..e];

    // Dispatch on first byte.
    match scalar[0] {
        b't' => if scalar == b"true"  { Ok(()) } else { Err(qjd_err::QJD_PARSE_ERROR) },
        b'f' => if scalar == b"false" { Ok(()) } else { Err(qjd_err::QJD_PARSE_ERROR) },
        b'n' => if scalar == b"null"  { Ok(()) } else { Err(qjd_err::QJD_PARSE_ERROR) },
        // RFC-valid and common malformed number starters (+, ., -, digit).
        b'-' | b'0'..=b'9' | b'+' | b'.' => number::validate_number(scalar),
        // NaN / Infinity are "meant as numbers" → QJD_INVALID_NUMBER, not parse error.
        _ if scalar == b"NaN" || scalar == b"Infinity" => number::validate_number(scalar),
        // Wrong-case literals (TRUE, NULL), identifiers (undefined), other garbage.
        _ => Err(qjd_err::QJD_PARSE_ERROR),
    }
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
