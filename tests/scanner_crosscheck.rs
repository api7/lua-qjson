#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
use proptest::prelude::*;

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
use quickdecode::__test_api::{Scanner, ScalarScanner, Avx2Scanner};

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn scalar_avx2_bit_identical(input in valid_jsonish()) {
        if !std::is_x86_feature_detected!("avx2")
            || !std::is_x86_feature_detected!("pclmulqdq") {
            return Ok(());
        }
        let mut a = Vec::new();
        let mut b = Vec::new();
        let ra = ScalarScanner::scan(input.as_bytes(), &mut a);
        let rb = Avx2Scanner::scan(input.as_bytes(), &mut b);
        // Both paths run the same scan_emit_resume + validate_brackets
        // pipeline, so Result equality is required: same Ok/Err verdict
        // AND same error offset when Err.
        prop_assert_eq!(&ra, &rb, "scan results differ for {:?}", input);
        // Emit order is also expected to be identical; both phases push
        // through end-of-buffer before any potential Err is returned.
        prop_assert_eq!(&a, &b, "indices differ for {:?}", input);
    }
}

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
fn valid_jsonish() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just("{".to_string()),
            Just("}".to_string()),
            Just("[".to_string()),
            Just("]".to_string()),
            Just(",".to_string()),
            Just(":".to_string()),
            Just("\"a\"".to_string()),
            Just("\"\\\\\"".to_string()),
            Just("\"\\\"\"".to_string()),
            Just("\"\\u00e9\"".to_string()),
            Just("\"中文\"".to_string()),
            Just("123".to_string()),
            // Adversarial: long strings to ensure chunked path fires
            Just("\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"".to_string()),
            Just("\"\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\"".to_string()),
        ],
        0..200,
    ).prop_map(|v| v.concat())
}

#[cfg(not(all(target_arch = "x86_64", feature = "avx2")))]
#[test] fn skip() {}
