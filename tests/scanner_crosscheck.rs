#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
use proptest::prelude::*;

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
use qjson::__test_api::{Scanner, ScalarScanner, Avx2Scanner};

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
        // Both scanners must agree on Ok vs Err and on the error offset.
        prop_assert_eq!(ra.is_ok(), rb.is_ok(), "scan result kind differs for {:?}", input);
        if let (Err(ae), Err(be)) = (&ra, &rb) {
            prop_assert_eq!(ae, be, "scan error offsets differ for {:?}", input);
        }
        // On success, indices must be identical. On error, the partial
        // emit may differ: the fused scalar (scan_and_validate) aborts at
        // the first bracket mismatch, while AVX2 emits all structural
        // chars before validate_brackets runs. Only compare on Ok.
        if ra.is_ok() {
            prop_assert_eq!(&a, &b, "indices differ for {:?}", input);
        }
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
#[test] fn skip_avx2() {}

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
#[test]
fn scalar_avx2_handles_continued_backslash_run_before_quote() {
    if !std::is_x86_feature_detected!("avx2")
        || !std::is_x86_feature_detected!("pclmulqdq") {
        return;
    }
    assert_continued_backslash_run_matches_scalar::<Avx2Scanner>();
}

// ── NEON cross-check ──────────────────────────────────────────────────────────

#[cfg(target_arch = "aarch64")]
use proptest::prelude::*;

#[cfg(target_arch = "aarch64")]
use qjson::__test_api::{Scanner, ScalarScanner, NeonScanner};

#[cfg(target_arch = "aarch64")]
proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn scalar_neon_bit_identical(input in neon_valid_jsonish()) {
        if !std::arch::is_aarch64_feature_detected!("aes") {
            return Ok(());
        }
        let mut a = Vec::new();
        let mut b = Vec::new();
        let ra = ScalarScanner::scan(input.as_bytes(), &mut a);
        let rb = NeonScanner::scan(input.as_bytes(), &mut b);
        // Both scanners must agree on Ok vs Err and on the error offset.
        prop_assert_eq!(ra.is_ok(), rb.is_ok(), "scan result kind differs for {:?}", input);
        if let (Err(ae), Err(be)) = (&ra, &rb) {
            prop_assert_eq!(ae, be, "scan error offsets differ for {:?}", input);
        }
        // On success, indices must be identical. On error, the partial
        // emit may differ between fused-scalar and two-pass NEON because
        // the fused path stops at the first bracket error while NEON emits
        // all structural chars before validating; only check on Ok.
        if ra.is_ok() {
            prop_assert_eq!(&a, &b, "indices differ for {:?}", input);
        }
    }
}

#[cfg(target_arch = "aarch64")]
fn neon_valid_jsonish() -> impl Strategy<Value = String> {
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
            Just("\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"".to_string()),
            Just("\"\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\"".to_string()),
        ],
        0..200,
    ).prop_map(|v| v.concat())
}

#[cfg(not(target_arch = "aarch64"))]
#[test] fn skip_neon() {}

#[cfg(target_arch = "aarch64")]
#[test]
fn scalar_neon_handles_continued_backslash_run_before_quote() {
    if !std::arch::is_aarch64_feature_detected!("aes") {
        return;
    }
    assert_continued_backslash_run_matches_scalar::<NeonScanner>();
}

#[cfg(any(all(target_arch = "x86_64", feature = "avx2"), target_arch = "aarch64"))]
fn assert_continued_backslash_run_matches_scalar<S: Scanner>() {
    let mut input = b"[\"".to_vec();
    while input.len() < 63 {
        input.push(b'a');
    }
    input.extend_from_slice(b"\\\\\\\"");
    while input.len() < 130 {
        input.push(b'b');
    }
    input.extend_from_slice(b"\"]");

    let mut a = Vec::new();
    let mut b = Vec::new();
    let ra = ScalarScanner::scan(&input, &mut a);
    let rb = S::scan(&input, &mut b);
    assert_eq!(ra, rb);
    assert_eq!(a, b);
}
