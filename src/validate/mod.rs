//! Post-scan validators invoked by Document::parse_with_options.
//!
//! Walking the already-emitted `indices` array is intentionally
//! decoupled from the SIMD/scalar scanner paths so the structural
//! scanner code stays untouched.

pub(crate) mod number;
pub(crate) use number::validate_number;

pub(crate) mod strings;
pub(crate) use strings::validate_string_span;

use crate::error::qjson_err;

/// Verify that the maximum bracket-stack depth implied by `indices`
/// does not exceed `max_depth`. Walks indices once; assumes scan() has
/// already validated bracket pairing.
///
/// `indices` is the post-scan vector with the trailing u32::MAX sentinel.
pub(crate) fn validate_depth(
    buf: &[u8],
    indices: &[u32],
    max_depth: u32,
) -> Result<(), qjson_err> {
    let mut depth: u32 = 0;
    for &idx in indices {
        if idx == u32::MAX { break; }
        match buf[idx as usize] {
            b'{' | b'[' => {
                depth += 1;
                if depth > max_depth {
                    return Err(qjson_err::QJSON_NESTING_TOO_DEEP);
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
) -> Result<(), qjson_err> {
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
        return Err(qjson_err::QJSON_TRAILING_CONTENT);
    }
    Ok(())
}

/// Grammar-aware eager pass: walk `indices` once and validate every
/// structural transition, key/value string, and scalar value.
///
/// The state machine tracks the expected next-token kind in each
/// container context (object/array) via a stack. Empty gaps where a
/// value is required (`[,]`, `{"a":}`), missing colons (`{"a"}`),
/// missing commas (`{"a":1"b":2}`), non-string object keys (`{1:1}`),
/// and stray structural tokens (`[1:2]`) all surface here as
/// `QJSON_PARSE_ERROR`.
///
/// Scalar tokens (numbers, `true`, `false`, `null`) live in the byte
/// gap before the *next* structural offset. They are dispatched to
/// `validate_number` or matched against the three literal keywords;
/// the error-code precedence matches the previous heuristic-based
/// `check_gap` so existing tests keep their current error codes.
pub(crate) fn validate_eager_values(
    buf: &[u8],
    indices: &[u32],
) -> Result<(), qjson_err> {
    // Stack of container contexts; the top is the current state.
    // We use a single seed entry `CtxKind::Top` for the root value.
    let mut stack: Vec<CtxKind> = Vec::with_capacity(16);
    stack.push(CtxKind::Top);

    // Byte position just past the previous structural we consumed —
    // i.e. the start of the current gap. A gap may contain a scalar
    // value or be whitespace-only.
    let mut prev_end: usize = 0;

    let mut i: usize = 0;
    while i < indices.len() {
        let idx = indices[i];
        if idx == u32::MAX { break; }
        let pos = idx as usize;
        let b = buf[pos];

        // First, consume any scalar token sitting in the gap before
        // this structural. This may transition the current state from
        // a value-expecting form to its "AfterValue" form.
        consume_scalar_gap(buf, prev_end, pos, stack.last_mut().unwrap())?;

        match b {
            b'{' | b'[' => {
                let cur = stack.last_mut().unwrap();
                match *cur {
                    CtxKind::Top
                    | CtxKind::ArrAfterOpen
                    | CtxKind::ArrAfterComma
                    | CtxKind::ObjAfterColon => {
                        // Transition parent to AfterValue ahead of the
                        // descent; the inner container's close pops back.
                        *cur = parent_after_value(*cur);
                        stack.push(if b == b'{' {
                            CtxKind::ObjAfterOpen
                        } else {
                            CtxKind::ArrAfterOpen
                        });
                    }
                    _ => return Err(qjson_err::QJSON_PARSE_ERROR),
                }
                prev_end = pos + 1;
                i += 1;
            }
            b'}' => {
                let top = stack.pop().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                if !matches!(top, CtxKind::ObjAfterOpen | CtxKind::ObjAfterValue) {
                    return Err(qjson_err::QJSON_PARSE_ERROR);
                }
                if stack.is_empty() { return Err(qjson_err::QJSON_PARSE_ERROR); }
                prev_end = pos + 1;
                i += 1;
            }
            b']' => {
                let top = stack.pop().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                if !matches!(top, CtxKind::ArrAfterOpen | CtxKind::ArrAfterValue) {
                    return Err(qjson_err::QJSON_PARSE_ERROR);
                }
                if stack.is_empty() { return Err(qjson_err::QJSON_PARSE_ERROR); }
                prev_end = pos + 1;
                i += 1;
            }
            b',' => {
                let cur = stack.last_mut().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                match *cur {
                    CtxKind::ArrAfterValue => *cur = CtxKind::ArrAfterComma,
                    CtxKind::ObjAfterValue => *cur = CtxKind::ObjAfterComma,
                    _ => return Err(qjson_err::QJSON_PARSE_ERROR),
                }
                prev_end = pos + 1;
                i += 1;
            }
            b':' => {
                let cur = stack.last_mut().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                match *cur {
                    CtxKind::ObjAfterKey => *cur = CtxKind::ObjAfterColon,
                    _ => return Err(qjson_err::QJSON_PARSE_ERROR),
                }
                prev_end = pos + 1;
                i += 1;
            }
            b'"' => {
                // The scanner pairs the opening and closing quotes; the
                // closing quote is at indices[i + 1].
                if i + 1 >= indices.len() { return Err(qjson_err::QJSON_PARSE_ERROR); }
                let close = indices[i + 1] as usize;
                if close <= pos || close >= buf.len() || buf[close] != b'"' {
                    return Err(qjson_err::QJSON_PARSE_ERROR);
                }
                strings::validate_string_span(&buf[pos + 1 .. close])?;

                let cur = stack.last_mut().ok_or(qjson_err::QJSON_PARSE_ERROR)?;
                match *cur {
                    // Key position in an object.
                    CtxKind::ObjAfterOpen | CtxKind::ObjAfterComma => {
                        *cur = CtxKind::ObjAfterKey;
                    }
                    // Value position (top-level, array element, or object value).
                    CtxKind::Top
                    | CtxKind::ArrAfterOpen
                    | CtxKind::ArrAfterComma
                    | CtxKind::ObjAfterColon => {
                        *cur = parent_after_value(*cur);
                    }
                    _ => return Err(qjson_err::QJSON_PARSE_ERROR),
                }
                prev_end = close + 1;
                i += 2;
            }
            _ => return Err(qjson_err::QJSON_PARSE_ERROR),
        }
    }

    // Tail: a top-level scalar root (e.g. `42`, `true`) lives in the
    // gap after the last structural — or, if there are no structurals,
    // the whole buffer.
    consume_scalar_gap(buf, prev_end, buf.len(), stack.last_mut().unwrap())?;

    // After the walk, the stack must hold exactly one frame: the root
    // context, which must be `TopDone` (root value consumed).
    if stack.len() != 1 || stack[0] != CtxKind::TopDone {
        return Err(qjson_err::QJSON_PARSE_ERROR);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CtxKind {
    Top,           // top-level value not yet consumed
    TopDone,       // top-level value consumed; only whitespace/EOI allowed
    ArrAfterOpen,  // just saw `[`; expect value or `]`
    ArrAfterValue, // just saw a value; expect `,` or `]`
    ArrAfterComma, // just saw `,`; expect value (no trailing comma)
    ObjAfterOpen,  // just saw `{`; expect key (string) or `}`
    ObjAfterKey,   // just saw key string; expect `:`
    ObjAfterColon, // just saw `:`; expect value
    ObjAfterValue, // just saw value; expect `,` or `}`
    ObjAfterComma, // just saw `,`; expect key (no trailing comma)
}

/// Transition the value-expecting state to its corresponding
/// "after value" state once the value (scalar / string / container)
/// has been consumed.
#[inline]
fn parent_after_value(s: CtxKind) -> CtxKind {
    match s {
        CtxKind::Top           => CtxKind::TopDone,
        CtxKind::ArrAfterOpen  => CtxKind::ArrAfterValue,
        CtxKind::ArrAfterComma => CtxKind::ArrAfterValue,
        CtxKind::ObjAfterColon => CtxKind::ObjAfterValue,
        other                  => other, // unreachable for callers
    }
}

/// Examine the byte gap `[start, end)` between two structurals.
/// If the gap contains a scalar token, validate it and transition
/// `*state` to its corresponding "AfterValue" form. If the gap is
/// whitespace only, leave `*state` unchanged — the next structural's
/// own check rejects empty values where they are not allowed
/// (e.g. `ObjAfterColon` followed by `}` is caught when `}` pops).
fn consume_scalar_gap(
    buf: &[u8],
    start: usize,
    end: usize,
    state: &mut CtxKind,
) -> Result<(), qjson_err> {
    // Strip whitespace.
    let mut s = start;
    while s < end && is_ws(buf[s]) { s += 1; }
    let mut e = end;
    while e > s && is_ws(buf[e - 1]) { e -= 1; }

    if s == e {
        return Ok(());
    }

    // The gap is non-empty: it MUST be a scalar token, and the state
    // must allow a scalar at this position. Strings and containers are
    // handled by their structural-token cases, not here.
    if !matches!(
        *state,
        CtxKind::Top
            | CtxKind::ArrAfterOpen
            | CtxKind::ArrAfterComma
            | CtxKind::ObjAfterColon
    ) {
        return Err(qjson_err::QJSON_PARSE_ERROR);
    }

    validate_scalar(&buf[s..e])?;
    *state = parent_after_value(*state);
    Ok(())
}

/// Dispatch a non-empty whitespace-trimmed scalar token to its
/// grammar validator. Mirrors the previous `check_gap` precedence:
///   - `true` / `false` / `null` exact → Ok
///   - `NaN` / `Infinity` → `QJSON_INVALID_NUMBER` (via validate_number)
///   - `-` / digit / `+` / `.` → `validate_number`
///   - Else → `QJSON_PARSE_ERROR`
fn validate_scalar(scalar: &[u8]) -> Result<(), qjson_err> {
    match scalar[0] {
        b't' => if scalar == b"true"  { Ok(()) } else { Err(qjson_err::QJSON_PARSE_ERROR) },
        b'f' => if scalar == b"false" { Ok(()) } else { Err(qjson_err::QJSON_PARSE_ERROR) },
        b'n' => if scalar == b"null"  { Ok(()) } else { Err(qjson_err::QJSON_PARSE_ERROR) },
        b'-' | b'0'..=b'9' | b'+' | b'.' => number::validate_number(scalar),
        _ if scalar == b"NaN" || scalar == b"Infinity" => number::validate_number(scalar),
        _ => Err(qjson_err::QJSON_PARSE_ERROR),
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
            Err(qjson_err::QJSON_NESTING_TOO_DEEP),
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
            Err(qjson_err::QJSON_TRAILING_CONTENT),
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
            Err(qjson_err::QJSON_TRAILING_CONTENT),
        );
    }

    // ── grammar state machine (validate_eager_values) ──────────────────

    #[test]
    fn grammar_accepts_empty_containers() {
        for buf in [&b"{}"[..], &b"[]"[..]] {
            assert!(validate_eager_values(buf, &ix(buf)).is_ok(),
                "grammar should accept {:?}", buf);
        }
    }

    #[test]
    fn grammar_accepts_simple_values() {
        for buf in [
            &b"{\"a\":1}"[..], &b"[1,2,3]"[..],
            &b"[true,false,null]"[..], &b"\"hi\""[..], &b"42"[..],
            &b"{\"a\":[1,{\"b\":2}]}"[..],
        ] {
            assert!(validate_eager_values(buf, &ix(buf)).is_ok(),
                "grammar should accept {:?}", buf);
        }
    }

    #[test]
    fn grammar_rejects_missing_colon() {
        let buf = b"{\"a\"}";
        assert_eq!(validate_eager_values(buf, &ix(buf)), Err(qjson_err::QJSON_PARSE_ERROR));
    }

    #[test]
    fn grammar_rejects_leading_comma_with_value() {
        let buf = b"[,1]";
        assert_eq!(validate_eager_values(buf, &ix(buf)), Err(qjson_err::QJSON_PARSE_ERROR));
    }

    #[test]
    fn grammar_rejects_missing_comma_in_object() {
        let buf = b"{\"a\":1\"b\":2}";
        assert_eq!(validate_eager_values(buf, &ix(buf)), Err(qjson_err::QJSON_PARSE_ERROR));
    }

    #[test]
    fn grammar_rejects_non_string_object_key() {
        let buf = b"{1:1}";
        assert_eq!(validate_eager_values(buf, &ix(buf)), Err(qjson_err::QJSON_PARSE_ERROR));
    }

    #[test]
    fn grammar_rejects_colon_in_array() {
        let buf = b"[1:2]";
        assert_eq!(validate_eager_values(buf, &ix(buf)), Err(qjson_err::QJSON_PARSE_ERROR));
    }

    #[test]
    fn grammar_rejects_missing_comma_between_arrays() {
        let buf = b"[3[4]]";
        assert_eq!(validate_eager_values(buf, &ix(buf)), Err(qjson_err::QJSON_PARSE_ERROR));
    }

    #[test]
    fn grammar_rejects_trailing_garbage_inside_object() {
        let buf = b"{\"a\":\"a\" 123}";
        assert_eq!(validate_eager_values(buf, &ix(buf)), Err(qjson_err::QJSON_PARSE_ERROR));
    }
}
