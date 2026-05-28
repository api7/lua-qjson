-- Lazy table view + cjson-compatible encoder for qjson.

local ffi = require("ffi")
local C   = require("qjson.lib")
-- Defer the require to avoid a circular dependency when qjson.lua
-- re-exports this module.  By the time _M.decode is called, qjson
-- is already registered in package.loaded.
local function get_qjson()
    return require("qjson")
end

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

-- Weak side-table for container type hints, avoiding collision with
-- user-visible keys.  Maps materialized table → "object" | "array".
local TABLE_TYPE_HINT = setmetatable({}, { __mode = "k" })

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
local cur_box   = ffi.new("qjson_cursor[1]")
local child_box = ffi.new("qjson_cursor[1]")
local sz_a      = ffi.new("size_t[1]")
local sz_b      = ffi.new("size_t[1]")

local QJSON_OK        = 0
local QJSON_NOT_FOUND = 2
local T_NULL = 0
local T_BOOL = 1
local T_NUM  = 2
local T_STR  = 3
local T_ARR  = 4
local T_OBJ  = 5

local function check(rc)
    if rc == QJSON_OK then return true end
    if rc == QJSON_NOT_FOUND then return false end
    error("qjson: " .. ffi.string(C.qjson_strerror(rc)))
end

local LazyObject = {}
local LazyArray  = {}

-- Sentinel tables used as raw keys to avoid collision with user JSON keys.
-- Using tables (not strings) ensures no user JSON key can match these.
local ORDER_KEYS = {}    -- stores ordered key list after materialization
local ORDER_VALUES = {}  -- stores key->value map after materialization
local CHILD_CACHE = {}   -- stores key->child proxy cache before materialization

local function get_child_cache(view)
    local cache = rawget(view, CHILD_CACHE)
    if not cache then
        cache = {}
        rawset(view, CHILD_CACHE, cache)
    end
    return cache
end

local function cached_child(view, key)
    local cache = rawget(view, CHILD_CACHE)
    if cache then return cache[key] end
    return nil
end

-- Build a new lazy view for a child container cursor.
-- src_box is an FFI cdata `qjson_cursor[1]`; src_box[0] is the cursor whose
-- data we copy into a fresh per-view allocation so the new view's _cur
-- survives later overwrites of src_box.
local function wrap_child(parent_view, src_box)
    C.qjson_cursor_bytes(src_box[0], sz_a, sz_b)
    local own_box = ffi.new("qjson_cursor[1]")
    ffi.copy(own_box, src_box, ffi.sizeof("qjson_cursor"))
    return {
        _parent  = parent_view,
        _dirty   = false,
        _doc     = parent_view._doc,
        _cur_box = own_box,        -- keep cdata alive
        _cur     = own_box[0],     -- stable reference into own_box
        _bs      = tonumber(sz_a[0]),
        _be      = tonumber(sz_b[0]),
    }
end

-- Decode the value at src_box[0] into a Lua value.
-- src_box is a `qjson_cursor[1]`; for container types, a new view is created
-- via wrap_child so the caller's box can be freely reused afterwards.
local function decode_cursor(parent_view, src_box)
    local trc = C.qjson_cursor_typeof(src_box[0], "", 0, type_box)
    if not check(trc) then return nil end
    local t = type_box[0]
    if t == T_STR then
        local rrc = C.qjson_cursor_get_str(src_box[0], "", 0, strp_box, size_box)
        if not check(rrc) then return nil end
        return ffi.string(strp_box[0], size_box[0])
    elseif t == T_NUM then
        local rrc = C.qjson_cursor_get_f64(src_box[0], "", 0, f64_box)
        if not check(rrc) then return nil end
        return f64_box[0]
    elseif t == T_BOOL then
        local rrc = C.qjson_cursor_get_bool(src_box[0], "", 0, bool_box)
        if not check(rrc) then return nil end
        return bool_box[0] ~= 0
    elseif t == T_NULL then
        return _M.null
    elseif t == T_OBJ then
        return setmetatable(wrap_child(parent_view, src_box), LazyObject)
    elseif t == T_ARR then
        return setmetatable(wrap_child(parent_view, src_box), LazyArray)
    end
    return nil
end

-- Resolve a child cursor at `key` (object) and decode it into a Lua value.
-- Returns nil for missing keys (cjson semantics).
-- Container results (lazy proxies) are cached behind a sentinel key so that
-- subsequent accesses return the same Lua table object without occupying the
-- user field name and bypassing LazyObject.__newindex.
local function read_object_field(self, key)
    if type(key) ~= "string" then return nil end

    -- Check if materialized (using sentinel key to avoid collision with user JSON keys)
    local values = rawget(self, ORDER_VALUES)
    if values then
        return values[key]
    end

    local cached = cached_child(self, key)
    if cached ~= nil then return cached end

    -- Use child_box so the lookup result does not alias self._cur (which is
    -- itself stored in root_box's backing memory in the decode caller).
    local rc = C.qjson_cursor_field(self._cur, key, #key, child_box)
    if not check(rc) then return nil end
    local v = decode_cursor(self, child_box)
    -- Cache containers so identity is stable and materialization sticks.
    if type(v) == "table" then get_child_cache(self)[key] = v end
    return v
end

LazyObject.__index = read_object_field

-- Resolve a child cursor at integer index `key` (1-based) and decode it.
-- Returns nil for missing/out-of-range indices and non-integer keys.
-- Container results are rawset-cached for the same identity-stability reason
-- as read_object_field.
local function read_array_index(self, key)
    if type(key) ~= "number" then return nil end
    -- 1-based external, 0-based internal
    local i = key - 1
    if i < 0 or i ~= math.floor(i) then return nil end
    local rc = C.qjson_cursor_index(self._cur, i, child_box)
    if not check(rc) then return nil end
    local v = decode_cursor(self, child_box)
    -- Cache containers so identity is stable and materialization sticks.
    if type(v) == "table" then rawset(self, key, v) end
    return v
end

LazyArray.__index = read_array_index

-- Iterator function for lazy_object_iter: advances through object entries by
-- integer index, returning key/value pairs in source order.
local function lazy_object_iter(state, _prev_key)
    local i = state.i
    state.i = i + 1
    local rc = C.qjson_cursor_object_entry_at(
        state.view._cur, i, strp_box, size_box, child_box
    )
    if rc == QJSON_NOT_FOUND then return nil end
    check(rc)
    local k = ffi.string(strp_box[0], size_box[0])
    local v = decode_cursor(state.view, child_box)
    return k, v
end

function LazyObject.__pairs(t)
    local keys = rawget(t, ORDER_KEYS)
    if keys then
        local values = rawget(t, ORDER_VALUES)
        local i = 0
        return function()
            i = i + 1
            local k = keys[i]
            if k then return k, values[k] end
        end
    end
    return lazy_object_iter, { view = t, i = 0 }, nil
end

local function lazy_array_iter(state, _prev_i)
    local i = state.i
    local rc = C.qjson_cursor_index(state.view._cur, i, child_box)
    if rc == QJSON_NOT_FOUND then return nil end
    check(rc)
    state.i = i + 1
    local v = decode_cursor(state.view, child_box)
    return i + 1, v
end

function LazyArray.__ipairs(t)
    return lazy_array_iter, { view = t, i = 0 }, 0
end

function _M.ipairs(t)
    local mt = getmetatable(t)
    if mt == LazyArray then
        return LazyArray.__ipairs(t)
    end
    return ipairs(t)
end

function _M.pairs(t)
    local mt = getmetatable(t)
    if mt == LazyObject then
        return LazyObject.__pairs(t)
    elseif mt == LazyArray then
        return _M.ipairs(t)
    end
    return pairs(t)
end

local function lazy_len(self)
    if getmetatable(self) == LazyObject then
        local keys = rawget(self, ORDER_KEYS)
        if keys then return #keys end
    end
    local rc = C.qjson_cursor_len(self._cur, "", 0, size_box)
    check(rc)
    return tonumber(size_box[0])
end

LazyObject.__len = lazy_len
LazyArray.__len  = lazy_len

-- Public fallback for `#t` on a lazy proxy. Vanilla LuaJIT 5.1 does not invoke
-- __len on tables (only userdata) unless built with LUAJIT_ENABLE_LUA52COMPAT
-- (OpenResty's default). Callers running on a non-compat LuaJIT must use
-- qjson.len(t) — same role qjson.pairs / qjson.ipairs play for __pairs / __ipairs.
function _M.len(t)
    local mt = getmetatable(t)
    if mt == LazyObject or mt == LazyArray then
        return lazy_len(t)
    end
    return #t
end

-- Materialize all key/value pairs from a LazyObject view into a plain list.
-- Returns a sequence of {k, v} pairs. The view is not mutated here; mutation
-- happens in __newindex after the walk completes successfully.
local function materialize_object_contents(view)
    local i = 0
    local pairs_out = {}
    while true do
        local rc = C.qjson_cursor_object_entry_at(view._cur, i, strp_box, size_box, child_box)
        if rc == QJSON_NOT_FOUND then break end
        check(rc)
        local k = ffi.string(strp_box[0], size_box[0])
        local v = decode_cursor(view, child_box)
        pairs_out[#pairs_out+1] = {k, v}
        i = i + 1
    end
    return pairs_out
end

-- Materialize all elements from a LazyArray view into a plain sequence.
-- Returns a sequence indexed 1..n. The view is not mutated here.
local function materialize_array_contents(view)
    local i = 0
    local out = {}
    while true do
        local rc = C.qjson_cursor_index(view._cur, i, child_box)
        if rc == QJSON_NOT_FOUND then break end
        check(rc)
        out[i + 1] = decode_cursor(view, child_box)
        i = i + 1
    end
    return out
end

-- The set of keys reserved by the lazy view bookkeeping; user-supplied JSON
-- keys with these names would collide (minor, deferred). Centralized so
-- __newindex (cache snapshotting before materialization) and
-- encode_lazy_object_walking (skipping internals while encoding a dirty
-- proxy) share one source of truth.
local INTERNAL_KEYS = {
    _doc = true, _cur_box = true, _cur = true, _bs = true, _be = true,
    _parent = true, _dirty = true,
    -- Note: ORDER_KEYS and ORDER_VALUES are sentinel tables, not strings,
    -- so they don't need to be in this string-keyed exclusion set.
}

-- On first write, walk all cursor entries into ORDER_KEYS (ordered list) and
-- ORDER_VALUES (key→value map) stored under sentinel table keys. The LazyObject
-- metatable is kept alive so __index continues to route reads through
-- ORDER_VALUES. Any CHILD_CACHE entries (pre-materialization child proxies) are
-- promoted into ORDER_VALUES so proxy identity is preserved across materialization.
LazyObject.__newindex = function(t, k, v)
    -- Mark dirty from this view up to the root.
    local cur = t
    while cur do
        local mt = getmetatable(cur)
        if mt ~= LazyObject and mt ~= LazyArray then break end
        rawset(cur, "_dirty", true)
        cur = rawget(cur, "_parent")
    end

    local keys = rawget(t, ORDER_KEYS)
    if not keys then
        -- First modification: materialize key order
        keys = {}
        local values = {}
        local i = 0
        while true do
            local rc = C.qjson_cursor_object_entry_at(t._cur, i, strp_box, size_box, child_box)
            if rc == QJSON_NOT_FOUND then break end
            check(rc)
            local key = ffi.string(strp_box[0], size_box[0])
            keys[#keys + 1] = key
            -- Prefer cached child proxy over fresh decode
            local cached = cached_child(t, key)
            if cached ~= nil and not INTERNAL_KEYS[key] then
                values[key] = cached
            else
                values[key] = decode_cursor(t, child_box)
            end
            i = i + 1
        end
        rawset(t, ORDER_KEYS, keys)
        rawset(t, ORDER_VALUES, values)
    end

    local values = rawget(t, ORDER_VALUES)
    if v == nil then
        -- Delete: remove from _keys
        for i, key in ipairs(keys) do
            if key == k then
                table.remove(keys, i)
                break
            end
        end
        values[k] = nil
    elseif values[k] == nil then
        -- New key: append to _keys
        keys[#keys + 1] = k
        values[k] = v
    else
        -- Existing key: just update value
        values[k] = v
    end
end

-- On first write, walk all existing elements into a plain sequence,
-- switch to empty_array_mt (no lazy machinery), then apply the assignment.
-- Existing rawget-cached entries are preserved so callers' references remain valid.
LazyArray.__newindex = function(t, k, v)
    -- Mark dirty from this view up to the root.
    local cur = t
    while cur do
        local mt = getmetatable(cur)
        if mt ~= LazyObject and mt ~= LazyArray then break end
        rawset(cur, "_dirty", true)
        cur = rawget(cur, "_parent")
    end
    local contents = materialize_array_contents(t)
    -- Snapshot integer-key cache BEFORE nilling internals.
    local cache = {}
    local ck, cv = next(t)
    while ck ~= nil do
        if type(ck) == "number" then
            cache[ck] = cv
        end
        ck, cv = next(t, ck)
    end
    rawset(t, "_parent",  nil)
    rawset(t, "_dirty",   nil)
    rawset(t, "_doc",     nil)
    rawset(t, "_cur_box", nil)
    rawset(t, "_cur",     nil)
    rawset(t, "_bs",      nil)
    rawset(t, "_be",      nil)
    setmetatable(t, _M.empty_array_mt)
    TABLE_TYPE_HINT[t] = "array"
    for i, x in ipairs(contents) do
        rawset(t, i, cache[i] or x)
    end
    rawset(t, k, v)
end

function _M.decode(json_str)
    -- Reuse the existing qjson.parse path to get a Doc with stable buffer hold.
    local doc = get_qjson().parse(json_str)
    -- Open the root cursor into cur_box, then copy into a dedicated box owned
    -- by the view so that later child lookups (which reuse child_box) do not
    -- alias the root cursor's backing storage.
    local rc = C.qjson_open(doc._ptr, "", 0, cur_box)
    if not check(rc) then
        error("qjson: open root failed")
    end
    local root_box = ffi.new("qjson_cursor[1]")
    ffi.copy(root_box, cur_box, ffi.sizeof("qjson_cursor"))
    -- Determine root container kind (object/array) and wrap accordingly.
    -- Both have meaningful byte spans for encode.
    local trc = C.qjson_cursor_typeof(root_box[0], "", 0, type_box)
    if not check(trc) then
        error("qjson: root typeof failed")
    end
    local rt = type_box[0]
    local brc = C.qjson_cursor_bytes(root_box[0], sz_a, sz_b)
    if not check(brc) then
        error("qjson: root byte-span failed")
    end
    local view = {
        _dirty   = false,
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
        error("qjson: top-level JSON value is not an object or array")
    end
end

local function materialize(v)
    local mt = (type(v) == "table") and getmetatable(v) or nil
    if mt == LazyObject then
        local out = {}
        local keys = rawget(v, ORDER_KEYS)
        if keys then
            -- Already materialized: use ORDER_KEYS order and ORDER_VALUES
            local values = rawget(v, ORDER_VALUES)
            for _, k in ipairs(keys) do
                local val = values[k]
                if val ~= nil then
                    out[k] = materialize(val)
                end
            end
        else
            -- Not yet materialized: use cursor-based walk
            for _, kv in ipairs(materialize_object_contents(v)) do
                local child = cached_child(v, kv[1]) or kv[2]
                out[kv[1]] = materialize(child)
            end
        end
        return out
    elseif mt == LazyArray then
        local raw = materialize_array_contents(v)
        local out = {}
        for i, x in ipairs(raw) do
            out[i] = materialize(x)
        end
        if #out == 0 then
            setmetatable(out, _M.empty_array_mt)
        end
        return out
    end
    return v
end

_M.materialize = materialize

local string_byte = string.byte
local string_format = string.format

-- Escape lookup table: byte value → escape sequence string (or nil if safe).
local ESCAPES = {
    [0x22] = '\\"',
    [0x5C] = '\\\\',
    [0x0A] = '\\n',
    [0x0D] = '\\r',
    [0x09] = '\\t',
    [0x08] = '\\b',
    [0x0C] = '\\f',
}

-- JSON string escaper with bulk-copy fast path.
-- Scans for bytes that need escaping; copies clean segments via s:sub.
-- For strings with no escapes, returns '"' .. s .. '"' with zero table allocations.
local function encode_string(s)
    local n = #s
    local last, i = 1, 1
    local out = nil   -- lazily create table only when escapes found
    while i <= n do
        local b = string_byte(s, i)
        local esc = ESCAPES[b]
        if esc or b < 0x20 then
            if not out then out = {'"'} end
            if i > last then out[#out + 1] = s:sub(last, i - 1) end
            if esc then
                out[#out + 1] = esc
            else
                out[#out + 1] = string_format('\\u%04x', b)
            end
            last = i + 1
        end
        i = i + 1
    end
    if not out then return '"' .. s .. '"' end
    if last <= n then out[#out + 1] = s:sub(last, n) end
    out[#out + 1] = '"'
    return table.concat(out)
end

local function encode_number(n)
    if n ~= n or n == math.huge or n == -math.huge then
        error("qjson.encode: cannot encode non-finite number")
    end
    if n == math.floor(n) and math.abs(n) < 1e15 then
        return string_format("%d", n)
    end
    return string_format("%.14g", n)
end

-- Forward declaration so encode_lazy_object_walking, encode_lazy_array_walking,
-- and encode_array/encode_object can reference encode before its definition is
-- complete (Lua resolves upvalues at call time, but the slot must be declared first).
local encode

-- Walk a dirty LazyObject and emit JSON, preferring cached children (which
-- may be materialized) over freshly resolved cursors. Non-cached children
-- emit through a fresh proxy and naturally fast-path their unmodified subtree.
local function encode_lazy_object_walking(t)
    local keys = rawget(t, ORDER_KEYS)
    if keys then
        -- Materialized: use ORDER_KEYS order
        local values = rawget(t, ORDER_VALUES)
        local parts = {}
        for _, k in ipairs(keys) do
            local v = values[k]
            if v ~= nil then
                parts[#parts + 1] = encode_string(k) .. ":" .. encode(v)
            end
        end
        return "{" .. table.concat(parts, ",") .. "}"
    end

    -- Original cursor-based walking (for dirty-via-child-only case)
    local parts = {}
    local i = 0
    while true do
        local rc = C.qjson_cursor_object_entry_at(t._cur, i, strp_box, size_box, child_box)
        if rc == QJSON_NOT_FOUND then break end
        check(rc)
        local k = ffi.string(strp_box[0], size_box[0])
        local cached = cached_child(t, k)
        local v
        if cached ~= nil and not INTERNAL_KEYS[k] then
            v = cached
        else
            v = decode_cursor(t, child_box)
        end
        parts[#parts + 1] = encode_string(k) .. ":" .. encode(v)
        i = i + 1
    end
    return "{" .. table.concat(parts, ",") .. "}"
end

local function encode_lazy_array_walking(t)
    local parts = {}
    local rc = C.qjson_cursor_len(t._cur, "", 0, size_box)
    check(rc)
    local n = tonumber(size_box[0])
    for i = 0, n - 1 do
        local irc = C.qjson_cursor_index(t._cur, i, child_box)
        check(irc)
        local cached = rawget(t, i + 1)
        local v
        if cached ~= nil then
            v = cached
        else
            v = decode_cursor(t, child_box)
        end
        parts[#parts + 1] = encode(v)
    end
    return "[" .. table.concat(parts, ",") .. "]"
end

local function encode_proxy(t)
    if not t._dirty then
        -- Fast path: no mutations — slice the original buffer bytes.
        return t._doc._hold:sub(t._bs + 1, t._be)
    end
    if getmetatable(t) == LazyObject then
        return encode_lazy_object_walking(t)
    end
    return encode_lazy_array_walking(t)
end

local function is_array(t)
    local mt = getmetatable(t)
    if mt == _M.empty_array_mt then return true end
    local n = #t
    local count = 0
    for k in pairs(t) do
        count = count + 1
        if type(k) ~= "number" or k < 1 or k > n or k ~= math.floor(k) then
            return false
        end
    end
    return count == n and (n > 0 or mt == _M.empty_array_mt)
end

local function encode_array(t)
    local parts = {}
    for i = 1, #t do
        parts[i] = encode(t[i])
    end
    return "[" .. table.concat(parts, ",") .. "]"
end

local function encode_object(t)
    local parts = {}
    for k, v in pairs(t) do
        if type(k) ~= "string" then
            error("qjson.encode: object key must be a string, got " .. type(k))
        end
        parts[#parts+1] = encode_string(k) .. ":" .. encode(v)
    end
    return "{" .. table.concat(parts, ",") .. "}"
end

-- Dispatch for plain (non-lazy) tables. Separated from the main encode
-- function to keep the lazy-proxy fast path narrow for LuaJIT traces.
local function encode_plain_table(v)
    local mt = getmetatable(v)
    if mt == _M.empty_array_mt then
        return encode_array(v)
    end
    local hint = TABLE_TYPE_HINT[v]
    if hint == "object" then
        return encode_object(v)
    end
    if hint == "array" then
        return encode_array(v)
    end
    if is_array(v) then
        return encode_array(v)
    end
    return encode_object(v)
end

encode = function(v)
    if rawequal(v, _M.null) then
        return "null"
    end
    local tv = type(v)
    if tv == "string" then
        return encode_string(v)
    elseif tv == "number" then
        return encode_number(v)
    elseif tv == "boolean" then
        return v and "true" or "false"
    elseif tv == "table" then
        local mt = getmetatable(v)
        if mt == LazyObject or mt == LazyArray then
            return encode_proxy(v)
        end
        return encode_plain_table(v)
    end
    error("qjson.encode: unsupported value type: " .. tv)
end

_M.encode = encode

-- Debug convenience: tostring(lazy_view) returns the original JSON bytes.
-- Not the canonical encoder — callers should still use qjson.encode for output.
LazyObject.__tostring = encode_proxy
LazyArray.__tostring  = encode_proxy

-- Test-only exports for metatable identity checks.
_M._LazyObject = LazyObject
_M._LazyArray  = LazyArray

return _M
