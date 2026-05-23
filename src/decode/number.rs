use crate::error::qjson_err;

pub(crate) fn parse_i64(bytes: &[u8], skip_validation: bool) -> Result<i64, qjson_err> {
    if !skip_validation {
        crate::validate::validate_number(bytes)?;
    }

    // When validation is skipped the caller guarantees the input is a
    // well-formed JSON number, but we still protect against empty input
    // so a misuse of the skip-flag cannot panic on bytes[0].
    if bytes.is_empty() {
        return Err(qjson_err::QJSON_INVALID_NUMBER);
    }

    // Fast guard: first byte must plausibly start a number, otherwise
    // the caller passed skip_validation=true on non-number input.
    if skip_validation && !matches!(bytes[0], b'-' | b'0'..=b'9') {
        return Err(qjson_err::QJSON_INVALID_NUMBER);
    }

    // After ABNF validation, integer-only inputs have no `.`/`e`/`E`.
    if memchr::memchr3(b'.', b'e', b'E', bytes).is_some() {
        return Err(qjson_err::QJSON_TYPE_MISMATCH);
    }
    let (neg, rest) = match bytes[0] {
        b'-' => (true, &bytes[1..]),
        _    => (false, bytes),
    };
    // ABNF guarantees `rest` is non-empty and digit-only here.
    let mut v: i64 = 0;
    for &c in rest {
        let d = (c - b'0') as i64;
        v = match v.checked_mul(10).and_then(|x| {
            if neg { x.checked_sub(d) } else { x.checked_add(d) }
        }) {
            Some(n) => n,
            None    => return Err(qjson_err::QJSON_OUT_OF_RANGE),
        };
    }
    Ok(v)
}

pub(crate) fn parse_f64(bytes: &[u8], skip_validation: bool) -> Result<f64, qjson_err> {
    if !skip_validation {
        crate::validate::validate_number(bytes)?;
    }

    // When validation is skipped, do a cheap precheck to avoid returning
    // a mode-dependent error code for non-number input.  The leading
    // byte must plausibly start a JSON number: `-`, `.`, or digit.
    if skip_validation
        && (bytes.is_empty() || !matches!(bytes[0], b'-' | b'.' | b'0'..=b'9'))
    {
        return Err(qjson_err::QJSON_INVALID_NUMBER);
    }

    let s = std::str::from_utf8(bytes).map_err(|_| qjson_err::QJSON_DECODE_FAILED)?;
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        Ok(_)                  => Err(qjson_err::QJSON_NUMBER_OUT_OF_RANGE),
        Err(_)                 => Err(qjson_err::QJSON_DECODE_FAILED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn i64_zero()       { assert_eq!(parse_i64(b"0", false),  Ok(0)); }
    #[test] fn i64_positive()   { assert_eq!(parse_i64(b"42", false), Ok(42)); }
    #[test] fn i64_negative()   { assert_eq!(parse_i64(b"-7", false), Ok(-7)); }
    #[test] fn i64_max() { assert_eq!(parse_i64(b"9223372036854775807", false), Ok(i64::MAX)); }
    #[test] fn i64_min() { assert_eq!(parse_i64(b"-9223372036854775808", false), Ok(i64::MIN)); }

    #[test]
    fn i64_overflow() {
        assert_eq!(parse_i64(b"9223372036854775808", false), Err(qjson_err::QJSON_OUT_OF_RANGE));
    }

    #[test]
    fn i64_rejects_decimal() {
        assert_eq!(parse_i64(b"1.5", false), Err(qjson_err::QJSON_TYPE_MISMATCH));
    }

    #[test]
    fn i64_rejects_exponent() {
        assert_eq!(parse_i64(b"1e5", false), Err(qjson_err::QJSON_TYPE_MISMATCH));
    }

    #[test]
    fn i64_rejects_empty() {
        assert_eq!(parse_i64(b"", false), Err(qjson_err::QJSON_INVALID_NUMBER));
    }

    #[test] fn f64_zero()    { assert_eq!(parse_f64(b"0.0", false).unwrap(),  0.0); }
    #[test] fn f64_inexact_decimal() { assert!((parse_f64(b"1.7", false).unwrap() - 1.7).abs() < 1e-12); }
    #[test] fn f64_negative(){ assert_eq!(parse_f64(b"-1.5", false).unwrap(), -1.5); }
    #[test] fn f64_exponent(){ assert_eq!(parse_f64(b"1e2", false).unwrap(),  100.0); }

    #[test]
    fn f64_rejects_garbage() {
        assert_eq!(parse_f64(b"hello", false), Err(qjson_err::QJSON_INVALID_NUMBER));
    }

    // ── skip_validation=true branch ────────────────────────────────

    #[test]
    fn i64_skip_validation_valid_input() {
        assert_eq!(parse_i64(b"42", true), Ok(42));
    }

    #[test]
    fn i64_skip_validation_empty_fails_gracefully() {
        assert_eq!(parse_i64(b"", true), Err(qjson_err::QJSON_INVALID_NUMBER));
    }

    #[test]
    fn i64_skip_validation_non_digit_returns_invalid_number() {
        assert_eq!(parse_i64(b"true", true), Err(qjson_err::QJSON_INVALID_NUMBER));
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn f64_skip_validation_valid_input() {
        assert_eq!(parse_f64(b"3.14", true).unwrap(), 3.14);
    }

    #[test]
    fn f64_skip_validation_garbage_fails_at_parse() {
        assert_eq!(parse_f64(b"hello", true), Err(qjson_err::QJSON_INVALID_NUMBER));
    }

    #[test]
    fn f64_skip_validation_empty_returns_invalid_number() {
        assert_eq!(parse_f64(b"", true), Err(qjson_err::QJSON_INVALID_NUMBER));
    }

    #[test]
    fn f64_skip_validation_non_number_returns_invalid_number() {
        assert_eq!(parse_f64(b"null", true), Err(qjson_err::QJSON_INVALID_NUMBER));
    }
}