#![allow(non_camel_case_types)]

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum qjson_err {
    QJSON_OK                  =  0,
    QJSON_PARSE_ERROR         =  1,
    QJSON_NOT_FOUND           =  2,
    QJSON_TYPE_MISMATCH       =  3,
    QJSON_OUT_OF_RANGE        =  4,
    QJSON_DECODE_FAILED       =  5,
    QJSON_INVALID_PATH        =  6,
    QJSON_INVALID_ARG         =  7,
    QJSON_OOM                 =  8,
    QJSON_NESTING_TOO_DEEP    =  9,
    QJSON_TRAILING_CONTENT    = 10,
    QJSON_NUMBER_OUT_OF_RANGE = 11,
    QJSON_INVALID_NUMBER      = 12,
    QJSON_INVALID_STRING      = 13,
    QJSON_INVALID_UTF8        = 14,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum qjson_type {
    QJSON_T_NULL = 0,
    QJSON_T_BOOL = 1,
    QJSON_T_NUM  = 2,
    QJSON_T_STR  = 3,
    QJSON_T_ARR  = 4,
    QJSON_T_OBJ  = 5,
}

pub fn strerror(code: qjson_err) -> &'static str {
    match code {
        qjson_err::QJSON_OK                  => "ok",
        qjson_err::QJSON_PARSE_ERROR         => "JSON parse error",
        qjson_err::QJSON_NOT_FOUND           => "path not found",
        qjson_err::QJSON_TYPE_MISMATCH       => "type mismatch at path",
        qjson_err::QJSON_OUT_OF_RANGE        => "numeric out of range",
        qjson_err::QJSON_DECODE_FAILED       => "decode failed",
        qjson_err::QJSON_INVALID_PATH        => "invalid path syntax",
        qjson_err::QJSON_INVALID_ARG         => "invalid argument",
        qjson_err::QJSON_OOM                 => "out of memory",
        qjson_err::QJSON_NESTING_TOO_DEEP    => "nesting depth exceeds limit",
        qjson_err::QJSON_TRAILING_CONTENT    => "trailing content after root value",
        qjson_err::QJSON_NUMBER_OUT_OF_RANGE => "number out of representable range",
        qjson_err::QJSON_INVALID_NUMBER      => "invalid number format (RFC 8259)",
        qjson_err::QJSON_INVALID_STRING      => "invalid string content (unescaped control char)",
        qjson_err::QJSON_INVALID_UTF8        => "invalid UTF-8 in string",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strerror_covers_every_variant() {
        for code in [
            qjson_err::QJSON_OK, qjson_err::QJSON_PARSE_ERROR, qjson_err::QJSON_NOT_FOUND,
            qjson_err::QJSON_TYPE_MISMATCH, qjson_err::QJSON_OUT_OF_RANGE,
            qjson_err::QJSON_DECODE_FAILED, qjson_err::QJSON_INVALID_PATH,
            qjson_err::QJSON_INVALID_ARG, qjson_err::QJSON_OOM,
            qjson_err::QJSON_NESTING_TOO_DEEP, qjson_err::QJSON_TRAILING_CONTENT,
            qjson_err::QJSON_NUMBER_OUT_OF_RANGE, qjson_err::QJSON_INVALID_NUMBER,
            qjson_err::QJSON_INVALID_STRING, qjson_err::QJSON_INVALID_UTF8,
        ] {
            assert!(!strerror(code).is_empty());
        }
    }
}
