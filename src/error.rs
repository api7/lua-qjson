#![allow(non_camel_case_types)]

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum qjd_err {
    QJD_OK                  =  0,
    QJD_PARSE_ERROR         =  1,
    QJD_NOT_FOUND           =  2,
    QJD_TYPE_MISMATCH       =  3,
    QJD_OUT_OF_RANGE        =  4,
    QJD_DECODE_FAILED       =  5,
    QJD_INVALID_PATH        =  6,
    QJD_INVALID_ARG         =  7,
    QJD_OOM                 =  8,
    QJD_NESTING_TOO_DEEP    =  9,
    QJD_TRAILING_CONTENT    = 10,
    QJD_NUMBER_OUT_OF_RANGE = 11,
    QJD_INVALID_NUMBER      = 12,
    QJD_INVALID_STRING      = 13,
    QJD_INVALID_UTF8        = 14,
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
        qjd_err::QJD_OK                  => "ok",
        qjd_err::QJD_PARSE_ERROR         => "JSON parse error",
        qjd_err::QJD_NOT_FOUND           => "path not found",
        qjd_err::QJD_TYPE_MISMATCH       => "type mismatch at path",
        qjd_err::QJD_OUT_OF_RANGE        => "numeric out of range",
        qjd_err::QJD_DECODE_FAILED       => "decode failed",
        qjd_err::QJD_INVALID_PATH        => "invalid path syntax",
        qjd_err::QJD_INVALID_ARG         => "invalid argument",
        qjd_err::QJD_OOM                 => "out of memory",
        qjd_err::QJD_NESTING_TOO_DEEP    => "nesting depth exceeds limit",
        qjd_err::QJD_TRAILING_CONTENT    => "trailing content after root value",
        qjd_err::QJD_NUMBER_OUT_OF_RANGE => "number out of representable range",
        qjd_err::QJD_INVALID_NUMBER      => "invalid number format (RFC 8259)",
        qjd_err::QJD_INVALID_STRING      => "invalid string content (unescaped control char)",
        qjd_err::QJD_INVALID_UTF8        => "invalid UTF-8 in string",
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
            qjd_err::QJD_INVALID_ARG, qjd_err::QJD_OOM,
            qjd_err::QJD_NESTING_TOO_DEEP, qjd_err::QJD_TRAILING_CONTENT,
            qjd_err::QJD_NUMBER_OUT_OF_RANGE, qjd_err::QJD_INVALID_NUMBER,
            qjd_err::QJD_INVALID_STRING, qjd_err::QJD_INVALID_UTF8,
        ] {
            assert!(!strerror(code).is_empty());
        }
    }
}
