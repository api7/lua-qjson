local ffi = require("ffi")

ffi.cdef[[
typedef struct qjd_doc qjd_doc;
typedef struct {
    const qjd_doc* doc;
    uint32_t idx_start, idx_end, _reserved0, _reserved1;
} qjd_cursor;

const char* qjd_strerror(int code);
qjd_doc* qjd_parse(const uint8_t* buf, size_t len, int* err_out);
void qjd_free(qjd_doc* doc);

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

local _M = {
    T_NULL = 0, T_BOOL = 1, T_NUM = 2,
    T_STR  = 3, T_ARR  = 4, T_OBJ = 5,
}

local Doc    = {}; Doc.__index    = Doc
local Cursor = {}; Cursor.__index = Cursor

local function check_err(rc)
    if rc == 0 then return true end
    if rc == NOT_FOUND then return false end
    error("quickdecode: " .. ffi.string(C.qjd_strerror(rc)))
end

function _M.parse(json_str)
    local ptr = C.qjd_parse(json_str, #json_str, err_box)
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
