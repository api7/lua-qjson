# Lazy table API with cjson-compatible decode / encode

**Status**: design approved, ready for implementation plan
**Touches**: `lua/quickdecode/table.lua` (new), `lua/quickdecode.lua` (re-export), `src/ffi.rs` (one new helper), `include/lua_quick_decode.h`, `tests/lua/lazy_table_spec.lua` (new), `benches/lua_bench.lua`, `README.md`

## Problem

Current `quickdecode` exposes a path-based API (`doc:get_str("foo.bar")`, `doc:open("path")`). It's fast and zero-alloc, but it does not look like what callers get from `cjson.decode`, which is a normal Lua table they can read with `t.foo`, iterate with `pairs`, and re-encode with `cjson.encode`. Migrating existing cjson-shaped code to `quickdecode` therefore requires touching every access site.

We want to keep the lazy-decode performance win while giving callers an object that **reads like** the table cjson would have returned and **encodes back** to JSON byte-for-byte equivalent to what `cjson.encode` would emit. Writes are allowed; they materialize the affected level into a plain Lua table.

## Goal

Add `qd.decode(json) → lazy_table` and `qd.encode(lazy_or_real) → json` so the migration cost from cjson to quickdecode is, in most call sites, just `cjson` → `qd` and `cjson.encode` → `qd.encode`. Other than those two symbol swaps, code that was reading `t.foo` / `pairs(t)` / `t.headers[1].name` keeps working.

The Phase-1 structural scan from the existing parser is reused unchanged — this feature is a new Lua layer on top of the existing C ABI, plus a single FFI helper.

## User-facing API

```lua
local qd    = require("quickdecode")
local cjson = require("cjson")          -- optional; provides null / empty_array sentinels

local t = qd.decode(json_str)

-- Read like a cjson table. Nested containers stay lazy.
print(t.model)
for _, m in ipairs(t.messages) do
    print(m.role, m.content)
end

-- Writes materialize the affected level into a plain Lua table.
t.extra = "x"

-- Re-emit. Unmodified subtrees fast-path through original-substring emit.
local s = qd.encode(t)
```

The only API difference vs cjson: callers must use `qd.encode`, not `cjson.encode`. `cjson.encode` bypasses metamethods in C, so a lazy proxy cannot be made transparent to it without giving up the lazy win entirely.

### Exports

`qd.decode(json_str)` — parse and return a lazy view.
`qd.encode(value)` — serialize a lazy view, a real table, or any cjson-shaped value.
`qd.materialize(value)` — recursively force a lazy view into a plain Lua table (for callers that have to pass to `cjson.encode` or any third-party consumer that walks raw tables).
`qd.pairs(t)`, `qd.ipairs(t)` — explicit iterators for environments without LJ52 `__pairs` / `__ipairs`.
`qd.null` — JSON null sentinel. Aliased to `cjson.null` when cjson is loaded.
`qd.empty_array_mt` — metatable marking a real Lua table as a JSON array (so empty / numeric-keyed tables encode as `[]`). Aliased to `cjson.empty_array_mt` when cjson is loaded.

Existing `qd.parse` and the path-based getters stay; the new API lives alongside them.

## Architecture

### Rust / FFI

One new export. Everything else reuses existing entry points (`qjd_cursor_field`, `qjd_cursor_index`, `qjd_cursor_get_*`, `qjd_cursor_typeof`, `qjd_cursor_len`).

```c
// Write the original-buffer byte range [byte_start, byte_end) that the
// cursor's value occupies, including the value itself but not surrounding
// whitespace or separators. Used by qd.encode's "emit original substring"
// fast path on unmodified lazy subtrees.
int qjd_cursor_bytes(const qjd_cursor*, size_t* byte_start, size_t* byte_end);
```

Implementation: `byte_start = indices[idx_start] as usize`; `byte_end = indices[idx_end] as usize + 1`. For scalar cursors, `byte_start` snaps to the first non-whitespace after the previous structural char (the helper already exists at `Decoder::find_scalar_start` / `scalar_bytes` in `src/ffi.rs`); the helper reuses that logic so `qjd_cursor_bytes` returns a clean span for both scalar and container cursors.

No ABI changes to `qjd_cursor` or `qjd_doc`. No new error codes.

### Lua layer — `lua/quickdecode/table.lua`

Two metatables:

```lua
local LazyObject = {}     -- JSON {...}
local LazyArray  = {}     -- JSON [...]
```

A lazy view is a plain Lua table with four fields:

```lua
{
    _doc = doc,            -- the parent Doc (holds the Rust qjd_doc + buffer hold)
    _cur = cursor_cdata,   -- qjd_cursor by value
    _bs  = byte_start,     -- byte offset into _doc._hold
    _be  = byte_end,
}
```

`_doc` keeps the underlying buffer pinned; `_bs` / `_be` enable substring emit without re-querying FFI. `_cur` is a `qjd_cursor` cdata stored by value (it's just two `u32`s of payload).

Metamethods (each defined on both `LazyObject` and `LazyArray` where applicable):

| Method | Behavior |
|---|---|
| `__index(t, k)` | `LazyObject`: route `qjd_cursor_field(_cur, k)` → resolve to a scalar (decode and return real Lua value), a JSON null (return `qd.null`), or another container (wrap in a new `LazyObject` / `LazyArray`). `LazyArray`: same but via `qjd_cursor_index(_cur, k-1)` for integer `k`; falls through to `nil` for non-integer keys (cjson semantics). Missing key returns `nil` (NOT_FOUND). |
| `__newindex(t, k, v)` | Materialize this level (see "Write semantics" below), detach the metatable, then `rawset(t, k, v)`. |
| `__len(t)` | `qjd_cursor_len(_cur)`. |
| `__pairs(t)` | Return a stateful iterator over the immediate children: each step calls into FFI to get the next (key, value) pair at this level. Values are wrapped lazily, matching `__index` semantics. |
| `__ipairs(t)` | `LazyArray` only: 1-based iterator using `qjd_cursor_index`. On `LazyObject`, falls through to default (returns nothing useful, same as cjson behavior on objects). |
| `__tostring(t)` | Emit the original JSON substring `_doc._hold:sub(_bs+1, _be)` — a debug convenience; not the canonical encoder. |

A user-facing `qd.pairs(t)` and `qd.ipairs(t)` exist as named wrappers so callers on plain Lua 5.1 (no `__pairs` honoured) can still iterate. They forward to the metatable's iterator factory.

### `qd.encode(value)`

Three branches:

```
qd.encode(x):
  rawequal(x, qd.null)                          → "null"
  type(x) == "string" / "number" / "boolean"    → scalar encode
  type(x) == "table":
    getmetatable(x) is LazyObject / LazyArray   → emit _doc._hold:sub(_bs+1, _be)
    otherwise (real table, possibly mixed)      → walk lua_next, recurse qd.encode on each v
                                                  array-vs-object decision: if t has
                                                  qd.empty_array_mt or all keys are
                                                  1..#t integers → encode as [...]
                                                  else → encode as {...}
```

The "lazy proxy → original-substring" branch is the fast path that gives the encode win. The "real table" branch is the fallback for any subtree that was materialized via `__newindex`. A mixed tree (object whose top level was written, nested objects still lazy) walks one level via lua_next and recurses; nested lazies emit their original substring.

For a plain Lua table that never came from `qd.decode`, `qd.encode` works exactly like `cjson.encode`. If `cjson` is loaded and the table has no lazy proxies anywhere, `qd.encode` may delegate to `cjson.encode(table)` rather than re-implementing the entire encoder; the canonical implementation does its own walk to keep the dependency optional, but matching cjson's output rules (key ordering not preserved, etc.) is the contract.

### Sentinel bridging

At module load:

```lua
local ok, cjson = pcall(require, "cjson")
local _M = {}
if ok then
    _M.null = cjson.null
    _M.empty_array_mt = cjson.empty_array_mt
else
    _M.null = setmetatable({}, { __tostring = function() return "null" end })
    _M.empty_array_mt = { __jsontype = "array" }
end
```

Callers that already check `v == cjson.null` keep working.

`qd.empty_array_mt` exists for the **real-table** side of the API: a lazy proxy always knows it is an object or an array from its metatable (`LazyObject` vs `LazyArray`), so empty containers on the read side need no special handling. The sentinel kicks in when:

- `qd.materialize` is asked to convert a lazy (possibly empty) array — the output table is stamped with `qd.empty_array_mt` so re-encoding round-trips to `[]`.
- A caller hands `qd.encode` a hand-built empty Lua table — without the metatable it cannot tell `[]` from `{}`, so it falls back to `{}` (matching cjson's default).
- `__newindex` on an empty `LazyArray` materializes to a real table; the metatable is set to `qd.empty_array_mt` so the array tag survives.

## Write semantics — first-touch materialization

`t.foo = v` on a lazy proxy:

1. Build a temporary plain table by walking `_cur`'s direct children. For each `(k, v)`:
   - Scalar JSON value → decode and store the real Lua scalar.
   - JSON null → `qd.null`.
   - Nested container → a freshly constructed `LazyObject` / `LazyArray` proxy (no recursive materialization).
2. Atomically swap: copy the temp table's contents into `t` (via `rawset`), then `setmetatable(t, nil)` (or to `qd.empty_array_mt` if `t` was a `LazyArray` so encode keeps the array tag).
3. `rawset(t, k, v)`.

After this, `t` is a normal Lua table. Reads no longer go through FFI; writes are normal table writes. Nested containers are still lazy proxies — accessing them still triggers their own `__index`, and writing into them triggers their own materialization.

Failure during step 1 (e.g. an unexpected FFI error mid-walk) leaves `t` untouched and re-raises. The implementation builds the materialized contents in a *separate* local table and only copies into `t` once the walk completes, so partial-write states cannot leak.

## Error handling

- `qd.decode(invalid_json)` → `error("quickdecode: JSON parse error")` (same as current `qd.parse`).
- `__index` returns `nil` for missing keys (cjson semantics). Other FFI errors (malformed UTF-8 in `\u`, etc.) raise.
- `qd.encode(unsupported_value)` (function / userdata / table with cycles) → raises; matches cjson.encode behavior.
- `__newindex` materialization is atomic: success or no-op-on-error.

## Testing

### Rust

`tests/ffi_cursor_bytes.rs` (new): for each fixture, walk the parse tree, call `qjd_cursor_bytes` at every node, and assert `buf[byte_start..byte_end]` reparses to a structurally-equal JSON value (use `serde_json` from a dev-dependency in the test crate, not in the main lib). Covers scalar / object / array / nested.

### Lua busted spec — `tests/lua/lazy_table_spec.lua`

1. **cjson equivalence (read side).** For each fixture, `qd.materialize(qd.decode(j))` deep-equals `cjson.decode(j)` (custom deep-equal aware of `qd.null` ≡ `cjson.null` and `empty_array_mt` ≡ `cjson.empty_array_mt`).
2. **Encode round-trip.** `qd.encode(qd.decode(j))` and `cjson.encode(cjson.decode(j))`, after parsing both back through `cjson.decode`, are structurally equal. (Byte-equal comparison is not required — neither library guarantees object key order.)
3. **Original-substring fast path.** For an unmodified lazy proxy, `qd.encode(t)` returns exactly `j` minus surrounding whitespace (assert byte equality on a fixture with no insignificant whitespace).
4. **Write-then-encode.** `t.extra = "x"` on a lazy `LazyObject`; assert `getmetatable(t) == nil`; assert `qd.encode(t)` is the original JSON with `"extra":"x"` appended (modulo key order).
5. **Nested stays lazy after parent write.** After mutating top level, `t.messages` is still a `LazyArray` proxy; mutating `t.messages[1]` materializes only that array level, not its siblings.
6. **Sentinel propagation.** JSON `null` → `qd.null`; `qd.encode` emits `null`. JSON `[]` → table with `qd.empty_array_mt`; `qd.encode` emits `[]`.
7. **Lazy access counter.** Hook `qjd_cursor_field` via a Lua-side ffi-cdef wrapper (or expose a debug counter on the doc); assert that reading 3 of 100 object fields makes ~3 FFI calls, not 100.
8. **Shallow `pairs`.** `for k, v in pairs(t)` over a `LazyObject` yields each direct child; nested values are still lazy proxies.

### Bench — `benches/lua_bench.lua`

Add scenarios alongside existing `qd.parse` rows:

- `qd.decode + access 3 fields` — should land within ±10% of `qd.parse + get_str` for the same payload.
- `qd.decode + qd.encode (unmodified)` — substring emit fast path; expected ≥ 5× `cjson.encode(cjson.decode(j))`.
- `qd.decode + qd.materialize + qd.encode` — full round-trip without lazy benefit; expected within +20% of cjson decode+encode.

### Performance targets

| Scenario | Target |
|---|---|
| Read 3 fields via `t.foo` | within ±10% of current `qd.parse:get_str` |
| `qd.encode(unmodified)` | ≥ 5× `cjson.encode(cjson.decode(j))` (memcpy substring vs full re-emit) |
| `qd.encode` after `qd.materialize` | within +20% of `cjson.encode(cjson.decode(j))` |
| `qd.decode + iterate 100 keys at top level` | within ±20% of `cjson.decode + pairs` |

A bench gap of 10–20% is acceptable because each metamethod hop costs more than a direct table read in LuaJIT; the win comes from never building the deep table in the first place when only a few fields are read.

## Scope / non-goals

- **No deep materialization.** `__newindex` materializes the affected level only; nested containers stay lazy.
- **No mutation tracking / overlay.** A lazy subtree is either entirely original (emit substring) or entirely materialized (walk lua_next). No per-key dirty bits.
- **No cjson.encode literal compatibility.** Callers must switch to `qd.encode`. This is the single API change required by the migration.
- **No JSON encoding spec extensions.** `qd.encode` aims for cjson.encode-compatible output, not for stable key ordering or canonical JSON.
- **Encode path-based access (`t:get_str("a.b.c")`) is untouched.** The new `qd.decode` is additive; the existing `qd.parse` API stays available for callers who prefer it.

## Open questions

None at design time. Open items, if any surface during implementation, will land in `README.md` under **Roadmap / Deferred**.
