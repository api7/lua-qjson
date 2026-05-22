#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
use proptest::prelude::*;

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
use qjson::__test_api::{validate_span_scalar, validate_span_avx2};

#[cfg(all(target_arch = "x86_64", feature = "avx2"))]
proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// AVX2 and scalar validators must return byte-identical Result values
    /// for every byte sequence: same Ok/Err, same error code on Err.
    #[test]
    fn scalar_avx2_match(input in prop::collection::vec(any::<u8>(), 0..256)) {
        if !std::is_x86_feature_detected!("avx2") {
            return Ok(());
        }
        let scalar = validate_span_scalar(&input);
        let avx2   = validate_span_avx2(&input);
        prop_assert_eq!(&scalar, &avx2,
            "validators disagree on input {:02X?}: scalar={:?} avx2={:?}",
            &input, scalar, avx2);
    }
}

#[cfg(not(all(target_arch = "x86_64", feature = "avx2")))]
#[test]
fn skip_string_validate() {}
