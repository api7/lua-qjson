use crate::error::qjd_err;

pub(crate) fn parse_i64(bytes: &[u8]) -> Result<i64, qjd_err> {
    crate::validate::validate_number(bytes)?;
    // After ABNF validation, integer-only inputs have no `.`/`e`/`E`.
    if bytes.iter().any(|&b| b == b'.' || b == b'e' || b == b'E') {
        return Err(qjd_err::QJD_TYPE_MISMATCH);
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
            None    => return Err(qjd_err::QJD_OUT_OF_RANGE),
        };
    }
    Ok(v)
}

pub(crate) fn parse_f64(bytes: &[u8]) -> Result<f64, qjd_err> {
    crate::validate::validate_number(bytes)?;
    let s = std::str::from_utf8(bytes).map_err(|_| qjd_err::QJD_DECODE_FAILED)?;
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        Ok(_)                  => Err(qjd_err::QJD_NUMBER_OUT_OF_RANGE),
        Err(_)                 => Err(qjd_err::QJD_DECODE_FAILED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn i64_zero()       { assert_eq!(parse_i64(b"0"),  Ok(0)); }
    #[test] fn i64_positive()   { assert_eq!(parse_i64(b"42"), Ok(42)); }
    #[test] fn i64_negative()   { assert_eq!(parse_i64(b"-7"), Ok(-7)); }
    #[test] fn i64_max() { assert_eq!(parse_i64(b"9223372036854775807"), Ok(i64::MAX)); }
    #[test] fn i64_min() { assert_eq!(parse_i64(b"-9223372036854775808"), Ok(i64::MIN)); }

    #[test]
    fn i64_overflow() {
        assert_eq!(parse_i64(b"9223372036854775808"), Err(qjd_err::QJD_OUT_OF_RANGE));
    }

    #[test]
    fn i64_rejects_decimal() {
        assert_eq!(parse_i64(b"1.5"), Err(qjd_err::QJD_TYPE_MISMATCH));
    }

    #[test]
    fn i64_rejects_exponent() {
        assert_eq!(parse_i64(b"1e5"), Err(qjd_err::QJD_TYPE_MISMATCH));
    }

    #[test]
    fn i64_rejects_empty() {
        assert_eq!(parse_i64(b""), Err(qjd_err::QJD_INVALID_NUMBER));
    }

    #[test] fn f64_zero()    { assert_eq!(parse_f64(b"0.0").unwrap(),  0.0); }
    #[test] fn f64_inexact_decimal() { assert!((parse_f64(b"1.7").unwrap() - 1.7).abs() < 1e-12); }
    #[test] fn f64_negative(){ assert_eq!(parse_f64(b"-1.5").unwrap(), -1.5); }
    #[test] fn f64_exponent(){ assert_eq!(parse_f64(b"1e2").unwrap(),  100.0); }

    #[test]
    fn f64_rejects_garbage() {
        assert_eq!(parse_f64(b"hello"), Err(qjd_err::QJD_INVALID_NUMBER));
    }
}
