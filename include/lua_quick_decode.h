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
    QJD_OOM           = 8,
    /* Returned when a qjd_doc* (or a qjd_cursor whose doc field references one)
     * was produced by a decoder that has since been re-parsed, reset, or
     * destroyed. The handle is no longer usable; obtain a fresh one. */
    QJD_STALE_DOC     = 9
} qjd_err;

typedef enum {
    QJD_T_NULL = 0, QJD_T_BOOL = 1, QJD_T_NUM = 2,
    QJD_T_STR  = 3, QJD_T_ARR  = 4, QJD_T_OBJ = 5
} qjd_type;

typedef struct qjd_doc     qjd_doc;
typedef struct qjd_decoder qjd_decoder;

typedef struct {
    const qjd_doc* doc;
    uint32_t       idx_start;
    uint32_t       idx_end;
    uint32_t       _reserved0;
    uint32_t       _reserved1;
} qjd_cursor;

const char* qjd_strerror(int code);

/* One-shot parse: allocates a private decoder internally; freed by qjd_free. */
qjd_doc* qjd_parse(const uint8_t* buf, size_t len, int* err_out);
void     qjd_free (qjd_doc* doc);

/* Pooled / reusable decoder. Amortizes per-parse allocations of the
 * structural-offset buffer, the lazy-decode scratch buffer, and the skip
 * cache across many parses. Recommended for hot paths.
 *
 * After qjd_decoder_parse() is called on a decoder, all docs and cursors
 * produced by *prior* parses on that decoder become stale; operations on
 * them return QJD_STALE_DOC. After qjd_decoder_destroy(), all operations
 * return QJD_INVALID_ARG. All docs produced by a decoder must be freed
 * with qjd_free() before the decoder is freed with qjd_decoder_free(). */
qjd_decoder* qjd_decoder_new    (void);
void         qjd_decoder_free   (qjd_decoder*);
void         qjd_decoder_reset  (qjd_decoder*);
void         qjd_decoder_destroy(qjd_decoder*);
qjd_doc*     qjd_decoder_parse  (qjd_decoder*, const uint8_t* buf, size_t len,
                                 int* err_out);

int qjd_get_str  (qjd_doc*, const char* path, size_t path_len,
                  const uint8_t** out_ptr, size_t* out_len);
int qjd_get_i64  (qjd_doc*, const char* path, size_t path_len, int64_t* out);
int qjd_get_f64  (qjd_doc*, const char* path, size_t path_len, double*  out);
int qjd_get_bool (qjd_doc*, const char* path, size_t path_len, int*     out);
int qjd_is_null  (qjd_doc*, const char* path, size_t path_len, int*     out);
int qjd_typeof   (qjd_doc*, const char* path, size_t path_len, int*     type_out);
int qjd_len      (qjd_doc*, const char* path, size_t path_len, size_t*  out);

int qjd_open            (qjd_doc*, const char* path, size_t path_len, qjd_cursor* out);
int qjd_cursor_open     (const qjd_cursor*, const char* path, size_t path_len, qjd_cursor* out);
int qjd_cursor_field    (const qjd_cursor*, const char* key,  size_t key_len, qjd_cursor* out);
int qjd_cursor_index    (const qjd_cursor*, size_t i, qjd_cursor* out);

int qjd_cursor_get_str  (const qjd_cursor*, const char* path, size_t path_len,
                         const uint8_t** out_ptr, size_t* out_len);
int qjd_cursor_get_i64  (const qjd_cursor*, const char* path, size_t path_len, int64_t* out);
int qjd_cursor_get_f64  (const qjd_cursor*, const char* path, size_t path_len, double*  out);
int qjd_cursor_get_bool (const qjd_cursor*, const char* path, size_t path_len, int*     out);
int qjd_cursor_typeof   (const qjd_cursor*, const char* path, size_t path_len, int*     out);
int qjd_cursor_len      (const qjd_cursor*, const char* path, size_t path_len, size_t*  out);

#ifdef __cplusplus
}
#endif

#endif
