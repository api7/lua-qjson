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
        // Both scanners must agree on Ok vs Err (and on the error offset).
        prop_assert_eq!(&ra, &rb, "scan results differ for {:?}", input);
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

// ── FFI cross-check: qjson_cursor_field_bytes ─────────────────────────────────
//
// The above proptest already guarantees ScalarScanner and Avx2Scanner emit
// bit-identical indices for any input both accept. Since `qjson_cursor_field_bytes`
// reads only `doc.buf` and `doc.indices`, identical indices ⇒ identical FFI
// output. CI gates "cargo test --release" (default features, AVX2-on-x86)
// and "cargo test --release --no-default-features" (scalar) exercise both
// dispatch paths against the same proptest, so any backend drift surfaces
// as a failure on one but not the other.

use qjson::ffi::{
    qjson_cursor, qjson_cursor_field_bytes, qjson_free, qjson_open, qjson_parse,
};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn cursor_field_bytes_matches_source_span(
        kvs in proptest::collection::vec(
            ("[a-z]{1,4}", -100i64..100i64),
            1..6usize,
        ),
    ) {
        // Build a small valid JSON object {"k1":n1,"k2":n2,...}. Duplicate
        // keys collapse to the first occurrence in our expectation map below
        // (matches qjson_cursor_field_bytes semantics).
        let mut json = String::from("{");
        let mut expected: Vec<(String, String)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (i, (k, v)) in kvs.iter().enumerate() {
            if i > 0 { json.push(','); }
            json.push('"'); json.push_str(k); json.push_str("\":");
            let vs = v.to_string();
            json.push_str(&vs);
            if seen.insert(k.clone()) {
                expected.push((k.clone(), vs));
            }
        }
        json.push('}');

        unsafe {
            let mut err: std::os::raw::c_int = -1;
            let doc = qjson_parse(json.as_ptr(), json.len(), &mut err);
            prop_assert!(!doc.is_null());

            let mut root: qjson_cursor = std::mem::zeroed();
            let rc = qjson_open(doc, std::ptr::null(), 0, &mut root);
            prop_assert_eq!(rc, 0);

            for (k, want) in &expected {
                let mut child: qjson_cursor = std::mem::zeroed();
                let mut bs: usize = 0;
                let mut be: usize = 0;
                let rc = qjson_cursor_field_bytes(
                    &root,
                    k.as_ptr() as *const i8,
                    k.len(),
                    &mut child,
                    &mut bs,
                    &mut be,
                );
                prop_assert_eq!(rc, 0, "lookup of {:?} in {:?} failed rc={}", k, json, rc);
                prop_assert_eq!(&json.as_bytes()[bs..be], want.as_bytes());
            }

            qjson_free(doc);
        }
    }
}

// ── NEON cross-check ──────────────────────────────────────────────────────────

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
        // Both scanners must agree on Ok vs Err (and on the error offset).
        prop_assert_eq!(&ra, &rb, "scan results differ for {:?}", input);
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
