local ffi = require("ffi")

ffi.cdef[[
typedef struct qjd_doc qjd_doc;
typedef struct {
    const qjd_doc* doc;
    uint32_t idx_start, idx_end, _reserved0, _reserved1;
} qjd_cursor;

typedef struct {
    uint32_t mode;
    uint32_t max_depth;
} qjd_options;

const char* qjd_strerror(int code);
qjd_doc* qjd_parse   (const uint8_t* buf, size_t len, int* err_out);
qjd_doc* qjd_parse_ex(const uint8_t* buf, size_t len,
                       const qjd_options* opts, int* err_out);
void     qjd_free    (qjd_doc* doc);

int qjd_get_str (qjd_doc*, const char* path, size_t path_len, const uint8_t** p, size_t* n);
int qjd_get_i64 (qjd_doc*, const char* path, size_t path_len, int64_t* out);
int qjd_get_f64 (qjd_doc*, const char* path, size_t path_len, double*  out);
int qjd_get_bool(qjd_doc*, const char* path, size_t path_len, int*     out);
int qjd_is_null (qjd_doc*, const char* path, size_t path_len, int*     out);
int qjd_typeof  (qjd_doc*, const char* path, size_t path_len, int*     out);
int qjd_len     (qjd_doc*, const char* path, size_t path_len, size_t*  out);

int qjd_open        (qjd_doc*, const char* path, size_t path_len, qjd_cursor* out);
int qjd_cursor_open (const qjd_cursor*, const char* path, size_t path_len, qjd_cursor* out);
int qjd_cursor_field(const qjd_cursor*, const char* key,  size_t key_len, qjd_cursor* out);
int qjd_cursor_index(const qjd_cursor*, size_t i, qjd_cursor* out);

int qjd_cursor_get_str (const qjd_cursor*, const char*, size_t, const uint8_t**, size_t*);
int qjd_cursor_get_i64 (const qjd_cursor*, const char*, size_t, int64_t*);
int qjd_cursor_get_f64 (const qjd_cursor*, const char*, size_t, double*);
int qjd_cursor_get_bool(const qjd_cursor*, const char*, size_t, int*);
int qjd_cursor_typeof  (const qjd_cursor*, const char*, size_t, int*);
int qjd_cursor_len     (const qjd_cursor*, const char*, size_t, size_t*);
int qjd_cursor_bytes(const qjd_cursor*, size_t* byte_start, size_t* byte_end);
int qjd_cursor_object_entry_at(const qjd_cursor*, size_t i,
                                const uint8_t** key_ptr, size_t* key_len,
                                qjd_cursor* value_out);
]]

local C = ffi.load("quickdecode")

local err_box  = ffi.new("int[1]")
local i64_box  = ffi.new("int64_t[1]")
local f64_box  = ffi.new("double[1]")
local bool_box = ffi.new("int[1]")
local size_box = ffi.new("size_t[1]")
local type_box = ffi.new("int[1]")
local strp_box = ffi.new("const uint8_t*[1]")
local cur_box  = ffi.new("qjd_cursor[1]")

local NOT_FOUND = 2
-- Error codes mirrored from include/lua_quick_decode.h. Kept in sync manually;
-- src/error.rs has the authoritative numbering.
local ERR = {
    OK                  =  0,
    PARSE_ERROR         =  1,
    NOT_FOUND           =  2,
    TYPE_MISMATCH       =  3,
    OUT_OF_RANGE        =  4,
    DECODE_FAILED       =  5,
    INVALID_PATH        =  6,
    INVALID_ARG         =  7,
    OOM                 =  8,
    NESTING_TOO_DEEP    =  9,
    TRAILING_CONTENT    = 10,
    NUMBER_OUT_OF_RANGE = 11,
    INVALID_NUMBER      = 12,
    INVALID_STRING      = 13,
    INVALID_UTF8        = 14,
}

local _M = {
    T_NULL = 0, T_BOOL = 1, T_NUM = 2,
    T_STR  = 3, T_ARR  = 4, T_OBJ = 5,
}
_M.ERR = ERR

local Doc    = {}; Doc.__index    = Doc
local Cursor = {}; Cursor.__index = Cursor

local function check_err(rc)
    if rc == 0 then return true end
    if rc == NOT_FOUND then return false end
    error("quickdecode: " .. ffi.string(C.qjd_strerror(rc)))
end

local opts_box = ffi.new("qjd_options[1]")

local MODE_EAGER = 0
local MODE_LAZY  = 1

function _M.parse(json_str, opts)
    local ptr
    if opts == nil then
        ptr = C.qjd_parse(json_str, #json_str, err_box)
    else
        if type(opts) ~= "table" then
            error("quickdecode.parse: opts must be a table")
        end
        local lazy = opts.lazy
        if lazy ~= nil and type(lazy) ~= "boolean" then
            error("quickdecode.parse: opts.lazy must be a boolean")
        end
        local max_depth = opts.max_depth or 0
        if type(max_depth) ~= "number" or max_depth < 0 or max_depth ~= math.floor(max_depth) then
            error("quickdecode.parse: opts.max_depth must be a non-negative integer")
        end
        opts_box[0].mode      = lazy and MODE_LAZY or MODE_EAGER
        opts_box[0].max_depth = max_depth
        ptr = C.qjd_parse_ex(json_str, #json_str, opts_box, err_box)
    end
    if ptr == nil then
        error("quickdecode: " .. ffi.string(C.qjd_strerror(err_box[0])))
    end
    return setmetatable({
        _ptr  = ffi.gc(ptr, C.qjd_free),
        _hold = json_str,   -- strong ref keeps buffer alive
    }, Doc)
end

function Doc:get_str(path)
    local rc = C.qjd_get_str(self._ptr, path, #path, strp_box, size_box)
    if not check_err(rc) then return nil end
    return ffi.string(strp_box[0], size_box[0])
end

function Doc:get_i64(path)
    local rc = C.qjd_get_i64(self._ptr, path, #path, i64_box)
    if not check_err(rc) then return nil end
    return tonumber(i64_box[0])
end

function Doc:get_f64(path)
    local rc = C.qjd_get_f64(self._ptr, path, #path, f64_box)
    if not check_err(rc) then return nil end
    return f64_box[0]
end

function Doc:get_bool(path)
    local rc = C.qjd_get_bool(self._ptr, path, #path, bool_box)
    if not check_err(rc) then return nil end
    return bool_box[0] ~= 0
end

function Doc:is_null(path)
    local rc = C.qjd_is_null(self._ptr, path, #path, bool_box)
    if not check_err(rc) then return nil end
    return bool_box[0] ~= 0
end

function Doc:typeof(path)
    local rc = C.qjd_typeof(self._ptr, path, #path, type_box)
    if not check_err(rc) then return nil end
    return type_box[0]
end

function Doc:len(path)
    local rc = C.qjd_len(self._ptr, path, #path, size_box)
    if not check_err(rc) then return nil end
    return tonumber(size_box[0])
end

function Doc:open(path)
    local rc = C.qjd_open(self._ptr, path, #path, cur_box)
    if not check_err(rc) then return nil end
    return setmetatable({ _cur = cur_box[0], _doc = self }, Cursor)
end

function Cursor:get_str(path)
    path = path or ""
    local rc = C.qjd_cursor_get_str(self._cur, path, #path, strp_box, size_box)
    if not check_err(rc) then return nil end
    return ffi.string(strp_box[0], size_box[0])
end

function Cursor:get_i64(path)
    path = path or ""
    local rc = C.qjd_cursor_get_i64(self._cur, path, #path, i64_box)
    if not check_err(rc) then return nil end
    return tonumber(i64_box[0])
end

function Cursor:get_f64(path)
    path = path or ""
    local rc = C.qjd_cursor_get_f64(self._cur, path, #path, f64_box)
    if not check_err(rc) then return nil end
    return f64_box[0]
end

function Cursor:get_bool(path)
    path = path or ""
    local rc = C.qjd_cursor_get_bool(self._cur, path, #path, bool_box)
    if not check_err(rc) then return nil end
    return bool_box[0] ~= 0
end

function Cursor:typeof(path)
    path = path or ""
    local rc = C.qjd_cursor_typeof(self._cur, path, #path, type_box)
    if not check_err(rc) then return nil end
    return type_box[0]
end

function Cursor:len(path)
    path = path or ""
    local rc = C.qjd_cursor_len(self._cur, path, #path, size_box)
    if not check_err(rc) then return nil end
    return tonumber(size_box[0])
end

function Cursor:open(path)
    local rc = C.qjd_cursor_open(self._cur, path, #path, cur_box)
    if not check_err(rc) then return nil end
    return setmetatable({ _cur = cur_box[0], _doc = self._doc }, Cursor)
end

function Cursor:field(key)
    local rc = C.qjd_cursor_field(self._cur, key, #key, cur_box)
    if not check_err(rc) then return nil end
    return setmetatable({ _cur = cur_box[0], _doc = self._doc }, Cursor)
end

function Cursor:index(i)
    local rc = C.qjd_cursor_index(self._cur, i, cur_box)
    if not check_err(rc) then return nil end
    return setmetatable({ _cur = cur_box[0], _doc = self._doc }, Cursor)
end

-- Lazy table API (cjson-shaped surface). See lua/quickdecode/table.lua.
local _lazy = require("quickdecode.table")
_M.decode         = _lazy.decode
_M.encode         = _lazy.encode
_M.materialize    = _lazy.materialize
_M.pairs          = _lazy.pairs
_M.ipairs         = _lazy.ipairs
_M.len            = _lazy.len
_M.null           = _lazy.null
_M.empty_array_mt = _lazy.empty_array_mt
_M._LazyObject    = _lazy._LazyObject
_M._LazyArray     = _lazy._LazyArray

return _M
