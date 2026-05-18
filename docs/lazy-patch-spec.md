# Lazy Patch: Structural Patching for qjson.decode

> **Note:** This document is the original *design* memo. The Lua snippets below are high-level pseudocode showing intent — they are not the shipped code. The actual implementation lives in `lua/qjson/table.lua` and `src/ffi.rs`; the splice path is used only for the "all patches target existing fields, no deletions" case, while deletions and new fields fall through to a walking-based encoder (`encode_lazy_object_walking_with_patches`) that avoids the comma-rewriting issues that the splice pseudocode below has. Helper names like `find_field_value_span` correspond to the FFI symbol `qjson_cursor_field_bytes` (`src/ffi.rs:821`).

## Overview

This spec describes an optimization for the "decode → modify few fields → encode" workflow. Instead of materializing the entire root container on first write, we record patches and apply them during encode by splicing the original buffer.

## Problem Statement

Current behavior when modifying a field on a lazy table:

```lua
local tab = qjson.decode(json_str)  -- Returns LazyObject proxy
tab.model = "gpt4o"                 -- Triggers __newindex → materializes entire root
local out = qjson.encode(tab)       -- Walks materialized table + lazy subtrees
```

The `__newindex` handler (`lua/qjson/table.lua:266-285`) calls `materialize_object_contents()` which:
1. Iterates all key/value pairs via FFI
2. Clears the metatable
3. Copies all values into the plain table
4. Sets the new value

Then `encode()` must:
1. Detect the table is no longer lazy (metatable = nil)
2. Call `encode_object()` which uses `pairs()` iteration
3. For each key/value, call `encode_string()` (byte-by-byte processing)
4. Call `table.concat()` to join parts with the large lazy subtree JSON

### Performance Impact

| Payload Size | Current | Theoretical Optimal | Gap |
|--------------|---------|---------------------|-----|
| 1 KB | 3.9 μs | 0.5 μs | 8x |
| 100 KB | 31.8 μs | 5.5 μs | 6x |
| 1 MB | 526.5 μs | 46.9 μs | 11x |

The gap grows with payload size because `table.concat` with large strings has O(n) copy overhead.

## Proposed Solution

### Core Idea

Instead of materializing on write, record patches in a side table. On encode, splice the original buffer with patched values.

```lua
local tab = qjson.decode(json_str)  -- Returns LazyObject proxy
tab.model = "gpt4o"                 -- Records patch: {key="model", new_value='"gpt4o"'}
local out = qjson.encode(tab)       -- Splices: buf[0..model_start] + "gpt4o" + buf[model_end..]
```

### Data Structures

#### Patch Record (Lua side)

```lua
-- Stored in the lazy view's internal table.
-- Each patch records key, encoded JSON value, and the original Lua value
-- (so __index can hand back the user's value without re-decoding).
-- Byte offsets are resolved lazily during encode via qjson_cursor_field_bytes.
_patches = {
    { key = "model",       encoded_value = '"gpt4o"', lua_value = "gpt4o" },
    { key = "temperature", encoded_value = "0.9",      lua_value = 0.9 },
}
```

#### Extended LazyObject/LazyArray

```lua
-- Current internal fields
{
    _doc     = <Doc>,           -- Reference to parsed document
    _cur_box = <ffi cdata>,     -- Cursor box (keeps cdata alive)
    _cur     = <cursor>,        -- Stable cursor reference
    _bs      = <number>,        -- Byte start in original buffer
    _be      = <number>,        -- Byte end in original buffer
    
    -- New fields for lazy patch
    _patches = {},              -- Array of {key, encoded_value} or {index, encoded_value}
    _deleted = {},              -- Set of deleted keys/indices (for nil assignment)
}
```

### Algorithm

#### Modified `__newindex` (LazyObject)

```lua
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
        -- Encode the new value (we keep both encoded and the original Lua
        -- value so __index can return v without re-decoding).
        local encoded = encode_value(v)

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

        deleted[k] = nil
    end
end
```

#### Modified `__index` (LazyObject)

```lua
local original_index = LazyObject.__index

LazyObject.__index = function(t, k)
    -- Check patches first
    local patches = rawget(t, "_patches")
    if patches then
        for _, p in ipairs(patches) do
            if p.key == k then
                -- Return decoded value from patch
                -- Note: need to track original Lua value, not just encoded
                return p.lua_value
            end
        end
    end
    
    -- Check deleted
    local deleted = rawget(t, "_deleted")
    if deleted and deleted[k] then
        return nil
    end
    
    -- Fall back to original lazy lookup
    return original_index(t, k)
end
```

#### Modified `encode_proxy`

```lua
local function encode_proxy(t)
    local patches = rawget(t, "_patches")
    local deleted = rawget(t, "_deleted")
    
    -- Fast path: no patches and not dirty
    if not patches and not deleted and not is_dirty(t) then
        return t._doc._hold:sub(t._bs + 1, t._be)
    end
    
    -- Has patches: use splice encoding
    if patches and #patches > 0 then
        return encode_with_patches(t)
    end
    
    -- Has deletions only or dirty children: fall back to walking
    if getmetatable(t) == LazyObject then
        return encode_lazy_object_walking(t)
    end
    return encode_lazy_array_walking(t)
end
```

#### New `encode_with_patches`

```lua
local function encode_with_patches(t)
    local buf = t._doc._hold
    local patches = rawget(t, "_patches")
    local deleted = rawget(t, "_deleted") or {}
    
    -- Resolve byte offsets for each patch and collect spans to replace
    local replacements = {}  -- { {start, end, new_value}, ... }
    
    for _, p in ipairs(patches) do
        local start_off, end_off = find_field_value_span(t, p.key)
        if start_off then
            -- Existing field: replace value
            replacements[#replacements + 1] = {
                start = start_off,
                end_ = end_off,
                value = p.encoded_value,
            }
        else
            -- New field: will be appended
            -- Handled separately below
        end
    end
    
    -- Collect deleted field spans
    for k in pairs(deleted) do
        local field_start, field_end = find_field_span(t, k)  -- includes key and comma
        if field_start then
            replacements[#replacements + 1] = {
                start = field_start,
                end_ = field_end,
                value = "",  -- delete
            }
        end
    end
    
    -- Sort by start offset
    table.sort(replacements, function(a, b) return a.start < b.start end)
    
    -- Build output by splicing
    local parts = {}
    local pos = t._bs + 1  -- 1-based Lua index
    
    for _, r in ipairs(replacements) do
        -- Copy unchanged portion
        if r.start > pos then
            parts[#parts + 1] = buf:sub(pos, r.start - 1)
        end
        -- Insert replacement
        parts[#parts + 1] = r.value
        pos = r.end_ + 1
    end
    
    -- Copy remaining portion
    if pos <= t._be then
        parts[#parts + 1] = buf:sub(pos, t._be)
    end
    
    -- Handle new fields (not in original)
    local new_fields = {}
    for _, p in ipairs(patches) do
        if not find_field_value_span(t, p.key) then
            new_fields[#new_fields + 1] = '"' .. p.key .. '":' .. p.encoded_value
        end
    end
    
    if #new_fields > 0 then
        -- Insert before closing brace
        local result = table.concat(parts)
        local close_pos = result:find("}%s*$")
        if close_pos then
            local prefix = result:sub(1, close_pos - 1)
            -- Add comma if there are existing fields
            if prefix:match("[^{%s]%s*$") then
                prefix = prefix .. ","
            end
            return prefix .. table.concat(new_fields, ",") .. "}"
        end
    end
    
    return table.concat(parts)
end
```

### FFI Extension (Optional)

For better performance, the byte offset resolution can be moved to Rust:

```c
// New FFI function to get field byte span
int qjson_cursor_field_bytes(
    const qjson_cursor* cur,
    const char* key, size_t key_len,
    size_t* value_start,  // byte offset of value start (after colon)
    size_t* value_end     // byte offset of value end (before comma/brace)
);
```

This avoids re-parsing the structure in Lua to find field positions.

## Edge Cases

### 1. Nested Modifications

```lua
tab.messages[1].role = "assistant"
```

This triggers materialization of `tab.messages[1]`, but `tab` and `tab.messages` remain lazy with patches.

**Handling**: When a child is accessed and modified, the child gets its own `_patches`. The parent's `is_dirty()` check detects the materialized child and falls back to walking.

### 2. Reading a Patched Field

```lua
tab.model = "gpt4o"
print(tab.model)  -- Should return "gpt4o", not the original value
```

**Handling**: `__index` checks `_patches` before falling back to FFI lookup.

### 3. Deleting a Field

```lua
tab.model = nil
```

**Handling**: Record in `_deleted` set. During encode, the field span (including key and comma) is removed.

### 4. Adding a New Field

```lua
tab.new_field = "value"
```

**Handling**: The patch has no corresponding byte span in the original buffer. During encode, new fields are appended before the closing brace.

### 5. Type Changes

```lua
tab.temperature = "hot"  -- was number, now string
```

**Handling**: The encoded value is simply the new JSON representation. Type doesn't matter for splicing.

### 6. Iteration After Patch

```lua
tab.model = "gpt4o"
for k, v in qjson.pairs(tab) do
    print(k, v)
end
```

**Handling**: `__pairs` must merge original fields with patches and exclude deleted fields.

### 7. Multiple Patches to Same Field

```lua
tab.model = "gpt4o"
tab.model = "gpt4o-mini"
```

**Handling**: The second assignment updates the existing patch entry.

### 8. Patch Then Materialize

```lua
tab.model = "gpt4o"
tab.messages[1].role = "assistant"  -- This materializes messages[1]
-- tab still has patches, but tab.messages is now dirty
```

**Handling**: `is_dirty()` detects the materialized child. `encode_with_patches()` handles patched fields, then falls back to walking for dirty children.

## Implementation Plan

### Phase 1: Lua-only Implementation

1. Modify `LazyObject.__newindex` to record patches instead of materializing
2. Modify `LazyObject.__index` to check patches first
3. Add `encode_with_patches()` function
4. Modify `encode_proxy()` to detect and use patches
5. Update `__pairs` to merge patches
6. Add tests for all edge cases

### Phase 2: FFI Optimization (Optional)

1. Add `qjson_cursor_field_bytes()` to Rust FFI
2. Use FFI for byte offset resolution instead of Lua string search
3. Benchmark and validate improvement

## Testing Strategy

### Unit Tests

```lua
-- Basic patch
local tab = qjson.decode('{"a":1,"b":2}')
tab.a = 10
assert(qjson.encode(tab) == '{"a":10,"b":2}')

-- Read patched value
assert(tab.a == 10)

-- Multiple patches
tab.b = 20
assert(qjson.encode(tab) == '{"a":10,"b":20}')

-- Delete field
tab.a = nil
assert(qjson.encode(tab) == '{"b":20}')

-- Add new field
tab.c = 30
assert(qjson.encode(tab) == '{"b":20,"c":30}')

-- Nested modification (triggers partial materialization)
local tab2 = qjson.decode('{"x":{"y":1}}')
tab2.x.y = 2
assert(qjson.encode(tab2) == '{"x":{"y":2}}')

-- Iteration with patches
local tab3 = qjson.decode('{"a":1,"b":2}')
tab3.a = 10
tab3.c = 3
local keys = {}
for k, v in qjson.pairs(tab3) do keys[k] = v end
assert(keys.a == 10 and keys.b == 2 and keys.c == 3)
```

### Performance Tests

```lua
-- Benchmark: 100KB payload, modify 1 field
local json = make_payload(100 * 1024)
local t0 = os.clock()
for i = 1, 10000 do
    local tab = qjson.decode(json)
    tab.model = "gpt4o"
    local _ = qjson.encode(tab)
end
local t1 = os.clock()
-- Expected: < 10 μs/op (vs current ~30 μs/op)
```

## Risks and Mitigations

### Risk 1: Complexity

The patch tracking adds complexity to the lazy table implementation.

**Mitigation**: Clear separation between patch path and materialization path. Comprehensive tests.

### Risk 2: Memory Overhead

Patches table adds memory per modified lazy view.

**Mitigation**: Patches are small (key + encoded value). Only created on first write.

### Risk 3: Correctness

Splicing byte offsets is error-prone.

**Mitigation**: Extensive edge case tests. Fallback to walking if splice fails.

### Risk 4: Iteration Semantics

`pairs()` behavior changes with patches.

**Mitigation**: Document behavior. Ensure compatibility with existing code patterns.

## Success Criteria

1. **Performance**: 4-10x improvement for "decode → modify 1-2 fields → encode" workflow
2. **Correctness**: All existing tests pass, new edge case tests pass
3. **Compatibility**: No breaking changes to public API
4. **Memory**: No significant memory regression for unmodified lazy tables

## Appendix: Benchmark Data

### Current Implementation Profile

```
decode:   5.97 μs (20%)
modify:   0.37 μs (1%)   -- triggers materialization
encode:  12.55 μs (41%)  -- walks materialized root
total:   30.34 μs

Breakdown of encode:
- encode messages (lazy fast-path): 0.09 μs
- table.concat with 100KB string:   8+ μs
- encode_string (byte-by-byte):     0.9 μs
- pairs iteration + type checks:    3+ μs
```

### Projected Optimized Profile

```
decode:   5.97 μs (55%)
modify:   0.10 μs (1%)   -- just records patch
encode:   4.80 μs (44%)  -- splices original buffer
total:   10.87 μs

Breakdown of encode:
- resolve patch offsets:  0.5 μs
- string.sub splicing:    4.0 μs
- table.concat (small):   0.3 μs
```

### Expected Speedup by Payload Size

| Payload | Current | Optimized | Speedup |
|---------|---------|-----------|---------|
| 1 KB | 3.9 μs | 0.5 μs | 8x |
| 10 KB | 5.3 μs | 1.1 μs | 5x |
| 100 KB | 31.8 μs | 5.5 μs | 6x |
| 500 KB | 209.7 μs | 22.6 μs | 9x |
| 1 MB | 526.5 μs | 46.9 μs | 11x |
