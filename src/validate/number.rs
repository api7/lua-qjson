//! Strict RFC 8259 §6 number-format validation.

use crate::error::qjd_err;

/// Returns Ok if `bytes` matches the JSON `number` grammar exactly.
/// Otherwise returns `QJD_INVALID_NUMBER`.
///
/// Out-of-range (i.e. f64 overflow) is NOT detected here; the f64 decode
/// step surfaces it as `QJD_NUMBER_OUT_OF_RANGE`.
pub(crate) fn validate_number(bytes: &[u8]) -> Result<(), qjd_err> {
    let mut i = 0;

    // optional minus
    if bytes.first() == Some(&b'-') { i += 1; }

    // int: "0" | (digit1-9 *digit)
    match bytes.get(i) {
        Some(&b'0') => { i += 1; }
        Some(&(b'1'..=b'9')) => {
            i += 1;
            while let Some(&c) = bytes.get(i) {
                if !c.is_ascii_digit() { break; }
                i += 1;
            }
        }
        _ => return Err(qjd_err::QJD_INVALID_NUMBER),
    }

    // optional frac: "." 1*digit
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        let frac_start = i;
        while let Some(&c) = bytes.get(i) {
            if !c.is_ascii_digit() { break; }
            i += 1;
        }
        if i == frac_start { return Err(qjd_err::QJD_INVALID_NUMBER); }
    }

    // optional exp: ("e"|"E") ["+"|"-"] 1*digit
    if matches!(bytes.get(i), Some(&b'e') | Some(&b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(&b'+') | Some(&b'-')) { i += 1; }
        let exp_start = i;
        while let Some(&c) = bytes.get(i) {
            if !c.is_ascii_digit() { break; }
            i += 1;
        }
        if i == exp_start { return Err(qjd_err::QJD_INVALID_NUMBER); }
    }

    if i != bytes.len() { return Err(qjd_err::QJD_INVALID_NUMBER); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &[u8]) { assert!(validate_number(s).is_ok(), "{:?}", std::str::from_utf8(s)); }
    fn bad(s: &[u8]) { assert!(validate_number(s).is_err(), "{:?}", std::str::from_utf8(s)); }

    #[test] fn zero_ok()                 { ok(b"0"); }
    #[test] fn neg_zero_ok()             { ok(b"-0"); }
    #[test] fn int_ok()                  { ok(b"123"); }
    #[test] fn neg_int_ok()              { ok(b"-456"); }
    #[test] fn frac_ok()                 { ok(b"3.14"); }
    #[test] fn neg_frac_ok()             { ok(b"-2.718"); }
    #[test] fn exp_lower_ok()            { ok(b"1e10"); }
    #[test] fn exp_upper_ok()            { ok(b"1E10"); }
    #[test] fn exp_plus_ok()             { ok(b"1e+10"); }
    #[test] fn exp_minus_ok()            { ok(b"1e-10"); }
    #[test] fn frac_exp_ok()             { ok(b"1.5e2"); }
    #[test] fn i64_max_str_ok()          { ok(b"9223372036854775807"); }

    #[test] fn leading_plus_bad()        { bad(b"+1"); }
    #[test] fn leading_zero_bad()        { bad(b"01"); }
    #[test] fn leading_zeros_bad()       { bad(b"00"); }
    #[test] fn bare_dot_bad()            { bad(b".5"); }
    #[test] fn trailing_dot_bad()        { bad(b"1."); }
    #[test] fn missing_frac_digits_bad() { bad(b"1.e5"); }
    #[test] fn hex_bad()                 { bad(b"0x1F"); }
    #[test] fn incomplete_exp_bad()      { bad(b"1e"); }
    #[test] fn incomplete_exp_sign_bad() { bad(b"1e+"); }
    #[test] fn nan_bad()                 { bad(b"NaN"); }
    #[test] fn inf_bad()                 { bad(b"Infinity"); }
    #[test] fn neg_inf_bad()             { bad(b"-Infinity"); }
    #[test] fn empty_bad()               { bad(b""); }
    #[test] fn lone_minus_bad()          { bad(b"-"); }
    #[test] fn double_dot_bad()          { bad(b"1..2"); }
}
