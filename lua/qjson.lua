local ffi = require("ffi")

local C = require("qjson.lib")

local err_box  = ffi.new("qjson_error[1]")
local i64_box  = ffi.new("int64_t[1]")
local u64_box  = ffi.new("uint64_t[1]")
local f64_box  = ffi.new("double[1]")
local bool_box = ffi.new("int[1]")
local size_box = ffi.new("size_t[1]")
local type_box = ffi.new("int[1]")
local strp_box = ffi.new("const uint8_t*[1]")
local cur_box  = ffi.new("qjson_cursor[1]")

local NOT_FOUND = 2
-- Error codes mirrored from include/qjson.h. Kept in sync manually;
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

local MODE_EAGER = 0
local MODE_LAZY  = 1
local DEFAULT_MAX_DEPTH = 1024
local MAX_MAX_DEPTH = 4096
local SIZE_MAX   = ffi.cast("size_t", -1)
local EXPECT_CONTAINER = ffi.cast("size_t", -2)

local function extra_or_none(extra)
    if extra == nil then return SIZE_MAX end
    return extra
end

local function format_error(code, offset, extra, buf)
    local buf_len = 0
    if type(buf) == "string" then
        buf_len = #buf
    else
        buf = nil
    end
    extra = extra_or_none(extra)
    local needed = tonumber(C.qjson_format_error(code, offset, extra, buf, buf_len, nil, 0))
    local out = ffi.new("char[?]", needed + 1)
    local written = tonumber(C.qjson_format_error(code, offset, extra, buf, buf_len, out, needed + 1))
    return ffi.string(out, written)
end

local function check_access(doc, rc, expected_type)
    if rc == 0 then return true end
    if rc == NOT_FOUND then return false end
    local offset = SIZE_MAX
    local source = nil
    if doc and doc._ptr ~= nil then
        offset = C.qjson_doc_last_error_offset(doc._ptr)
        source = doc._hold
    end
    local msg = format_error(rc, offset, expected_type, source)
    error("qjson: " .. msg)
end

local opts_box = ffi.new("qjson_options[1]")

local function effective_max_depth(raw)
    if raw == 0 then
        return DEFAULT_MAX_DEPTH
    end
    if raw > MAX_MAX_DEPTH then
        return MAX_MAX_DEPTH
    end
    return raw
end

local function parse_error_message(err, json_str, max_depth)
    local extra
    if err.code == ERR.NESTING_TOO_DEEP then
        extra = max_depth or DEFAULT_MAX_DEPTH
    end
    return format_error(err.code, err.offset, extra, json_str)
end

function _M.parse(json_str, opts)
    local ptr
    local effective_depth = DEFAULT_MAX_DEPTH
    if opts == nil then
        ptr = C.qjson_parse(json_str, #json_str, err_box)
    else
        if type(opts) ~= "table" then
            error("qjson.parse: opts must be a table")
        end
        local lazy = opts.lazy
        if lazy ~= nil and type(lazy) ~= "boolean" then
            error("qjson.parse: opts.lazy must be a boolean")
        end
        local max_depth = opts.max_depth or 0
        if type(max_depth) ~= "number" or max_depth < 0 or max_depth ~= math.floor(max_depth) then
            error("qjson.parse: opts.max_depth must be a non-negative integer")
        end
        effective_depth = effective_max_depth(max_depth)
        opts_box[0].mode      = lazy and MODE_LAZY or MODE_EAGER
        opts_box[0].max_depth = max_depth
        ptr = C.qjson_parse_ex(json_str, #json_str, opts_box, err_box)
    end
    if ptr == nil then
        error("qjson: " .. parse_error_message(err_box[0], json_str, effective_depth))
    end
    return setmetatable({
        _ptr  = ffi.gc(ptr, C.qjson_free),
        _hold = json_str,   -- strong ref keeps buffer alive
    }, Doc)
end

function Doc:get_str(path)
    local rc = C.qjson_get_str(self._ptr, path, #path, strp_box, size_box)
    if not check_access(self, rc, _M.T_STR) then return nil end
    return ffi.string(strp_box[0], size_box[0])
end

function Doc:get_i64(path)
    local rc = C.qjson_get_i64(self._ptr, path, #path, i64_box)
    if not check_access(self, rc, _M.T_NUM) then return nil end
    return i64_box[0]
end

function Doc:get_u64(path)
    local rc = C.qjson_get_u64(self._ptr, path, #path, u64_box)
    if not check_access(self, rc, _M.T_NUM) then return nil end
    return u64_box[0]
end

function Doc:get_f64(path)
    local rc = C.qjson_get_f64(self._ptr, path, #path, f64_box)
    if not check_access(self, rc, _M.T_NUM) then return nil end
    return f64_box[0]
end

function Doc:get_bool(path)
    local rc = C.qjson_get_bool(self._ptr, path, #path, bool_box)
    if not check_access(self, rc, _M.T_BOOL) then return nil end
    return bool_box[0] ~= 0
end

function Doc:is_null(path)
    local rc = C.qjson_is_null(self._ptr, path, #path, bool_box)
    if not check_access(self, rc) then return nil end
    return bool_box[0] ~= 0
end

function Doc:typeof(path)
    local rc = C.qjson_typeof(self._ptr, path, #path, type_box)
    if not check_access(self, rc) then return nil end
    return type_box[0]
end

function Doc:len(path)
    local rc = C.qjson_len(self._ptr, path, #path, size_box)
    if not check_access(self, rc, EXPECT_CONTAINER) then return nil end
    return tonumber(size_box[0])
end

function Doc:open(path)
    local rc = C.qjson_open(self._ptr, path, #path, cur_box)
    if not check_access(self, rc) then return nil end
    return setmetatable({ _cur = cur_box[0], _doc = self }, Cursor)
end

function Cursor:get_str(path)
    path = path or ""
    local rc = C.qjson_cursor_get_str(self._cur, path, #path, strp_box, size_box)
    if not check_access(self._doc, rc, _M.T_STR) then return nil end
    return ffi.string(strp_box[0], size_box[0])
end

function Cursor:get_i64(path)
    path = path or ""
    local rc = C.qjson_cursor_get_i64(self._cur, path, #path, i64_box)
    if not check_access(self._doc, rc, _M.T_NUM) then return nil end
    return i64_box[0]
end

function Cursor:get_u64(path)
    path = path or ""
    local rc = C.qjson_cursor_get_u64(self._cur, path, #path, u64_box)
    if not check_access(self._doc, rc, _M.T_NUM) then return nil end
    return u64_box[0]
end

function Cursor:get_f64(path)
    path = path or ""
    local rc = C.qjson_cursor_get_f64(self._cur, path, #path, f64_box)
    if not check_access(self._doc, rc, _M.T_NUM) then return nil end
    return f64_box[0]
end

function Cursor:get_bool(path)
    path = path or ""
    local rc = C.qjson_cursor_get_bool(self._cur, path, #path, bool_box)
    if not check_access(self._doc, rc, _M.T_BOOL) then return nil end
    return bool_box[0] ~= 0
end

function Cursor:typeof(path)
    path = path or ""
    local rc = C.qjson_cursor_typeof(self._cur, path, #path, type_box)
    if not check_access(self._doc, rc) then return nil end
    return type_box[0]
end

function Cursor:len(path)
    path = path or ""
    local rc = C.qjson_cursor_len(self._cur, path, #path, size_box)
    if not check_access(self._doc, rc, EXPECT_CONTAINER) then return nil end
    return tonumber(size_box[0])
end

function Cursor:open(path)
    local rc = C.qjson_cursor_open(self._cur, path, #path, cur_box)
    if not check_access(self._doc, rc) then return nil end
    return setmetatable({ _cur = cur_box[0], _doc = self._doc }, Cursor)
end

function Cursor:field(key)
    local rc = C.qjson_cursor_field(self._cur, key, #key, cur_box)
    if not check_access(self._doc, rc, _M.T_OBJ) then return nil end
    return setmetatable({ _cur = cur_box[0], _doc = self._doc }, Cursor)
end

function Cursor:index(i)
    local rc = C.qjson_cursor_index(self._cur, i, cur_box)
    if not check_access(self._doc, rc, _M.T_ARR) then return nil end
    return setmetatable({ _cur = cur_box[0], _doc = self._doc }, Cursor)
end

-- Lazy table API (cjson-shaped surface). See lua/qjson/table.lua.
local _lazy = require("qjson.table")
_M.decode         = _lazy.decode
_M.encode         = _lazy.encode
_M.encode_number_precision = _lazy.encode_number_precision
_M.encode_sparse_array = _lazy.encode_sparse_array
_M.materialize    = _lazy.materialize
_M.pairs          = _lazy.pairs
_M.ipairs         = _lazy.ipairs
_M.len            = _lazy.len
_M.null           = _lazy.null
_M.empty_array_mt = _lazy.empty_array_mt
_M._LazyObject    = _lazy._LazyObject
_M._LazyArray     = _lazy._LazyArray

return _M
