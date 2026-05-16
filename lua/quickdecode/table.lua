-- Lazy table view + cjson-compatible encoder for quickdecode.
--
-- This module relies on the FFI cdef set up by `lua/quickdecode.lua`, so
-- callers must `require("quickdecode")` (transitively or directly) before
-- they require this module.

local ffi = require("ffi")
local C   = ffi.load("quickdecode")
local qd  = require("quickdecode")

-- Optional cjson bridge: reuse its sentinels when available so callers'
-- `v == cjson.null` comparisons keep working unchanged.
local has_cjson, cjson = pcall(require, "cjson")

local _M = {}

if has_cjson then
    _M.null            = cjson.null
    _M.empty_array_mt  = cjson.empty_array_mt
else
    _M.null            = setmetatable({}, { __tostring = function() return "null" end })
    _M.empty_array_mt  = { __jsontype = "array" }
end

-- Box scratch used for one-shot FFI returns. Reused across calls to avoid
-- per-call allocation; safe because the parent Doc / lazy view holds the
-- buffer alive and these are read-and-copy.
local err_box  = ffi.new("int[1]")
local i64_box  = ffi.new("int64_t[1]")
local f64_box  = ffi.new("double[1]")
local bool_box = ffi.new("int[1]")
local size_box = ffi.new("size_t[1]")
local type_box = ffi.new("int[1]")
local strp_box = ffi.new("const uint8_t*[1]")
local cur_box   = ffi.new("qjd_cursor[1]")
local child_box = ffi.new("qjd_cursor[1]")
local sz_a      = ffi.new("size_t[1]")
local sz_b      = ffi.new("size_t[1]")

local QJD_OK        = 0
local QJD_NOT_FOUND = 2
local T_NULL = 0
local T_BOOL = 1
local T_NUM  = 2
local T_STR  = 3
local T_ARR  = 4
local T_OBJ  = 5

local function check(rc)
    if rc == QJD_OK then return true end
    if rc == QJD_NOT_FOUND then return false end
    error("quickdecode: " .. ffi.string(C.qjd_strerror(rc)))
end

local LazyObject = {}
local LazyArray  = {}

-- Resolve a child cursor at `key` (object) and decode it into a Lua value.
-- Returns nil for missing keys (cjson semantics).
local function read_object_field(self, key)
    if type(key) ~= "string" then return nil end
    -- Use child_box so the lookup result does not alias self._cur (which is
    -- itself stored in cur_box's backing memory in the decode caller).
    local rc = C.qjd_cursor_field(self._cur, key, #key, child_box)
    if not check(rc) then return nil end
    local trc = C.qjd_cursor_typeof(child_box[0], "", 0, type_box)
    if not check(trc) then return nil end
    local t = type_box[0]
    if t == T_STR then
        local rrc = C.qjd_cursor_get_str(child_box[0], "", 0, strp_box, size_box)
        if not check(rrc) then return nil end
        return ffi.string(strp_box[0], size_box[0])
    elseif t == T_NUM then
        local rrc = C.qjd_cursor_get_f64(child_box[0], "", 0, f64_box)
        if not check(rrc) then return nil end
        return f64_box[0]
    elseif t == T_BOOL then
        local rrc = C.qjd_cursor_get_bool(child_box[0], "", 0, bool_box)
        if not check(rrc) then return nil end
        return bool_box[0] ~= 0
    elseif t == T_NULL then
        return _M.null
    end
    -- Container types are wrapped in a later task; for now return nil so
    -- this task's tests can pass on scalar-only fixtures.
    return nil
end

LazyObject.__index = read_object_field

function _M.decode(json_str)
    -- Reuse the existing qd.parse path to get a Doc with stable buffer hold.
    local doc = qd.parse(json_str)
    -- Open the root cursor into cur_box, then copy into a dedicated box owned
    -- by the view so that later child lookups (which reuse child_box) do not
    -- alias the root cursor's backing storage.
    local rc = C.qjd_open(doc._ptr, "", 0, cur_box)
    if not check(rc) then
        error("quickdecode: open root failed")
    end
    local root_box = ffi.new("qjd_cursor[1]")
    ffi.copy(root_box, cur_box, ffi.sizeof("qjd_cursor"))
    -- Determine root container kind (object/array) and wrap accordingly.
    -- Both have meaningful byte spans for encode.
    local trc = C.qjd_cursor_typeof(root_box[0], "", 0, type_box)
    check(trc)
    local rt = type_box[0]
    local brc = C.qjd_cursor_bytes(root_box[0], sz_a, sz_b)
    check(brc)
    local view = {
        _doc     = doc,
        _cur_box = root_box,   -- keep the box alive; _cur is a stable reference
        _cur     = root_box[0],
        _bs      = tonumber(sz_a[0]),
        _be      = tonumber(sz_b[0]),
    }
    if rt == T_OBJ then
        return setmetatable(view, LazyObject)
    elseif rt == T_ARR then
        return setmetatable(view, LazyArray)
    else
        error("quickdecode: top-level JSON value is not an object or array")
    end
end

return _M
