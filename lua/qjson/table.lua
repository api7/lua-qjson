-- Lazy table view + cjson-compatible encoder for qjson.
--
-- This module relies on the FFI cdef set up by `lua/qjson.lua`, so
-- callers must `require("qjson")` (transitively or directly) before
-- they require this module.

local ffi = require("ffi")
local C   = ffi.load("qjson")
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

-- Build a new lazy view for a child container cursor.
-- src_box is an FFI cdata `qjson_cursor[1]`; src_box[0] is the cursor whose
-- data we copy into a fresh per-view allocation so the new view's _cur
-- survives later overwrites of src_box.
local function wrap_child(parent_view, src_box)
    C.qjson_cursor_bytes(src_box[0], sz_a, sz_b)
    local own_box = ffi.new("qjson_cursor[1]")
    ffi.copy(own_box, src_box, ffi.sizeof("qjson_cursor"))
    return {
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
-- Container results (lazy proxies) are rawset-cached into `self` so that
-- subsequent accesses return the same Lua table object. This is required for
-- `t.a.x = v` to propagate back: __newindex materializes `t.a` in-place, and
-- the next `t.a` lookup retrieves the already-materialized table from the
-- raw table rather than creating a fresh proxy.
local function read_object_field(self, key)
    if type(key) ~= "string" then return nil end

    -- Check patches first
    local patches = rawget(self, "_patches")
    if patches then
        for _, p in ipairs(patches) do
            if p.key == key then
                return p.lua_value
            end
        end
    end

    -- Check deleted
    local deleted = rawget(self, "_deleted")
    if deleted and deleted[key] then
        return nil
    end

    -- Use child_box so the lookup result does not alias self._cur (which is
    -- itself stored in root_box's backing memory in the decode caller).
    local rc = C.qjson_cursor_field(self._cur, key, #key, child_box)
    if not check(rc) then return nil end
    local v = decode_cursor(self, child_box)
    -- Cache containers so identity is stable and materialization sticks.
    if type(v) == "table" then rawset(self, key, v) end
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
-- Handles patches and deletions.
local function lazy_object_iter(state, _prev_key)
    local view = state.view
    local patches = rawget(view, "_patches")
    local deleted = rawget(view, "_deleted")

    -- First, iterate through original fields (skipping deleted ones)
    while state.i < state.original_count do
        local i = state.i
        state.i = i + 1
        local rc = C.qjson_cursor_object_entry_at(
            view._cur, i, strp_box, size_box, child_box
        )
        if rc == QJSON_NOT_FOUND then break end
        check(rc)
        local k = ffi.string(strp_box[0], size_box[0])

        -- Skip deleted keys
        if deleted and deleted[k] then
            -- continue to next iteration
        else
            -- Check if this key has a patch
            if patches then
                for _, p in ipairs(patches) do
                    if p.key == k then
                        return k, p.lua_value
                    end
                end
            end
            -- No patch, return original value
            local v = decode_cursor(view, child_box)
            return k, v
        end
    end

    -- Then, iterate through new fields (patches for keys not in original)
    if patches then
        while state.patch_i <= #patches do
            local p = patches[state.patch_i]
            state.patch_i = state.patch_i + 1
            -- Check if this is a new field (not in original)
            local rc = C.qjson_cursor_field_bytes(view._cur, p.key, #p.key, sz_a, sz_b)
            if rc == QJSON_NOT_FOUND then
                return p.key, p.lua_value
            end
        end
    end

    return nil
end

function LazyObject.__pairs(t)
    -- Count original fields
    local rc = C.qjson_cursor_len(t._cur, "", 0, size_box)
    check(rc)
    local original_count = tonumber(size_box[0])
    return lazy_object_iter, { view = t, i = 0, original_count = original_count, patch_i = 1 }, nil
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
-- keys with these names would collide (minor, deferred). Centralized here so
-- the dirty check and __newindex can share the list.
local INTERNAL_KEYS = {
    _doc = true, _cur_box = true, _cur = true, _bs = true, _be = true,
    _patches = true, _deleted = true,
}

-- Forward declaration for encode (needed by __newindex to encode patch values)
local encode

-- On write, record a patch instead of materializing the entire object.
-- This allows encode to splice the original buffer with patched values.
LazyObject.__newindex = function(t, k, v)
    -- Initialize patches table if needed
    if not rawget(t, "_patches") then
        rawset(t, "_patches", {})
        rawset(t, "_deleted", {})
    end

    local patches = rawget(t, "_patches")
    local deleted = rawget(t, "_deleted")

    if v == nil then
        -- Mark key as deleted
        deleted[k] = true
        -- Remove from patches if previously patched
        for i, p in ipairs(patches) do
            if p.key == k then
                table.remove(patches, i)
                break
            end
        end
    else
        -- Encode the new value (we need both encoded and lua_value)
        local encoded = encode(v)

        -- Update or add patch
        local found = false
        for _, p in ipairs(patches) do
            if p.key == k then
                p.encoded_value = encoded
                p.lua_value = v
                found = true
                break
            end
        end
        if not found then
            patches[#patches + 1] = { key = k, encoded_value = encoded, lua_value = v }
        end

        -- Remove from deleted if previously deleted
        deleted[k] = nil
    end
end

-- On first write, walk all existing elements into a plain sequence,
-- switch to empty_array_mt (no lazy machinery), then apply the assignment.
-- Existing rawget-cached entries are preserved so callers' references remain valid.
LazyArray.__newindex = function(t, k, v)
    local contents = materialize_array_contents(t)
    -- Snapshot integer-key cache BEFORE nilling internals.
    -- Use next() for raw iteration: pairs() would invoke __pairs on lazy arrays,
    -- walking the full JSON via FFI instead of the Lua-side rawget cache.
    local cache = {}
    local ck, cv = next(t)
    while ck ~= nil do
        if type(ck) == "number" then
            cache[ck] = cv
        end
        ck, cv = next(t, ck)
    end
    t._doc, t._cur_box, t._cur, t._bs, t._be = nil, nil, nil, nil, nil
    setmetatable(t, _M.empty_array_mt)
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
        for _, kv in ipairs(materialize_object_contents(v)) do
            out[kv[1]] = materialize(kv[2])
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

-- Minimal JSON string escaper covering the cjson default set.
local function encode_string(s)
    local out = {'"'}
    for i = 1, #s do
        local b = string_byte(s, i)
        if b == 0x22 then out[#out+1] = '\\"'
        elseif b == 0x5C then out[#out+1] = '\\\\'
        elseif b == 0x0A then out[#out+1] = '\\n'
        elseif b == 0x0D then out[#out+1] = '\\r'
        elseif b == 0x09 then out[#out+1] = '\\t'
        elseif b == 0x08 then out[#out+1] = '\\b'
        elseif b == 0x0C then out[#out+1] = '\\f'
        elseif b < 0x20 then out[#out+1] = string_format('\\u%04x', b)
        else out[#out+1] = string.char(b)
        end
    end
    out[#out+1] = '"'
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

-- A lazy subtree is "dirty" if any cached descendant has been materialized
-- (no longer carries Lazy* metatable), or if it has patches/deletions.
-- Non-cached descendants are guaranteed untouched, so we only need to walk
-- the rawget-cached entries.
local function is_dirty(v)
    if type(v) ~= "table" then return false end
    local mt = getmetatable(v)
    if mt ~= LazyObject and mt ~= LazyArray then
        return true  -- materialized
    end
    -- Check for patches or deletions
    local patches = rawget(v, "_patches")
    local deleted = rawget(v, "_deleted")
    if patches and #patches > 0 then return true end
    if deleted then
        for _ in pairs(deleted) do return true end
    end
    -- Use next() for raw table iteration: pairs() would invoke __pairs on
    -- lazy tables, walking the full JSON via FFI instead of the Lua cache.
    local k, child = next(v)
    while k ~= nil do
        if not INTERNAL_KEYS[k] then
            if is_dirty(child) then return true end
        end
        k, child = next(v, k)
    end
    return false
end

-- Check if a lazy view has patches (but not necessarily dirty children)
local function has_patches(v)
    local patches = rawget(v, "_patches")
    return patches and #patches > 0
end

-- Check if a lazy view has deletions
local function has_deletions(v)
    local deleted = rawget(v, "_deleted")
    if not deleted then return false end
    for _ in pairs(deleted) do return true end
    return false
end

-- Walk a dirty LazyObject and emit JSON, preferring cached children (which
-- may be materialized) over freshly resolved cursors. Non-cached children
-- emit through a fresh proxy and naturally fast-path their unmodified subtree.
local function encode_lazy_object_walking(t)
    local parts = {}
    local i = 0
    while true do
        local rc = C.qjson_cursor_object_entry_at(t._cur, i, strp_box, size_box, child_box)
        if rc == QJSON_NOT_FOUND then break end
        check(rc)
        local k = ffi.string(strp_box[0], size_box[0])
        local v
        local cached = rawget(t, k)
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

-- Check if any cached child is dirty (materialized or has patches)
local function has_dirty_children(t)
    local k, child = next(t)
    while k ~= nil do
        if not INTERNAL_KEYS[k] then
            if type(child) == "table" then
                local mt = getmetatable(child)
                if mt ~= LazyObject and mt ~= LazyArray then
                    return true  -- materialized child
                end
                if is_dirty(child) then
                    return true
                end
            end
        end
        k, child = next(t, k)
    end
    return false
end

-- Encode a LazyObject with patches by splicing the original buffer.
-- This is the fast path for "decode -> modify few fields -> encode".
local function encode_with_patches(t)
    local buf = t._doc._hold
    local patches = rawget(t, "_patches") or {}
    local deleted = rawget(t, "_deleted") or {}

    -- Build a set of patched keys for quick lookup
    local patched_keys = {}
    for _, p in ipairs(patches) do
        patched_keys[p.key] = p
    end

    -- Collect replacements: { {start, end_, value}, ... }
    local replacements = {}

    -- For each patch, find the field's byte range in the original buffer
    for _, p in ipairs(patches) do
        local rc = C.qjson_cursor_field_bytes(t._cur, p.key, #p.key, sz_a, sz_b)
        if rc == QJSON_OK then
            -- Existing field: replace value
            replacements[#replacements + 1] = {
                start = tonumber(sz_a[0]),
                end_ = tonumber(sz_b[0]),
                value = p.encoded_value,
            }
        end
        -- If NOT_FOUND, it's a new field - handled separately below
    end

    -- Collect deleted field spans (we need to find the full field span including key)
    -- For simplicity, we'll handle deletions by walking and skipping deleted keys
    local has_deleted = false
    for _ in pairs(deleted) do has_deleted = true; break end

    -- If we have deletions or new fields, fall back to walking
    -- (splicing deletions is complex due to comma handling)
    local new_fields = {}
    for _, p in ipairs(patches) do
        local rc = C.qjson_cursor_field_bytes(t._cur, p.key, #p.key, sz_a, sz_b)
        if rc == QJSON_NOT_FOUND then
            new_fields[#new_fields + 1] = p
        end
    end

    if has_deleted or #new_fields > 0 then
        -- Fall back to walking for complex cases
        return encode_lazy_object_walking_with_patches(t, patches, deleted)
    end

    -- Sort replacements by start offset
    table.sort(replacements, function(a, b) return a.start < b.start end)

    -- Build output by splicing
    local parts = {}
    local pos = t._bs + 1  -- 1-based Lua index

    for _, r in ipairs(replacements) do
        -- Copy unchanged portion (convert 0-based to 1-based)
        local r_start_1based = r.start + 1
        if r_start_1based > pos then
            parts[#parts + 1] = buf:sub(pos, r_start_1based - 1)
        end
        -- Insert replacement
        parts[#parts + 1] = r.value
        pos = r.end_ + 1  -- end_ is exclusive, so +1 for 1-based
    end

    -- Copy remaining portion
    if pos <= t._be then
        parts[#parts + 1] = buf:sub(pos, t._be)
    end

    return table.concat(parts)
end

-- Walk a LazyObject with patches, handling deletions and new fields
local function encode_lazy_object_walking_with_patches(t, patches, deleted)
    local parts = {}

    -- Build a set of patched keys for quick lookup
    local patched_keys = {}
    for _, p in ipairs(patches) do
        patched_keys[p.key] = p
    end

    -- Walk original fields
    local i = 0
    while true do
        local rc = C.qjson_cursor_object_entry_at(t._cur, i, strp_box, size_box, child_box)
        if rc == QJSON_NOT_FOUND then break end
        check(rc)
        local k = ffi.string(strp_box[0], size_box[0])

        -- Skip deleted keys
        if not deleted[k] then
            local v
            local patch = patched_keys[k]
            if patch then
                -- Use patched value (already encoded)
                parts[#parts + 1] = encode_string(k) .. ":" .. patch.encoded_value
            else
                -- Use original or cached value
                local cached = rawget(t, k)
                if cached ~= nil and not INTERNAL_KEYS[k] then
                    v = cached
                else
                    v = decode_cursor(t, child_box)
                end
                parts[#parts + 1] = encode_string(k) .. ":" .. encode(v)
            end
        end
        i = i + 1
    end

    -- Add new fields (patches for keys not in original)
    for _, p in ipairs(patches) do
        local rc = C.qjson_cursor_field_bytes(t._cur, p.key, #p.key, sz_a, sz_b)
        if rc == QJSON_NOT_FOUND then
            parts[#parts + 1] = encode_string(p.key) .. ":" .. p.encoded_value
        end
    end

    return "{" .. table.concat(parts, ",") .. "}"
end

local function encode_proxy(t)
    local patches = rawget(t, "_patches")
    local deleted = rawget(t, "_deleted")

    -- Check if we have patches or deletions
    local has_patch = patches and #patches > 0
    local has_del = false
    if deleted then
        for _ in pairs(deleted) do has_del = true; break end
    end

    -- Fast path: no mutations at all — slice the original buffer bytes.
    if not has_patch and not has_del and not has_dirty_children(t) then
        return t._doc._hold:sub(t._bs + 1, t._be)
    end

    -- Has patches: use splice encoding for objects
    if getmetatable(t) == LazyObject then
        if has_patch and not has_dirty_children(t) then
            return encode_with_patches(t)
        end
        return encode_lazy_object_walking_with_patches(t, patches or {}, deleted or {})
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
        if is_array(v) then
            return encode_array(v)
        end
        return encode_object(v)
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
