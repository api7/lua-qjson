#ifndef QJSON_H
#define QJSON_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
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
    QJSON_INVALID_UTF8        = 14
} qjson_err;

typedef struct {
    int    code;
    size_t offset;
} qjson_error;

typedef enum {
    QJSON_T_NULL = 0, QJSON_T_BOOL = 1, QJSON_T_NUM = 2,
    QJSON_T_STR  = 3, QJSON_T_ARR  = 4, QJSON_T_OBJ = 5
} qjson_type;

#define QJSON_MODE_EAGER          0u
#define QJSON_MODE_LAZY           1u
#define QJSON_DEFAULT_MAX_DEPTH   1024u
#define QJSON_MAX_MAX_DEPTH       4096u

typedef struct {
    uint32_t mode;       /* QJSON_MODE_EAGER (0) or QJSON_MODE_LAZY (1) */
    uint32_t max_depth;  /* 0 = default; values above QJSON_MAX_MAX_DEPTH are clamped */
} qjson_options;

typedef struct qjson_doc qjson_doc;

typedef struct {
    const qjson_doc* doc;
    uint32_t       idx_start;
    uint32_t       idx_end;
    uint32_t       _reserved0;
    uint32_t       _reserved1;
} qjson_cursor;

typedef struct {
    const qjson_doc* doc;
    uint32_t       idx_current;
    uint32_t       idx_end;
} qjson_iter;

const char* qjson_strerror(int code);
size_t qjson_format_error(int code, size_t offset, size_t extra,
                          const char* buf, size_t buf_len,
                          char* out, size_t out_len);
size_t qjson_doc_last_error_offset(const qjson_doc* doc);

qjson_doc* qjson_parse(const uint8_t* buf, size_t len, qjson_error* err_out);
qjson_doc* qjson_parse_ex(const uint8_t* buf, size_t len,
                      const qjson_options* opts, qjson_error* err_out);
void     qjson_free (qjson_doc* doc);

int qjson_get_str  (qjson_doc*, const char* path, size_t path_len,
                  const uint8_t** out_ptr, size_t* out_len);
int qjson_get_i64  (qjson_doc*, const char* path, size_t path_len, int64_t* out);
int qjson_get_u64  (qjson_doc*, const char* path, size_t path_len, uint64_t* out);
int qjson_get_f64  (qjson_doc*, const char* path, size_t path_len, double*  out);
int qjson_get_bool (qjson_doc*, const char* path, size_t path_len, int*     out);
int qjson_is_null  (qjson_doc*, const char* path, size_t path_len, int*     out);
int qjson_typeof   (qjson_doc*, const char* path, size_t path_len, int*     type_out);
int qjson_len      (qjson_doc*, const char* path, size_t path_len, size_t*  out);

int qjson_open            (qjson_doc*, const char* path, size_t path_len, qjson_cursor* out);
int qjson_cursor_open     (const qjson_cursor*, const char* path, size_t path_len, qjson_cursor* out);
int qjson_cursor_field    (const qjson_cursor*, const char* key,  size_t key_len, qjson_cursor* out);
int qjson_cursor_index    (const qjson_cursor*, size_t i, qjson_cursor* out);

int qjson_cursor_get_str  (const qjson_cursor*, const char* path, size_t path_len,
                         const uint8_t** out_ptr, size_t* out_len);
int qjson_cursor_get_i64  (const qjson_cursor*, const char* path, size_t path_len, int64_t* out);
int qjson_cursor_get_u64  (const qjson_cursor*, const char* path, size_t path_len, uint64_t* out);
int qjson_cursor_get_f64  (const qjson_cursor*, const char* path, size_t path_len, double*  out);
int qjson_cursor_get_bool (const qjson_cursor*, const char* path, size_t path_len, int*     out);
int qjson_cursor_typeof   (const qjson_cursor*, const char* path, size_t path_len, int*     out);
int qjson_cursor_len      (const qjson_cursor*, const char* path, size_t path_len, size_t*  out);
int qjson_cursor_bytes    (const qjson_cursor*, size_t* byte_start, size_t* byte_end);
int qjson_cursor_object_entry_at(const qjson_cursor*, size_t i,
                                const uint8_t** key_ptr, size_t* key_len,
                                qjson_cursor* value_out);
int qjson_iter_init(const qjson_cursor*, qjson_iter* it);
int qjson_iter_next(qjson_iter* it,
                    const uint8_t** key_ptr, size_t* key_len,
                    qjson_cursor* value_out);

#ifdef __cplusplus
}
#endif

#endif
