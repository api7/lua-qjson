use crate::error::qjd_err;

/// Decode the JSON string between `start` and `end` (exclusive of the
/// surrounding quotes) into `scratch` if escapes are present. Returns
/// (ptr, len) pointing into either `buf` (no escapes) or `scratch`.
pub(crate) fn decode_string(
    buf: &[u8], start: usize, end: usize, scratch: &mut Vec<u8>,
) -> Result<(*const u8, usize), qjd_err> {
    let slice = &buf[start..end];
    crate::validate::validate_string_span(slice)?;
    if memchr::memchr(b'\\', slice).is_none() {
        return Ok((slice.as_ptr(), slice.len()));
    }

    scratch.clear();
    scratch.reserve(slice.len());

    let mut i = 0;
    while i < slice.len() {
        let b = slice[i];
        if b != b'\\' {
            scratch.push(b);
            i += 1;
            continue;
        }
        // Escape.
        if i + 1 >= slice.len() { return Err(qjd_err::QJD_DECODE_FAILED); }
        match slice[i + 1] {
            b'"'  => { scratch.push(b'"');  i += 2; }
            b'\\' => { scratch.push(b'\\'); i += 2; }
            b'/'  => { scratch.push(b'/');  i += 2; }
            b'b'  => { scratch.push(0x08);  i += 2; }
            b'f'  => { scratch.push(0x0C);  i += 2; }
            b'n'  => { scratch.push(b'\n'); i += 2; }
            b'r'  => { scratch.push(b'\r'); i += 2; }
            b't'  => { scratch.push(b'\t'); i += 2; }
            b'u'  => {
                if i + 6 > slice.len() { return Err(qjd_err::QJD_DECODE_FAILED); }
                let h = parse_hex4(&slice[i + 2 .. i + 6])?;
                i += 6;
                let cp = if (0xD800..=0xDBFF).contains(&h) {
                    // High surrogate; expect low surrogate next.
                    if i + 6 > slice.len() || &slice[i..i + 2] != b"\\u" {
                        return Err(qjd_err::QJD_DECODE_FAILED);
                    }
                    let l = parse_hex4(&slice[i + 2 .. i + 6])?;
                    if !(0xDC00..=0xDFFF).contains(&l) {
                        return Err(qjd_err::QJD_DECODE_FAILED);
                    }
                    i += 6;
                    0x10000 + ((h - 0xD800) << 10) + (l - 0xDC00)
                } else if (0xDC00..=0xDFFF).contains(&h) {
                    return Err(qjd_err::QJD_DECODE_FAILED);
                } else {
                    h
                };
                encode_utf8(cp, scratch);
            }
            _ => return Err(qjd_err::QJD_DECODE_FAILED),
        }
    }

    Ok((scratch.as_ptr(), scratch.len()))
}

fn parse_hex4(bytes: &[u8]) -> Result<u32, qjd_err> {
    let mut v: u32 = 0;
    for &b in bytes {
        v <<= 4;
        v |= match b {
            b'0'..=b'9' => (b - b'0') as u32,
            b'a'..=b'f' => (b - b'a' + 10) as u32,
            b'A'..=b'F' => (b - b'A' + 10) as u32,
            _ => return Err(qjd_err::QJD_DECODE_FAILED),
        };
    }
    Ok(v)
}

fn encode_utf8(cp: u32, out: &mut Vec<u8>) {
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.push(0xC0 | (cp >> 6) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else if cp < 0x10000 {
        out.push(0xE0 | (cp >> 12) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    } else {
        out.push(0xF0 | (cp >> 18) as u8);
        out.push(0x80 | ((cp >> 12) & 0x3F) as u8);
        out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
        out.push(0x80 | (cp & 0x3F) as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &[u8]) -> Result<Vec<u8>, qjd_err> {
        let mut scratch = Vec::new();
        let (p, n) = decode_string(s, 0, s.len(), &mut scratch)?;
        Ok(unsafe { std::slice::from_raw_parts(p, n) }.to_vec())
    }

    #[test]
    fn no_escape_returns_input() {
        assert_eq!(d(b"hello").unwrap(), b"hello".to_vec());
    }

    #[test]
    fn escaped_quote() {
        assert_eq!(d(b"a\\\"b").unwrap(), b"a\"b".to_vec());
    }

    #[test]
    fn escaped_backslash() {
        assert_eq!(d(b"a\\\\b").unwrap(), b"a\\b".to_vec());
    }

    #[test]
    fn escaped_newline() {
        assert_eq!(d(b"a\\nb").unwrap(), b"a\nb".to_vec());
    }

    #[test]
    fn escaped_tab() {
        assert_eq!(d(b"a\\tb").unwrap(), b"a\tb".to_vec());
    }

    #[test]
    fn escaped_unicode_ascii() {
        // A = 'A'
        assert_eq!(d(b"a\\u0041b").unwrap(), b"aAb".to_vec());
    }

    #[test]
    fn escaped_unicode_2byte() {
        // é = 0xC3 0xA9
        assert_eq!(d(b"\\u00e9").unwrap(), vec![0xC3, 0xA9]);
    }

    #[test]
    fn escaped_unicode_3byte() {
        // 中 = 0xE4 0xB8 0xAD
        assert_eq!(d(b"\\u4e2d").unwrap(), vec![0xE4, 0xB8, 0xAD]);
    }

    #[test]
    fn surrogate_pair() {
        // 😀 U+1F600 = 0xF0 0x9F 0x98 0x80
        assert_eq!(
            d(b"\\uD83D\\uDE00").unwrap(),
            vec![0xF0, 0x9F, 0x98, 0x80],
        );
    }

    #[test]
    fn lone_high_surrogate_fails() {
        assert_eq!(d(b"\\uD83D").unwrap_err(), qjd_err::QJD_DECODE_FAILED);
    }

    #[test]
    fn invalid_hex_in_unicode_fails() {
        assert_eq!(d(b"\\uZZZZ").unwrap_err(), qjd_err::QJD_DECODE_FAILED);
    }

    #[test]
    fn unknown_escape_fails() {
        assert_eq!(d(b"\\q").unwrap_err(), qjd_err::QJD_DECODE_FAILED);
    }

    #[test]
    fn dangling_backslash_fails() {
        assert_eq!(d(b"a\\").unwrap_err(), qjd_err::QJD_DECODE_FAILED);
    }
}
