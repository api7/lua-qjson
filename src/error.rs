#![allow(non_camel_case_types)]

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum qjd_err {
    QJD_OK              = 0,
    QJD_PARSE_ERROR     = 1,
    QJD_NOT_FOUND       = 2,
    QJD_TYPE_MISMATCH   = 3,
    QJD_OUT_OF_RANGE    = 4,
    QJD_DECODE_FAILED   = 5,
    QJD_INVALID_PATH    = 6,
    QJD_INVALID_ARG     = 7,
    QJD_OOM             = 8,
    QJD_STALE_DOC       = 9,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum qjd_type {
    QJD_T_NULL = 0,
    QJD_T_BOOL = 1,
    QJD_T_NUM  = 2,
    QJD_T_STR  = 3,
    QJD_T_ARR  = 4,
    QJD_T_OBJ  = 5,
}

pub fn strerror(code: qjd_err) -> &'static str {
    match code {
        qjd_err::QJD_OK            => "ok",
        qjd_err::QJD_PARSE_ERROR   => "JSON parse error",
        qjd_err::QJD_NOT_FOUND     => "path not found",
        qjd_err::QJD_TYPE_MISMATCH => "type mismatch at path",
        qjd_err::QJD_OUT_OF_RANGE  => "numeric out of range",
        qjd_err::QJD_DECODE_FAILED => "decode failed",
        qjd_err::QJD_INVALID_PATH  => "invalid path syntax",
        qjd_err::QJD_INVALID_ARG   => "invalid argument",
        qjd_err::QJD_OOM           => "out of memory",
        qjd_err::QJD_STALE_DOC     => "stale document or cursor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strerror_covers_every_variant() {
        for code in [
            qjd_err::QJD_OK, qjd_err::QJD_PARSE_ERROR, qjd_err::QJD_NOT_FOUND,
            qjd_err::QJD_TYPE_MISMATCH, qjd_err::QJD_OUT_OF_RANGE,
            qjd_err::QJD_DECODE_FAILED, qjd_err::QJD_INVALID_PATH,
            qjd_err::QJD_INVALID_ARG, qjd_err::QJD_OOM, qjd_err::QJD_STALE_DOC,
        ] {
            assert!(!strerror(code).is_empty());
        }
    }
}
