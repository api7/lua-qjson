use super::Scanner;

pub struct ScalarScanner;

impl Scanner for ScalarScanner {
    fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize> {
        scan_and_validate(buf, out)
    }
}

/// Single-pass: emit structural offsets AND validate bracket pairing inline.
/// Replaces the two-pass `scan_emit_resume` + `validate_brackets` sequence.
pub(crate) fn scan_and_validate(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize> {
    out.reserve(buf.len() / 6);
    let mut i = 0usize;
    let mut in_str = false;
    let mut stack: Vec<u8> = Vec::with_capacity(32);
    while i < buf.len() {
        let b = buf[i];
        if in_str {
            if b == b'\\' { i += 2; continue; }
            if b == b'"' { in_str = false; out.push(i as u32); }
            i += 1;
            continue;
        }
        match b {
            b'"'        => { in_str = true; out.push(i as u32); }
            b'{' | b'[' => { stack.push(b); out.push(i as u32); }
            b'}'        => { out.push(i as u32); if stack.pop() != Some(b'{') { return Err(i); } }
            b']'        => { out.push(i as u32); if stack.pop() != Some(b'[') { return Err(i); } }
            b':' | b',' => { out.push(i as u32); }
            _ => {}
        }
        i += 1;
    }
    if in_str { return Err(buf.len()); }
    if !stack.is_empty() { return Err(buf.len()); }
    Ok(())
}

/// Emit structural-character offsets for `buf[start..]`, continuing from a
/// given in-string state. Does NOT validate bracket pairing; the caller is
/// responsible for running `validate_brackets` over the emitted offsets.
///
/// Used by `ScalarScanner::scan` (with start=0, in_str_init=false) and as
/// the unaligned-tail handler by `Avx2Scanner::scan` (with the carried
/// in-string state from the last AVX2 chunk).
#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
pub(crate) fn scan_emit_resume(
    buf: &[u8],
    start: usize,
    in_str_init: bool,
    out: &mut Vec<u32>,
) -> Result<(), usize> {
    let mut i = start;
    let mut in_str = in_str_init;

    while i < buf.len() {
        let b = buf[i];

        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
                out.push(i as u32);
            }
            i += 1;
            continue;
        }

        match b {
            b'"' => {
                in_str = true;
                out.push(i as u32);
            }
            b'{' | b'}' | b'[' | b']' | b',' | b':' => out.push(i as u32),
            _ => {}
        }
        i += 1;
    }

    if in_str {
        return Err(buf.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(input: &[u8]) -> Result<Vec<u32>, usize> {
        let mut v = Vec::new();
        ScalarScanner::scan(input, &mut v).map(|_| v)
    }

    #[test]
    fn empty_object() {
        assert_eq!(scan(b"{}"), Ok(vec![0, 1]));
    }

    #[test]
    fn empty_array() {
        assert_eq!(scan(b"[]"), Ok(vec![0, 1]));
    }

    #[test]
    fn simple_object() {
        // {"a":1}
        //  ^   ^
        //  012345 6
        assert_eq!(scan(b"{\"a\":1}"), Ok(vec![0, 1, 3, 4, 6]));
        //                            { " " : }
    }

    #[test]
    fn nested_object() {
        // {"a":{"b":2}}
        //  0   4    9 10 11 12
        let r = scan(b"{\"a\":{\"b\":2}}").unwrap();
        // Positions of: { " " : { " " : } }
        assert_eq!(r, vec![0, 1, 3, 4, 5, 6, 8, 9, 11, 12]);
    }

    #[test]
    fn array_with_strings() {
        // ["a","b"]
        // 0 12 3 4 56 7 8
        let r = scan(b"[\"a\",\"b\"]").unwrap();
        assert_eq!(r, vec![0, 1, 3, 4, 5, 7, 8]);
    }

    #[test]
    fn escape_double_quote_in_string() {
        // {"a":"x\"y"}
        // 0 12 3 4 5 678 9 10 11
        let r = scan(b"{\"a\":\"x\\\"y\"}").unwrap();
        assert_eq!(r, vec![0, 1, 3, 4, 5, 10, 11]);
    }

    #[test]
    fn escape_backslash_then_quote() {
        // {"a":"x\\"}     (string content is `x\`)
        // 0 12 3 4 5 678 9 10
        let r = scan(b"{\"a\":\"x\\\\\"}").unwrap();
        assert_eq!(r, vec![0, 1, 3, 4, 5, 9, 10]);
    }

    #[test]
    fn unclosed_string_is_error() {
        assert!(scan(b"{\"a\":\"foo").is_err());
    }

    #[test]
    fn unmatched_closer_is_error() {
        assert!(scan(b"]").is_err());
    }

    #[test]
    fn mismatched_bracket_type_is_error() {
        assert!(scan(b"{]").is_err());
    }

    #[test]
    fn deeply_nested() {
        let mut buf = vec![b'['; 100];
        buf.resize(200, b']');
        let r = scan(&buf).unwrap();
        assert_eq!(r.len(), 200);
    }
}
