# Lazy scalar-patch on existing keys — design

GitHub issue: [api7/lua-qjson#48](https://github.com/api7/lua-qjson/issues/48)

## Goal

Land the smallest slice of PR #44 (`feat(lazy): structural patching for decode+modify+encode`) that captures the bulk of its encode-path performance benefit while eliminating its −37% regression on the `github-100k parse + access fields` benchmark.

Concretely: record **scalar replacements on existing keys** of a `LazyObject` in a side `_patches` list, and emit them via a splice encoder over the original JSON byte buffer. Any other mutation pattern (delete, new key, table-valued write) falls through to the existing materialization path unchanged.

## Non-goals (deferred to future PRs)

- Deletion (`t.x = nil`) — PR-2 candidate
- New-field insertion (`t.new_key = v` where `new_key` not in original JSON) — PR-3 candidate
- Sidecar container cache (`_child_cache`)
- `LazyArray` patches
- `is_dirty` / walking encoder rewrites beyond a minimal patch-aware tweak

## Architecture

### Rust / FFI surface

One new exported symbol:

```c
int qjson_cursor_field_bytes(
    const qjson_cursor*  c,
    const char*          key,
    size_t               key_len,
    qjson_cursor*        value_out,   // may be NULL
    size_t*              value_bs,    // required
    size_t*              value_be     // required
);
```

- Equivalent to a fused `qjson_cursor_field` + `qjson_cursor_bytes` — saves one FFI crossing at write time.
- `value_out` may be NULL: this PR only needs the byte span; skipping the cursor copy avoids a memcpy.
- Returns `QJSON_NOT_FOUND` when the key is absent, propagating other `qjson_err` codes unchanged.
- Wrapped in the same `catch_unwind` panic barrier as every other `pub unsafe extern "C"` in `src/ffi.rs`.

Declared in three places (CLAUDE.md requirement — error enum sync rule applies to all FFI):
- `src/ffi.rs` (Rust impl)
- `include/qjson.h` (public C header)
- `lua/qjson.lua` (`ffi.cdef` block)

### Lua side — `lua/qjson/table.lua`

`LazyObject` views gain **one** new field, lazily allocated:

```lua
_patches = nil    -- nil until first patch write
                  -- when non-nil: array of { k = string, v = scalar, bs = int, be = int }
                  --   k  : the JSON key being patched
                  --   v  : the replacement scalar value (Lua string/number/boolean/qjson.null)
                  --   bs : value byte-start in _doc._hold (inclusive)
                  --   be : value byte-end   in _doc._hold (exclusive)
```

Added to `INTERNAL_KEYS` so `next(view)` skips it.

No other structural changes: no sidecar `_child_cache`, no `_deleted` set, no changes to `LazyArray`, no changes to the rawget-cache discipline for container children.

## Flows

### Write — `LazyObject.__newindex(t, k, v)`

```
1. Fast precheck (Lua-only, no FFI):
   if type(k) ~= "string"             → goto fallthrough
   if v is not scalar (string |
                       number |
                       boolean |
                       qjson.null)    → goto fallthrough

2. FFI: qjson_cursor_field_bytes(t._cur, k, #k, NULL, &bs, &be)
   if rc == QJSON_NOT_FOUND            → goto fallthrough (key absent → would be insertion)
   if rc != OK                         → check(rc) propagates error

3. Record patch:
   patches = rawget(t, "_patches")
   if patches == nil:
       patches = {}
       rawset(t, "_patches", patches)
   linear scan for existing entry with same k:
       found  → update entry.v (bs/be unchanged)
       absent → append { k = k, v = v, bs = bs, be = be }

4. Invalidate cached child:
   rawset(t, k, nil)
   — drops any previously cached container proxy at this key, so the
     next read goes through __index and finds the patch.

fallthrough:
   delegate to existing materialize_object_contents path, but first
   apply any pre-existing _patches into the materialized contents and
   then clear _patches before setmetatable(t, nil).
```

The FFI call in step 2 is the only side effect that can fail. Steps 3-4 are pure Lua table ops. So patch recording is atomic: either the patch is fully recorded or the view falls through to materialization; no intermediate state.

### Read — `LazyObject.__index = read_object_field(self, key)`

```
1. if type(key) ~= "string"  → return nil  (unchanged)

2. patches = rawget(self, "_patches")
   if patches ~= nil:
       linear scan; on hit return patch.v

3. Original FFI path (qjson_cursor_field + decode_cursor): unchanged
4. Container result rawset-cached on self: unchanged
```

Hot-path cost when no patches were ever recorded: 1 `rawget("_patches")` + 1 nil branch = ~2 LuaJIT bytecodes. This is the entire delta vs main on the read-only fast path.

### Encode — `encode_proxy(t)`

Decision tree (priority order):

```
A. is_dirty(t)              → encode_lazy_object_walking(t)   [existing, small tweak]
B. rawget(t, "_patches")    → encode_lazy_object_splice(t)    [new]
C. else                     → t._doc._hold:sub(t._bs+1, t._be) [existing, unchanged]
```

- **A** wins over **B**: a dirty (materialized) cached child means the walking encoder must run; it gains a `_patches` lookup per key so patched scalars still substitute correctly. A key cannot be both dirty and patched because write-step 4 clears the cached child.
- **B** new function `encode_lazy_object_splice(t)`:
  1. Sort `_patches` by `bs` ascending (≤5 entries typical; sort cost negligible).
  2. `local buf, cursor, parts = t._doc._hold, t._bs, {}`
  3. For each patch `p`:
     - `parts[#parts+1] = buf:sub(cursor+1, p.bs)`   — original bytes
     - `parts[#parts+1] = encode(p.v)`               — replacement scalar
     - `cursor = p.be`
  4. `parts[#parts+1] = buf:sub(cursor+1, t._be)`    — tail
  5. `return table.concat(parts)`
- **C** unchanged: no patches, no dirty children → slice the whole original range.

### Iteration — `lazy_object_iter` (the `__pairs` driver)

Per yielded `(k, v)` from FFI, look up `_patches` and substitute when matched. Key order is the original JSON key order — unchanged.

### `materialize(v)`

Recursive full materialization must also apply patches. Simplest implementation: route through `__pairs` (which already substitutes), instead of calling `materialize_object_contents` directly.

## Behavior preservation

| Surface | Guarantee | Mechanism |
|---|---|---|
| `next(view)` | Identical to main | `_patches` added to `INTERNAL_KEYS` |
| `pairs(view)` / `qjson.pairs(view)` | Unpatched: identical; patched: substitutes value, preserves key order | `lazy_object_iter` patch lookup |
| `#view` / `qjson.len(view)` | Identical | `lazy_len` unchanged; scalar patches do not change child count |
| Lua string pointer lifetime | Identical | No new ptr caching; only size_t offsets stored |
| FFI panic barrier | Preserved | New FFI wrapped in `catch_unwind` |
| `LazyArray` | Identical | Out of scope this PR |

## Edge cases

1. **Repeated patch on same key** — `find_patch` hit, update `v` in place, list does not grow.
2. **Container → scalar replacement** — `bs/be` span covers the whole nested object/array. Splice replaces it with the encoded scalar. Cached child proxy is cleared by write-step 4; any external reference becomes a detached proxy (acceptable — caller explicitly replaced the slot).
3. **Write then read** — read path short-circuits via `_patches` and returns the patched value.
4. **scalar → different scalar type** — `bs/be` is the value's syntactic span (number digits, or `"…"` for string). Splice replaces with encoded new value. JSON remains valid.
5. **Non-scalar write or absent key** — falls through; any pre-recorded patches are applied into the materialized contents before `setmetatable(t, nil)`, then `_patches` is cleared.
6. **`pairs` then write** — `lazy_object_iter` doesn't mutate the view. Subsequent writes still work normally.
7. **Mixed dirty children + patches** — write-step 4 clears the cached child, so the same key cannot be both. Different keys: walking encoder runs (dirty wins decision), patch lookup is applied per key.
8. **Duplicate keys in source JSON** — `qjson_cursor_field_bytes` returns the first match's value span (matches existing `qjson_cursor_field` behavior). The splice replaces only that occurrence. Acceptable.
9. **`qjson.null` write** — accepted as scalar; `encode(qjson.null)` emits `"null"`.
10. **FFI failure (OOM etc.)** — `check(rc)` raises; view state untouched (patch list not yet modified).

## Tests

### Rust integration — `tests/ffi_cursor_field_bytes.rs` (~80 lines)

| Case | Expectation |
|---|---|
| Simple object, query existing key | rc=OK, `(bs, be)` points to the value bytes |
| String value with escape `{"k":"a\\nb"}` | `(bs, be)` includes both surrounding quotes (syntactic, not decoded) |
| Nested container value `{"k":{"x":1}}` | `(bs, be)` spans the entire `{"x":1}` |
| Missing key | rc=QJSON_NOT_FOUND |
| Array cursor input | rc=QJSON_TYPE_MISMATCH (matches `qjson_cursor_field` behavior) |
| `value_out == NULL` | Still writes `bs/be`, no crash |
| Duplicate keys | Returns the first occurrence |

Plus: add a case to `tests/scanner_crosscheck.rs` proptest so AVX2 and scalar scanners stay in agreement on `qjson_cursor_field_bytes` results.

### Rust panic barrier — `tests/ffi_panic.rs`

Under the `test-panic` cargo feature, add one assertion that calling `qjson_cursor_field_bytes` on a doc whose internals are injected to panic returns `QJSON_OOM` (process does not crash).

### Lua — new `tests/lua/lazy_patch_spec.lua` (~150 lines)

Grouped describes:

- **scalar patches on existing keys** — happy-path matrix (string/number/bool/null/cross-type/container→scalar), repeated writes, read-after-write, `pairs` ordering, `qjson.pairs`, `#view` invariance, encode equivalence, encode whitespace preservation, multiple patches, `tostring`, `materialize`.
- **fall through to materialization** — nil write, table write, new key write, prior patches preserved across fall through.
- **mixed patches and dirty children** — both encoders honor patches.
- **LazyArray unchanged** — array `__newindex` still materializes (regression guard).

### Regression guards — unchanged

`tests/lua/lazy_table_spec.lua` and `tests/lua/cjson_compat_spec.lua` must pass **without modification**. This is an explicit acceptance criterion.

## Benchmarks

Add one new scenario to `benches/lua_bench.lua` reusing `benches/fixtures/github-100k.json`:

```
qjson.decode + modify-N-scalars + encode    (N = 5)
```

Implementation note: pick 5 shallow numeric keys known to exist in the GitHub fixture (e.g. `stargazers_count`, `forks_count`, `open_issues`, `subscribers_count`, `watchers`). Each iteration: decode → 5 assignments → encode → discard.

The existing `decode + encode (unmodified)` and `parse + access fields` scenarios are unchanged and serve as regression evidence.

## Acceptance criteria

| Criterion | How to verify |
|---|---|
| `github-100k parse + access fields` does not regress vs main, ±3% median over 5 runs | `make bench` before/after |
| `decode + encode (unmodified)` ≥ 100 KB does not regress | `make bench` |
| New `decode + modify-5-scalars + encode` ≥ 100 KB improves vs main | `make bench` |
| `lazy_table_spec.lua` and `cjson_compat_spec.lua` pass unchanged | `make test` |
| `lazy_patch_spec.lua` all green | `make test` |
| Lua diff: ≤ +100 net lines in `lua/qjson/table.lua` | `git diff --stat` |
| `cargo test --release --no-default-features` passes (scanner parity) | CI gate 2 |
| `cargo test --features test-panic --release` passes | CI gate 3 |
| `make lint` clean (clippy `-D warnings`) | CI |

## Out-of-scope notes for follow-ups

- **PR-2 (deletion)** would add `_deleted` set + a third encode-tree branch + matching `__index` short-circuit.
- **PR-3 (insertion)** would either require switching cached-child storage to a sidecar (`_child_cache`) or accept that new keys go through full materialization. Defer decision until usage data shows it matters.
- Both are tracked as separate issues, not bundled here.
