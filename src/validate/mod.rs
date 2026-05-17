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
/// For container roots (`{`/`[`), we walk `indices` to find the closing
/// bracket where nesting depth returns to zero — that is the actual root
/// end, regardless of how many additional structural chars the buffer has.
/// For scalar roots (no opening bracket), we scan the raw bytes.
pub(crate) fn validate_trailing(
    buf: &[u8],
    indices: &[u32],
) -> Result<(), qjd_err> {
    // Find the first real structural character to determine root kind.
    let first = indices.iter().find(|&&i| i != u32::MAX).copied();

    let root_end = match first {
        None => {
            // No structural chars: bare scalar (number/true/false/null).
            let mut p = 0;
            while p < buf.len() && is_ws(buf[p]) { p += 1; }
            let start = p;
            while p < buf.len() && !is_ws(buf[p]) { p += 1; }
            if start == p { return Ok(()); } // whitespace-only (scan already rejected empty)
            while p < buf.len() && is_ws(buf[p]) { p += 1; }
            p
        }
        Some(first_idx) => {
            match buf[first_idx as usize] {
                b'{' | b'[' => {
                    // Walk indices to find the closing bracket at depth 0.
                    let mut depth: i32 = 0;
                    let mut closer: usize = first_idx as usize;
                    // Track whether we're inside a string (skip string interiors).
                    let mut in_str = false;
                    for &idx in indices {
                        if idx == u32::MAX { break; }
                        let pos = idx as usize;
                        match buf[pos] {
                            b'"' => { in_str = !in_str; }
                            _ if in_str => {}
                            b'{' | b'[' => { depth += 1; }
                            b'}' | b']' => {
                                depth -= 1;
                                if depth == 0 { closer = pos; break; }
                            }
                            _ => {}
                        }
                    }
                    let mut p = closer + 1;
                    while p < buf.len() && is_ws(buf[p]) { p += 1; }
                    p
                }
                b'"' => {
                    // Root is a string: opening quote at first_idx.
                    // The closing quote is the next structural char.
                    let close = indices.iter()
                        .skip(1) // skip the opening quote
                        .find(|&&i| i != u32::MAX)
                        .copied()
                        .unwrap_or(first_idx); // unclosed: scan already rejected
                    let mut p = close as usize + 1;
                    while p < buf.len() && is_ws(buf[p]) { p += 1; }
                    p
                }
                _ => {
                    // Structural char that's not an opener: scan/eager already
                    // would have caught a malformed root. Treat last structural as end.
                    let last = indices.iter().rev()
                        .find(|&&i| i != u32::MAX)
                        .copied()
                        .unwrap_or(first_idx);
                    let mut p = last as usize + 1;
                    while p < buf.len() && is_ws(buf[p]) { p += 1; }
                    p
                }
            }
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
    // Track the last non-quote structural char so check_gap can reject empty
    // gaps in positions where a value is required (after `:` or `,`).
    let mut prev_structural: u8 = 0;
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
                // An open-quote is itself a value, so pass it as the next char:
                // an empty gap before a string is always fine (`:` `"` and `,` `"` are
                // both valid — the string IS the value).
                check_gap(buf, prev_end, pos, prev_structural, b'"')?;
                in_str = true;
                prev_structural = b'"';
            }
            continue;
        }
        if in_str { continue; }

        check_gap(buf, prev_end, pos, prev_structural, b)?;
        prev_end = pos + 1;
        prev_structural = b;
    }
    // Tail gap (top-level scalar like "42"): next char is EOF (0 sentinel)
    check_gap(buf, prev_end, buf.len(), prev_structural, 0)
}

/// `prev_structural`: the last non-quote structural char before this gap.
/// `next_structural`: the structural char immediately after this gap (opens or closes).
fn check_gap(buf: &[u8], start: usize, end: usize, prev_structural: u8, next_structural: u8) -> Result<(), qjd_err> {
    // Strip surrounding whitespace.
    let mut s = start;
    while s < end && is_ws(buf[s]) { s += 1; }
    let mut e = end;
    while e > s && is_ws(buf[e - 1]) { e -= 1; }
    if s == e {
        // Empty gap: a value is required after `:` (object value) or `,` (next
        // element), BUT only when the next token is not a structural value-starter
        // (`"`, `{`, `[`) — those ARE the values. An empty gap before `}` / `]`
        // / `,` when the preceding token demands a value is a structural error.
        // This heuristic catches {"a":}, [,], [1,] without a full grammar walk.
        let next_is_value_starter = matches!(next_structural, b'"' | b'{' | b'[');
        if matches!(prev_structural, b':' | b',') && !next_is_value_starter {
            return Err(qjd_err::QJD_PARSE_ERROR);
        }
        return Ok(());
    }
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
