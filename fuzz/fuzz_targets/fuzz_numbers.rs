#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use qjson::doc::Document;
use qjson::ffi::*;
use qjson::options::{Options, QJSON_MODE_EAGER, QJSON_MODE_LAZY};
use serde_json::Value;
use std::os::raw::c_char;

const FUZZ_MAX_DEPTH: u32 = 128;

#[derive(Debug, Clone)]
struct NumberSpec {
    sign: Sign,
    integer: IntegerPart,
    fraction: Option<String>,
    exponent: Option<Exponent>,
}

#[derive(Debug, Clone)]
enum Sign {
    None,
    Minus,
    Plus,      // Invalid RFC 8259
    DoubleMinus, // Invalid
}

#[derive(Debug, Clone)]
enum IntegerPart {
    Zero,
    SingleDigit(u8), // 1-9
    MultiDigit(String),
    LeadingZero(String), // Invalid: 01, 00, 001
}

#[derive(Debug, Clone)]
struct Exponent {
    sign: ExpSign,
    value: String,
}

#[derive(Debug, Clone)]
enum ExpSign {
    None,
    Plus,
    Minus,
}

impl<'a> Arbitrary<'a> for NumberSpec {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        // Bias heavily toward boundary cases (70% of the time)
        let use_boundary = u.ratio(7, 10)?;

        if use_boundary {
            Self::arbitrary_boundary(u)
        } else {
            Self::arbitrary_random(u)
        }
    }
}

impl NumberSpec {
    fn arbitrary_boundary(u: &mut Unstructured) -> arbitrary::Result<Self> {
        let choice = u.int_in_range(0..=15)?;

        Ok(match choice {
            // Exponent boundaries
            0 => Self::from_str("1e308"),     // Near f64::MAX
            1 => Self::from_str("1e-308"),    // Near f64::MIN_POSITIVE
            2 => Self::from_str("1e309"),     // f64 overflow
            3 => Self::from_str("1e-324"),    // f64 underflow (subnormal)
            4 => Self::from_str("-1e308"),

            // Integer boundaries
            5 => Self::from_str("9223372036854775807"),     // i64::MAX
            6 => Self::from_str("9223372036854775808"),     // i64::MAX + 1
            7 => Self::from_str("-9223372036854775808"),    // i64::MIN
            8 => Self::from_str("-9223372036854775809"),    // i64::MIN - 1
            9 => Self::from_str("18446744073709551615"),    // u64::MAX
            10 => Self::from_str("18446744073709551616"),   // u64::MAX + 1

            // Leading zeros (invalid)
            11 => Self { sign: Sign::None, integer: IntegerPart::LeadingZero("00".into()), fraction: None, exponent: None },
            12 => Self { sign: Sign::None, integer: IntegerPart::LeadingZero("01".into()), fraction: None, exponent: None },
            13 => Self { sign: Sign::Minus, integer: IntegerPart::LeadingZero("00".into()), fraction: None, exponent: None },

            // Sign variations (invalid)
            14 => Self { sign: Sign::Plus, integer: IntegerPart::SingleDigit(1), fraction: None, exponent: None },
            15 => Self { sign: Sign::DoubleMinus, integer: IntegerPart::SingleDigit(1), fraction: None, exponent: None },

            _ => unreachable!(),
        })
    }

    fn arbitrary_random(u: &mut Unstructured) -> arbitrary::Result<Self> {
        let sign = match u.int_in_range(0..=10)? {
            0 => Sign::Plus,
            1 => Sign::DoubleMinus,
            2..=4 => Sign::Minus,
            _ => Sign::None,
        };

        let integer = match u.int_in_range(0..=10)? {
            0 => IntegerPart::LeadingZero(format!("0{}", u.int_in_range(0..=999)?)),
            1 => IntegerPart::LeadingZero("00".into()),
            2 => IntegerPart::Zero,
            3..=5 => IntegerPart::SingleDigit(u.int_in_range(1..=9)?),
            _ => {
                let len = u.int_in_range(1..=20)?;
                let first = u.int_in_range(1..=9)?;
                let mut s = first.to_string();
                for _ in 0..len {
                    s.push_str(&u.int_in_range(0..=9)?.to_string());
                }
                IntegerPart::MultiDigit(s)
            }
        };

        let fraction = if u.ratio(1, 3)? {
            let len = u.int_in_range(1..=20)?;
            let mut s = String::new();
            for _ in 0..len {
                s.push_str(&u.int_in_range(0..=9)?.to_string());
            }
            Some(s)
        } else {
            None
        };

        let exponent = if u.ratio(1, 3)? {
            let sign = match u.int_in_range(0..=2)? {
                0 => ExpSign::Plus,
                1 => ExpSign::Minus,
                _ => ExpSign::None,
            };
            let value = u.int_in_range(0..=350)?.to_string();
            Some(Exponent { sign, value })
        } else {
            None
        };

        Ok(Self { sign, integer, fraction, exponent })
    }

    fn from_str(s: &str) -> Self {
        let mut chars = s.chars().peekable();
        let sign = if chars.peek() == Some(&'-') {
            chars.next();
            Sign::Minus
        } else {
            Sign::None
        };

        let mut int_str = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                int_str.push(c);
                chars.next();
            } else {
                break;
            }
        }

        let integer = if int_str == "0" {
            IntegerPart::Zero
        } else if int_str.len() == 1 {
            IntegerPart::SingleDigit(int_str.as_bytes()[0] - b'0')
        } else {
            IntegerPart::MultiDigit(int_str)
        };

        let fraction = if chars.peek() == Some(&'.') {
            chars.next();
            let mut frac = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    frac.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            Some(frac)
        } else {
            None
        };

        let exponent = if matches!(chars.peek(), Some(&'e') | Some(&'E')) {
            chars.next();
            let exp_sign = match chars.peek() {
                Some(&'+') => { chars.next(); ExpSign::Plus },
                Some(&'-') => { chars.next(); ExpSign::Minus },
                _ => ExpSign::None,
            };
            let value: String = chars.collect();
            Some(Exponent { sign: exp_sign, value })
        } else {
            None
        };

        Self { sign, integer, fraction, exponent }
    }

    fn to_string(&self) -> String {
        let mut s = String::new();

        match &self.sign {
            Sign::None => {},
            Sign::Minus => s.push('-'),
            Sign::Plus => s.push('+'),
            Sign::DoubleMinus => s.push_str("--"),
        }

        match &self.integer {
            IntegerPart::Zero => s.push('0'),
            IntegerPart::SingleDigit(d) => s.push_str(&d.to_string()),
            IntegerPart::MultiDigit(v) => s.push_str(v),
            IntegerPart::LeadingZero(v) => s.push_str(v),
        }

        if let Some(frac) = &self.fraction {
            s.push('.');
            s.push_str(frac);
        }

        if let Some(exp) = &self.exponent {
            s.push('e');
            match exp.sign {
                ExpSign::None => {},
                ExpSign::Plus => s.push('+'),
                ExpSign::Minus => s.push('-'),
            }
            s.push_str(&exp.value);
        }

        s
    }

    fn is_rfc8259_valid(&self) -> bool {
        // Check sign
        if !matches!(self.sign, Sign::None | Sign::Minus) {
            return false;
        }

        // Check leading zeros
        match &self.integer {
            IntegerPart::LeadingZero(_) => return false,
            IntegerPart::MultiDigit(s) if s.starts_with('0') => return false,
            _ => {},
        }

        // Fraction must have at least one digit if present
        if let Some(frac) = &self.fraction {
            if frac.is_empty() {
                return false;
            }
        }

        // Exponent must have at least one digit if present
        if let Some(exp) = &self.exponent {
            if exp.value.is_empty() {
                return false;
            }
        }

        true
    }
}

#[derive(Debug)]
struct TestCase {
    number: NumberSpec,
    whitespace_prefix: &'static str,
    whitespace_suffix: &'static str,
}

impl<'a> Arbitrary<'a> for TestCase {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let number = NumberSpec::arbitrary(u)?;

        let ws_choices = ["", " ", "  ", "\t", "\n", "\r\n", " \t\n"];
        let whitespace_prefix = *u.choose(&ws_choices)?;
        let whitespace_suffix = *u.choose(&ws_choices)?;

        Ok(TestCase {
            number,
            whitespace_prefix,
            whitespace_suffix,
        })
    }
}

impl TestCase {
    fn to_json(&self) -> String {
        format!(
            "[{}{}{}]",
            self.whitespace_prefix,
            self.number.to_string(),
            self.whitespace_suffix
        )
    }
}

fuzz_target!(|test_case: TestCase| {
    let json = test_case.to_json();
    let data = json.as_bytes();

    // Test EAGER mode
    let opts_eager = Options { mode: QJSON_MODE_EAGER, max_depth: FUZZ_MAX_DEPTH };
    let qjson_eager_result = Document::parse_with_options(data, &opts_eager);
    let serde_result = serde_json::from_slice::<Value>(data);

    let is_rfc8259_valid = test_case.number.is_rfc8259_valid();

    // EAGER mode should reject RFC 8259 violations
    if !is_rfc8259_valid {
        if qjson_eager_result.is_ok() {
            panic!(
                "EAGER mode accepted invalid RFC 8259 number: {:?}\njson: {}",
                test_case.number, json
            );
        }
        // serde_json should also reject
        if serde_result.is_ok() {
            panic!(
                "serde_json accepted invalid RFC 8259 number (qjson correctly rejected): {:?}\njson: {}",
                test_case.number, json
            );
        }
        return; // Both correctly rejected, done
    }

    // For valid RFC 8259 numbers, check accept/reject consistency
    let qjson_eager_ok = qjson_eager_result.is_ok();
    let serde_ok = serde_result.is_ok();

    // Allow divergence when serde rejects for reasons qjson handles differently:
    // - "number out of range": serde's f64 model is stricter
    // - "invalid number": serde may reject some edge cases differently
    if qjson_eager_ok != serde_ok {
        if qjson_eager_ok && !serde_ok {
            let err_msg = serde_result.as_ref().err().unwrap().to_string();
            let allowed_divergence = err_msg.contains("number out of range")
                || err_msg.contains("invalid number");
            if !allowed_divergence {
                panic!(
                    "EAGER/serde mismatch: qjson_ok={} serde_ok={} serde_err={:?}\nnumber: {:?}\njson: {}",
                    qjson_eager_ok, serde_ok, err_msg, test_case.number, json
                );
            }
        } else {
            panic!(
                "EAGER/serde mismatch: qjson_ok={} serde_ok={}\nnumber: {:?}\njson: {}",
                qjson_eager_ok, serde_ok, test_case.number, json
            );
        }
    }

    // Test LAZY mode (only for valid RFC 8259)
    let opts_lazy = Options { mode: QJSON_MODE_LAZY, max_depth: FUZZ_MAX_DEPTH };
    let qjson_lazy_result = Document::parse_with_options(data, &opts_lazy);

    // LAZY mode should accept structurally valid JSON (brackets/quotes balanced)
    // but may defer number validation to access time
    let qjson_lazy_ok = qjson_lazy_result.is_ok();

    // If both qjson modes parsed successfully, test number extraction
    if qjson_eager_ok && qjson_lazy_ok {
        test_number_extraction(&json, &test_case.number);
    }

    // LAZY should be more permissive than EAGER for number-level issues
    // (but both will reject bracket/quote imbalance equally)
    if qjson_eager_ok && !qjson_lazy_ok {
        panic!(
            "EAGER succeeded but LAZY failed (LAZY should be more permissive): number: {:?}\njson: {}",
            test_case.number, json
        );
    }
});

fn test_number_extraction(json: &str, number_spec: &NumberSpec) {
    let mut err = qjson_error::default();
    let doc = unsafe {
        qjson_parse(
            json.as_ptr(),
            json.len(),
            &mut err
        )
    };

    if doc.is_null() {
        return; // Parse failed, already tested above
    }

    // Extract number at index 0 in the array
    let path = b"0";

    // Try i64 extraction
    let mut i64_val: i64 = 0;
    let i64_rc = unsafe {
        qjson_get_i64(
            doc,
            path.as_ptr() as *const c_char,
            path.len(),
            &mut i64_val
        )
    };

    // Try f64 extraction
    let mut f64_val: f64 = 0.0;
    let f64_rc = unsafe {
        qjson_get_f64(
            doc,
            path.as_ptr() as *const c_char,
            path.len(),
            &mut f64_val
        )
    };

    // If i64 succeeded, verify the value makes sense
    if i64_rc == 0 {
        // The extracted value should match when we parse with serde_json
        if let Ok(serde_val) = serde_json::from_str::<Value>(json) {
            if let Some(arr) = serde_val.as_array() {
                if let Some(num) = arr.get(0).and_then(|v| v.as_i64()) {
                    if i64_val != num {
                        panic!(
                            "i64 mismatch: qjson={} serde={} number: {:?}\njson: {}",
                            i64_val, num, number_spec, json
                        );
                    }
                }
            }
        }
    }

    // If f64 succeeded, verify it's finite and reasonable
    if f64_rc == 0 {
        if !f64_val.is_finite() {
            panic!(
                "f64 extraction returned non-finite value: {} for number: {:?}\njson: {}",
                f64_val, number_spec, json
            );
        }

        // Cross-check with serde_json
        if let Ok(serde_val) = serde_json::from_str::<Value>(json) {
            if let Some(arr) = serde_val.as_array() {
                if let Some(num) = arr.get(0).and_then(|v| v.as_f64()) {
                    // Skip infinite values from serde (e.g., 1e999)
                    if !num.is_finite() {
                        return;
                    }

                    // Allow small floating point differences (use OR to catch both
                    // small and large magnitude cases)
                    let abs_error = (f64_val - num).abs();
                    let rel_error = if num == 0.0 { abs_error } else { abs_error / num.abs() };

                    if rel_error > 1e-10 || abs_error > 1e-10 {
                        panic!(
                            "f64 mismatch: qjson={} serde={} rel_error={} abs_error={} number: {:?}\njson: {}",
                            f64_val, num, rel_error, abs_error, number_spec, json
                        );
                    }
                }
            }
        }
    }

    unsafe { qjson_free(doc) };
}
