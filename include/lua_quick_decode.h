#ifndef LUA_QUICK_DECODE_H
#define LUA_QUICK_DECODE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    QJD_OK            = 0,
    QJD_PARSE_ERROR   = 1,
    QJD_NOT_FOUND     = 2,
    QJD_TYPE_MISMATCH = 3,
    QJD_OUT_OF_RANGE  = 4,
    QJD_DECODE_FAILED = 5,
    QJD_INVALID_PATH  = 6,
    QJD_INVALID_ARG   = 7,
    QJD_OOM           = 8
} qjd_err;

typedef enum {
    QJD_T_NULL = 0, QJD_T_BOOL = 1, QJD_T_NUM = 2,
    QJD_T_STR  = 3, QJD_T_ARR  = 4, QJD_T_OBJ = 5
} qjd_type;

const char* qjd_strerror(int code);

/* Forward declarations; full prototypes filled in Task 14. */

#ifdef __cplusplus
}
#endif

#endif
