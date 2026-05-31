# Migrating from lua-cjson

`qjson` has a cjson-shaped Lua API for incremental migrations, but it is not a
table-building decoder internally. `qjson.decode` returns a lazy proxy over the
original JSON document, and `qjson.encode` knows how to encode that proxy without
materializing unchanged subtrees.

Use this guide when replacing `lua-cjson` in existing LuaJIT or OpenResty code.

## API mapping

| lua-cjson | qjson | Notes |
| --- | --- | --- |
| `cjson.decode(json)` | `qjson.decode(json)` | Returns a lazy object or array proxy, not a plain Lua table. |
| `cjson.decode(json)` when a plain table is required | `qjson.materialize(qjson.decode(json))` | Use before passing decoded data to libraries that use raw table access or C encoders. |
| `cjson.encode(value)` | `qjson.encode(value)` | Encodes lazy proxies, plain Lua tables, and mixed trees. |
| `cjson.null` | `qjson.null` | Aliases `cjson.null` when lua-cjson is available; otherwise qjson provides its own sentinel. |
| `cjson.empty_array_mt` | `qjson.empty_array_mt` | Aliases lua-cjson's empty-array metatable when available. |
| `pairs(t)` / `ipairs(t)` on decoded data | `qjson.pairs(t)` / `qjson.ipairs(t)` | Works on all LuaJIT builds. Plain `pairs` / `ipairs` need LuaJIT 5.2-compat support. |
| `next(t)` on decoded data | No direct lazy-proxy equivalent | Use `qjson.pairs`, `qjson.ipairs`, or `qjson.len`; materialize first if a library requires native `next`. |
| `#t` on decoded data | `qjson.len(t)` | Works on all LuaJIT builds. Plain `#t` needs LuaJIT 5.2-compat support. |
| Field reads after decode | `t.key`, `t[1]`, or `qjson.parse(json):get_*("path")` | Use `qjson.parse` and cursors for parse-once, extract-a-few-fields hot paths. |

## Pick the migration path

For code that already does `decode -> read or mutate -> encode`, start with the
lazy table API:

```lua
local qjson = require("qjson")

local body = qjson.decode(payload)
local model = body.model
body.seen = true

return qjson.encode(body)
```

For code that only needs a few fields, skip the table-shaped API and use the
cursor API directly:

```lua
local qjson = require("qjson")

local doc = qjson.parse(payload)
local model = doc:get_str("body.model")

local usage = doc:open("usage")
local input_tokens = usage and usage:get_i64("input_tokens")
```

Typed integer getters are lossless. `get_i64` returns LuaJIT `int64_t` cdata,
and `get_u64` returns `uint64_t` cdata for unsigned IDs such as Snowflakes or
database `unsigned bigint` values. This avoids LuaJIT's double precision limit
for JSON integers above 2^53. Use `get_f64(path)` for the Lua-number behavior,
or wrap `tonumber(doc:get_i64(path))` only when the value is known to fit.

For code that passes decoded data to a third-party encoder, validator, or helper
that expects an ordinary Lua table, materialize first:

```lua
local qjson = require("qjson")

local proxy = qjson.decode(payload)
third_party_library.accept_table(qjson.materialize(proxy))
```

Do not pass a qjson lazy proxy directly to `cjson.encode`. lua-cjson is written
in C and does not honor the metamethods qjson uses for lazy reads.

`qjson.encode` also accepts LuaJIT `int64_t` and `uint64_t` cdata values and
emits them as decimal JSON integers without `LL` or `ULL` suffixes.

## Behavior differences

### Decoded values are lazy proxies

`qjson.decode` returns a lazy object or array proxy. Field and index reads decode
only the requested value. Nested containers are also proxies and keep stable Lua
identity when read repeatedly.

The first write to a lazy object materializes that object level while preserving
source key order for `qjson.encode`. The first write to a lazy array materializes
the array level into a regular sequence tagged with `qjson.empty_array_mt` when
needed.

`qjson.decode` requires the top-level JSON value to be an object or array because
it returns a table-shaped proxy. If you need to accept top-level scalars, use
`qjson.parse` and the root cursor/getter APIs.

### Iteration and length helpers

OpenResty LuaJIT enables Lua 5.2 compatibility, so plain `pairs`, `ipairs`, and
`#` dispatch to the proxy metamethods there. Stock LuaJIT 5.1 often does not.
Portable code should use:

```lua
for k, v in qjson.pairs(obj) do
    -- object fields in source order
end

for i, v in qjson.ipairs(arr) do
    -- array elements in order
end

local n = qjson.len(obj_or_arr)
```

`qjson.materialize` converts proxies into plain Lua tables. Duplicate object keys
collapse to the last value during materialization, matching the table shape that
lua-cjson callers usually observe.

### Native `next` is not supported on proxies

Lua's native `next(t)` walks the raw table storage directly; it does not call
`__pairs`, `__ipairs`, `__index`, or any qjson helper. On a qjson lazy proxy,
that raw storage is implementation state and cache data, not the decoded JSON
object or array. This means `next(proxy)` is not a valid way to iterate JSON
fields, fetch the first entry, or test whether a decoded object is empty.

Replace common `next` patterns with proxy-aware helpers:

```lua
-- Before, with cjson-decoded plain tables:
if next(obj) == nil then
    -- empty object
end

-- After, with qjson lazy proxies:
if qjson.len(obj) == 0 then
    -- empty object or array
end

for k, v in qjson.pairs(obj) do
    -- object fields in source order
end

for i, v in qjson.ipairs(arr) do
    -- array elements in order
end
```

If code you do not control calls `next`, `rawget`, `rawset`, or otherwise expects
ordinary table storage, pass `qjson.materialize(proxy)` instead of the proxy.

### Validation mode

`qjson.decode(json)` uses qjson's default eager parsing path. Eager mode validates
structure, trailing content, number syntax, string content, and UTF-8 at parse
time.

`qjson.parse(json, { lazy = true })` is available for code that wants value-level
errors to surface only when the offending field is accessed. This is the parser
validation mode, not the same thing as the `qjson.decode` lazy table proxy.

### Nulls and empty arrays

When lua-cjson is installed, qjson reuses `cjson.null` and `cjson.empty_array_mt`
so existing sentinel comparisons can keep working:

```lua
local cjson = require("cjson")
local qjson = require("qjson")

assert(qjson.null == cjson.null)
assert(qjson.empty_array_mt == cjson.empty_array_mt)
```

When lua-cjson is not installed, qjson provides compatible sentinel values for
its own encoder and materializer.

## Unsupported lua-cjson APIs

qjson intentionally does not implement every lua-cjson configuration API.

| lua-cjson API | qjson status |
| --- | --- |
| `cjson.new()` | No equivalent. qjson has module-level functions and no isolated per-instance encoder/decoder state. |
| `cjson.encode_sparse_array()` | No equivalent. qjson follows its own table shape rules and uses `qjson.empty_array_mt` for empty arrays. |
| `cjson.encode_number_precision()` | No equivalent. qjson's encoder uses its built-in number formatting. |

If your application depends on an unsupported lua-cjson knob, keep lua-cjson for
that path or isolate the migration to call sites that use decode/encode without
per-instance configuration.

## Incremental checklist

1. Replace `local cjson = require("cjson")` with `local qjson = require("qjson")`
   in one read-heavy path.
2. Replace `cjson.decode` / `cjson.encode` with `qjson.decode` / `qjson.encode`.
3. Replace `pairs`, `ipairs`, and `#` on decoded values with `qjson.pairs`,
   `qjson.ipairs`, and `qjson.len` for LuaJIT portability.
4. Add `qjson.materialize(value)` before sending decoded values to third-party
   encoders or helpers that require plain Lua tables.
5. In hot paths that only read a few fields, consider `qjson.parse` plus
   `doc:get_*` or cursor getters instead of `qjson.decode`.
6. Leave call sites that depend on `cjson.new`, sparse-array configuration, or
   number-precision configuration on lua-cjson until they can be redesigned.
