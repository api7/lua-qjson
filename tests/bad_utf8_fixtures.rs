use qjson::doc::Document;
use qjson::error::qjson_err;
use qjson::options::{Options, QJSON_DEFAULT_MAX_DEPTH, QJSON_MODE_EAGER};

fn assert_rejects_utf8(path: &str) {
    let buf = std::fs::read(path)
        .unwrap_or_else(|e| panic!("read {}: {}", path, e));
    let opts = Options {
        mode:      QJSON_MODE_EAGER,
        max_depth: QJSON_DEFAULT_MAX_DEPTH,
    };
    let err = Document::parse_with_options(&buf, &opts)
        .err()
        .unwrap_or_else(|| panic!("{} parsed successfully but should be rejected", path));
    assert_eq!(
        err,
        qjson_err::QJSON_INVALID_UTF8,
        "{} rejected with wrong error code: {:?}",
        path, err
    );
}

#[test]
fn truncated_utf8_rejected() {
    assert_rejects_utf8("tests/fixtures/bad_utf8_truncated.json");
}

#[test]
fn overlong_utf8_rejected() {
    assert_rejects_utf8("tests/fixtures/bad_utf8_overlong.json");
}

#[test]
fn surrogate_utf8_rejected() {
    assert_rejects_utf8("tests/fixtures/bad_utf8_surrogate.json");
}
