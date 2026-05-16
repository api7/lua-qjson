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

-- Build a new lazy view for a child container cursor.
-- child_cursor is child_box[0] (a reference into child_box's backing memory).
-- We get the byte span via child_cursor, then ffi.copy from child_box into a
-- freshly-allocated per-view own_box so future child_box overwrites don't
-- corrupt this view's cursor.
local function wrap_child(parent_view, child_cursor)
    C.qjd_cursor_bytes(child_cursor, sz_a, sz_b)
    local own_box = ffi.new("qjd_cursor[1]")
    ffi.copy(own_box, child_box, ffi.sizeof("qjd_cursor"))
    return {
        _doc     = parent_view._doc,
        _cur_box = own_box,        -- keep cdata alive
        _cur     = own_box[0],     -- stable reference into own_box
        _bs      = tonumber(sz_a[0]),
        _be      = tonumber(sz_b[0]),
    }
end

-- Resolve a child cursor at `key` (object) and decode it into a Lua value.
-- Returns nil for missing keys (cjson semantics).
local function read_object_field(self, key)
    if type(key) ~= "string" then return nil end
    -- Use child_box so the lookup result does not alias self._cur (which is
    -- itself stored in root_box's backing memory in the decode caller).
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
    if t == T_OBJ then
        return setmetatable(wrap_child(self, child_box[0]), LazyObject)
    elseif t == T_ARR then
        return setmetatable(wrap_child(self, child_box[0]), LazyArray)
    end
    return nil
end

LazyObject.__index = read_object_field

-- Resolve a child cursor at integer index `key` (1-based) and decode it.
-- Returns nil for missing/out-of-range indices and non-integer keys.
local function read_array_index(self, key)
    if type(key) ~= "number" then return nil end
    -- 1-based external, 0-based internal
    local i = key - 1
    if i < 0 or i ~= math.floor(i) then return nil end
    local rc = C.qjd_cursor_index(self._cur, i, child_box)
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
    elseif t == T_OBJ then
        return setmetatable(wrap_child(self, child_box[0]), LazyObject)
    elseif t == T_ARR then
        return setmetatable(wrap_child(self, child_box[0]), LazyArray)
    end
    return nil
end

LazyArray.__index = read_array_index

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
    if not check(trc) then
        error("quickdecode: root typeof failed")
    end
    local rt = type_box[0]
    local brc = C.qjd_cursor_bytes(root_box[0], sz_a, sz_b)
    if not check(brc) then
        error("quickdecode: root byte-span failed")
    end
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
