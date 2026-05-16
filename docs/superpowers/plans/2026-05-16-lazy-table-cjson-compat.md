# Lazy Table cjson-Compat — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `qd.decode(json) → lazy_table` and `qd.encode(value) → json` so callers can migrate from `cjson` to `quickdecode` with two symbol swaps. Lazy proxies read like `cjson.decode`'d tables; encode walks them with an original-substring fast path for unmodified subtrees.

**Architecture:** Two new Rust FFI helpers (`qjd_cursor_bytes`, `qjd_cursor_object_entry_at`); the rest is a new `lua/quickdecode/table.lua` module with `LazyObject` / `LazyArray` metatables, plus thin re-exports in `lua/quickdecode.lua`. Writes trigger one-level materialization; `qd.encode` switches on lazy-vs-real-table per subtree.

**Tech Stack:** Rust (existing), LuaJIT 2.1 FFI, busted for Lua tests. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-05-16-lazy-table-cjson-compat-design.md`.

---

## File Structure

```
src/
├── ffi.rs                              — modify: add qjd_cursor_bytes, qjd_cursor_object_entry_at
├── doc.rs                              — modify: add helpers for "nth object entry" walk
└── (no other source changes)

include/lua_quick_decode.h              — modify: add two prototypes

lua/
├── quickdecode.lua                     — modify: ffi.cdef gains two lines, top-level re-exports
└── quickdecode/
    └── table.lua                       — create: LazyObject, LazyArray, qd.decode/encode/etc

tests/
├── ffi_cursor_bytes.rs                 — create: Rust integration test for qjd_cursor_bytes
├── ffi_object_iter.rs                  — create: Rust integration test for qjd_cursor_object_entry_at
└── lua/
    └── lazy_table_spec.lua             — create: busted spec for the lazy API

benches/
└── lua_bench.lua                       — modify: add qd.decode-based rows alongside existing qd.parse

README.md                               — modify: usage section for lazy table API; one Roadmap entry
```

The split between `lua/quickdecode.lua` and `lua/quickdecode/table.lua` keeps the existing path-API code untouched in its current file. The new file owns the lazy view, the encoder, and sentinel bridging; the existing file gains two `ffi.cdef` lines and a `require` + re-export at the bottom.

---

## Task 1: New FFI export — qjd_cursor_bytes

**Files:**
- Modify: `src/ffi.rs` (add export at end of "Path-based getters" section)
- Modify: `include/lua_quick_decode.h` (add prototype)
- Test: `tests/ffi_cursor_bytes.rs` (create)

- [ ] **Step 1: Write the failing test**

Create `tests/ffi_cursor_bytes.rs`:

```rust
use std::os::raw::c_int;
use std::ptr;

use quickdecode::ffi::{
    qjd_cursor, qjd_cursor_bytes, qjd_cursor_field, qjd_doc, qjd_free, qjd_open, qjd_parse,
};

unsafe fn open_root(json: &[u8]) -> (*mut qjd_doc, qjd_cursor) {
    let mut err: c_int = -1;
    let doc = qjd_parse(json.as_ptr(), json.len(), &mut err);
    assert!(!doc.is_null());
    let mut cur: qjd_cursor = std::mem::zeroed();
    let rc = qjd_open(doc, ptr::null(), 0, &mut cur);
    assert_eq!(rc, 0);
    (doc, cur)
}

#[test]
fn bytes_of_root_object_covers_full_json() {
    let json = br#"{"a":1,"b":[2,3]}"#;
    unsafe {
        let (doc, cur) = open_root(json);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjd_cursor_bytes(&cur, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], json.as_ref());
        qjd_free(doc);
    }
}

#[test]
fn bytes_of_string_value_is_quoted_span() {
    let json = br#"{"k":"hello"}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjd_cursor = std::mem::zeroed();
        let rc = qjd_cursor_field(&root, b"k".as_ptr() as *const i8, 1, &mut child);
        assert_eq!(rc, 0);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjd_cursor_bytes(&child, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], br#""hello""#);
        qjd_free(doc);
    }
}

#[test]
fn bytes_of_number_value_strips_separators() {
    let json = br#"{"k": 42 ,"x":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut child: qjd_cursor = std::mem::zeroed();
        let rc = qjd_cursor_field(&root, b"k".as_ptr() as *const i8, 1, &mut child);
        assert_eq!(rc, 0);
        let mut bs: usize = 0;
        let mut be: usize = 0;
        let rc = qjd_cursor_bytes(&child, &mut bs, &mut be);
        assert_eq!(rc, 0);
        assert_eq!(&json[bs..be], b"42");
        qjd_free(doc);
    }
}

#[test]
fn bytes_with_null_out_pointer_returns_invalid_arg() {
    let json = br#"{"a":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let rc = qjd_cursor_bytes(&root, ptr::null_mut(), ptr::null_mut());
        assert_eq!(rc, 7); // QJD_INVALID_ARG
        qjd_free(doc);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --release --test ffi_cursor_bytes`
Expected: FAIL — `qjd_cursor_bytes` is not defined.

- [ ] **Step 3: Implement qjd_cursor_bytes**

Add to `src/ffi.rs` after the existing cursor-based getters (after `qjd_cursor_len`):

```rust
/// Write the original-buffer byte range `[byte_start, byte_end)` that the
/// cursor's value occupies. For containers, the range spans the opening
/// bracket through the closing bracket inclusive (so `byte_end` is one past
/// the close char). For scalars, leading and trailing whitespace and
/// surrounding separators are stripped (same convention as `scalar_bytes`).
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a cursor produced by an earlier `qjd_*` call whose
/// document is still alive; `byte_start` and `byte_end` must be non-NULL
/// and writable.
#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_bytes(
    c: *const qjd_cursor, byte_start: *mut usize, byte_end: *mut usize,
) -> c_int {
    ffi_catch!({
        if byte_start.is_null() || byte_end.is_null() {
            return qjd_err::QJD_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let pos = d.indices[cur.idx_start as usize] as usize;
        let lead = match d.buf.get(pos) {
            Some(b) => *b,
            None => return qjd_err::QJD_PARSE_ERROR as c_int,
        };
        match lead {
            b'{' | b'[' | b'"' => {
                // Container or string: span runs from opener to the matching
                // closer, inclusive.
                let end = d.indices[cur.idx_end as usize] as usize;
                if end >= d.buf.len() {
                    return qjd_err::QJD_PARSE_ERROR as c_int;
                }
                *byte_start = pos;
                *byte_end = end + 1;
                qjd_err::QJD_OK as c_int
            }
            _ => {
                // Scalar: reuse scalar_bytes' start-and-end calculation.
                let start = match d.find_scalar_start(cur.idx_start) {
                    Ok(s) => s, Err(e) => return e as c_int,
                };
                let end = d.indices[cur.idx_start as usize] as usize;
                if end < start {
                    return qjd_err::QJD_PARSE_ERROR as c_int;
                }
                let mut e = end;
                while e > start && matches!(d.buf[e - 1], b' '|b'\t'|b'\n'|b'\r') { e -= 1; }
                *byte_start = start;
                *byte_end = e;
                qjd_err::QJD_OK as c_int
            }
        }
    })
}
```

Note on string values: the existing `Cursor` for a string spans `idx_start` (open quote) to `idx_start + 1` (close quote), so `indices[idx_end]` is the close-quote byte. Adding `+1` gives one past the close quote, the correct exclusive end.

- [ ] **Step 4: Add prototype to public C header**

Modify `include/lua_quick_decode.h`. Find the existing `int qjd_cursor_len(...)` line and add the new prototype directly after it:

```c
int qjd_cursor_bytes(const qjd_cursor*, size_t* byte_start, size_t* byte_end);
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --release --test ffi_cursor_bytes`
Expected: PASS — all four tests green.

- [ ] **Step 6: Run the full Rust test gate**

Run: `cargo test --release && cargo test --release --no-default-features`
Expected: PASS for both. No existing test should regress.

- [ ] **Step 7: Commit**

```bash
git add src/ffi.rs include/lua_quick_decode.h tests/ffi_cursor_bytes.rs
git commit -m "feat(ffi): add qjd_cursor_bytes returning original byte span"
```

---

## Task 2: New FFI export — qjd_cursor_object_entry_at

**Files:**
- Modify: `src/doc.rs` (add `nth_object_entry` helper)
- Modify: `src/ffi.rs` (add export)
- Modify: `include/lua_quick_decode.h` (add prototype)
- Test: `tests/ffi_object_iter.rs` (create)

This is what `__pairs` and `__newindex` materialization use to walk an object's children when only the index is known.

- [ ] **Step 1: Write the failing test**

Create `tests/ffi_object_iter.rs`:

```rust
use std::os::raw::c_int;
use std::ptr;

use quickdecode::ffi::{
    qjd_cursor, qjd_cursor_object_entry_at, qjd_doc, qjd_free, qjd_open, qjd_parse,
};

unsafe fn open_root(json: &[u8]) -> (*mut qjd_doc, qjd_cursor) {
    let mut err: c_int = -1;
    let doc = qjd_parse(json.as_ptr(), json.len(), &mut err);
    assert!(!doc.is_null());
    let mut cur: qjd_cursor = std::mem::zeroed();
    qjd_open(doc, ptr::null(), 0, &mut cur);
    (doc, cur)
}

unsafe fn entry_at(root: &qjd_cursor, i: usize) -> (String, qjd_cursor) {
    let mut kp: *const u8 = ptr::null();
    let mut kn: usize = 0;
    let mut vc: qjd_cursor = std::mem::zeroed();
    let rc = qjd_cursor_object_entry_at(root, i, &mut kp, &mut kn, &mut vc);
    assert_eq!(rc, 0, "entry_at({}) failed with rc={}", i, rc);
    let key = std::slice::from_raw_parts(kp, kn);
    (String::from_utf8(key.to_vec()).unwrap(), vc)
}

#[test]
fn three_keys_in_order() {
    let json = br#"{"a":1,"b":"x","c":[2,3]}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let (k0, _) = entry_at(&root, 0);
        let (k1, _) = entry_at(&root, 1);
        let (k2, _) = entry_at(&root, 2);
        assert_eq!(k0, "a");
        assert_eq!(k1, "b");
        assert_eq!(k2, "c");
        qjd_free(doc);
    }
}

#[test]
fn key_with_escape_decodes() {
    // The key `"a\nb"` (3 chars: a, newline, b) — verifies the FFI runs the
    // string-decode scratch path rather than handing back raw escaped bytes.
    let json = b"{\"a\\nb\":1}";
    unsafe {
        let (doc, root) = open_root(json);
        let (k0, _) = entry_at(&root, 0);
        assert_eq!(k0, "a\nb");
        qjd_free(doc);
    }
}

#[test]
fn out_of_range_returns_not_found() {
    let json = br#"{"a":1}"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut kp: *const u8 = ptr::null();
        let mut kn: usize = 0;
        let mut vc: qjd_cursor = std::mem::zeroed();
        let rc = qjd_cursor_object_entry_at(&root, 5, &mut kp, &mut kn, &mut vc);
        assert_eq!(rc, 2); // QJD_NOT_FOUND
        qjd_free(doc);
    }
}

#[test]
fn array_cursor_returns_type_mismatch() {
    let json = br#"[1,2,3]"#;
    unsafe {
        let (doc, root) = open_root(json);
        let mut kp: *const u8 = ptr::null();
        let mut kn: usize = 0;
        let mut vc: qjd_cursor = std::mem::zeroed();
        let rc = qjd_cursor_object_entry_at(&root, 0, &mut kp, &mut kn, &mut vc);
        assert_eq!(rc, 3); // QJD_TYPE_MISMATCH
        qjd_free(doc);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --release --test ffi_object_iter`
Expected: FAIL — `qjd_cursor_object_entry_at` is not defined.

- [ ] **Step 3: Implement helper on Document**

Add to `src/doc.rs` after `cursor_len`:

```rust
    /// Find the i-th key/value entry of an object cursor. Returns the
    /// indices position of the key (so the caller can decode it via the
    /// existing string-decode path) and the value's `Cursor`.
    ///
    /// Returns `QJD_TYPE_MISMATCH` for non-object cursors, `QJD_NOT_FOUND`
    /// when `i` is past the end.
    pub(crate) fn nth_object_entry(&self, cur: Cursor, n: usize) -> Result<(u32, Cursor), qjd_err> {
        let pos = self.indices[cur.idx_start as usize] as usize;
        let b = *self.buf.get(pos).ok_or(qjd_err::QJD_PARSE_ERROR)?;
        if b != b'{' {
            return Err(qjd_err::QJD_TYPE_MISMATCH);
        }
        // Mirror cursor_len's walk, but stop at the n-th child rather than counting.
        let closer_pos = self.indices[cur.idx_end as usize] as usize;
        let mut p = pos + 1;
        while p < closer_pos && matches!(self.buf[p], b' '|b'\t'|b'\n'|b'\r') {
            p += 1;
        }
        if p == closer_pos {
            return Err(qjd_err::QJD_NOT_FOUND);
        }
        let mut i = cur.idx_start + 1;
        let end = cur.idx_end;
        let mut count: usize = 0;
        loop {
            // For objects, the key occupies indices[i..=i+1] (open & close quote);
            // the value cursor starts at i+3 (after the colon at i+2).
            let key_idx_start = i;
            let value_idx_start = i + 3;
            let (cursor_end, skip_end) = crate::cursor::find_value_span(self, value_idx_start)?;
            if count == n {
                return Ok((key_idx_start, Cursor { idx_start: value_idx_start, idx_end: cursor_end }));
            }
            count += 1;
            let after_pos = self.indices[skip_end as usize] as usize;
            if after_pos >= self.buf.len() { return Err(qjd_err::QJD_PARSE_ERROR); }
            match self.buf[after_pos] {
                b',' => {
                    i = skip_end + 1;
                    if i > end { return Err(qjd_err::QJD_NOT_FOUND); }
                }
                b'}' => return Err(qjd_err::QJD_NOT_FOUND),
                _ => return Err(qjd_err::QJD_PARSE_ERROR),
            }
        }
    }
```

Add to `src/ffi.rs` after `qjd_cursor_bytes`:

```rust
/// Write the i-th object entry's key (decoded into the doc's scratch
/// buffer) and value cursor into the out parameters.
///
/// Returns `QJD_TYPE_MISMATCH` when the cursor is not an object, or
/// `QJD_NOT_FOUND` when `i` is past the end.
///
/// # Safety
///
/// See the module-level [shared safety contract](self#shared-safety-contract).
/// `c` must point to a live cursor; `key_ptr`, `key_len`, and `value_out`
/// must be non-NULL and writable. The `(*key_ptr, *key_len)` pair is
/// invalidated by the next `qjd_get_str` / `qjd_cursor_get_str` /
/// `qjd_cursor_object_entry_at` call on the same document (scratch reuse).
#[no_mangle]
pub unsafe extern "C" fn qjd_cursor_object_entry_at(
    c: *const qjd_cursor, i: usize,
    key_ptr: *mut *const u8, key_len: *mut usize,
    value_out: *mut qjd_cursor,
) -> c_int {
    ffi_catch!({
        if key_ptr.is_null() || key_len.is_null() || value_out.is_null() {
            return qjd_err::QJD_INVALID_ARG as c_int;
        }
        let (d, cur) = match cursor_to_internal(c) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        let (key_idx_start, value_cur) = match d.nth_object_entry(cur, i) {
            Ok(x) => x, Err(e) => return e as c_int,
        };
        // Decode the key: it sits at indices[key_idx_start..=key_idx_start+1]
        // — open quote at key_idx_start, close quote at key_idx_start+1.
        let open_pos = d.indices[key_idx_start as usize] as usize;
        let close_pos = d.indices[(key_idx_start + 1) as usize] as usize;
        let mut scratch = d.scratch.borrow_mut();
        match string::decode_string(d.buf, open_pos + 1, close_pos, &mut scratch) {
            Ok((p, n)) => {
                *key_ptr = p;
                *key_len = n;
                *value_out = internal_to_cursor((*c).doc, value_cur);
                qjd_err::QJD_OK as c_int
            }
            Err(e) => e as c_int,
        }
    })
}
```

(`internal_to_cursor` and `string` are already imported at the top of the file. If not, add `use crate::decode::string;`.)

- [ ] **Step 4: Add prototype to public C header**

Modify `include/lua_quick_decode.h` after the `qjd_cursor_bytes` line:

```c
int qjd_cursor_object_entry_at(const qjd_cursor*, size_t i,
                                const uint8_t** key_ptr, size_t* key_len,
                                qjd_cursor* value_out);
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --release --test ffi_object_iter`
Expected: PASS.

- [ ] **Step 6: Re-run the full Rust test gate**

Run: `cargo test --release && cargo test --release --no-default-features`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/doc.rs src/ffi.rs include/lua_quick_decode.h tests/ffi_object_iter.rs
git commit -m "feat(ffi): add qjd_cursor_object_entry_at for object iteration"
```

---

## Task 3: Lua module skeleton + sentinel bridging

**Files:**
- Modify: `lua/quickdecode.lua` (add two ffi.cdef lines)
- Create: `lua/quickdecode/table.lua` (skeleton)
- Test: smoke via `luajit -e` (busted suite added in later tasks)

- [ ] **Step 1: Extend FFI cdef in lua/quickdecode.lua**

In `lua/quickdecode.lua`, find the closing `]]` of the existing `ffi.cdef[[ ... ]]` block. Just before that closing `]]`, insert:

```c
int qjd_cursor_bytes(const qjd_cursor*, size_t* byte_start, size_t* byte_end);
int qjd_cursor_object_entry_at(const qjd_cursor*, size_t i,
                                const uint8_t** key_ptr, size_t* key_len,
                                qjd_cursor* value_out);
```

- [ ] **Step 2: Create the table-module skeleton**

Create `lua/quickdecode/table.lua`:

```lua
-- Lazy table view + cjson-compatible encoder for quickdecode.
--
-- This module relies on the FFI cdef set up by `lua/quickdecode.lua`, so
-- callers must `require("quickdecode")` (transitively or directly) before
-- they require this module.

local ffi = require("ffi")
local C   = ffi.load("quickdecode")

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

return _M
```

- [ ] **Step 3: Smoke-test the module loads**

Run from the repo root:

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
require("quickdecode")              -- triggers ffi.cdef
local qt = require("quickdecode.table")
print("ok null:", qt.null, "empty_array_mt:", qt.empty_array_mt)
print("equals cjson.null:", qt.null == require("cjson").null)
'
```

Expected output ends with `equals cjson.null: true` (assuming cjson is installed; if not, prints `false` and uses the fallback — still a valid run).

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode.lua lua/quickdecode/table.lua
git commit -m "feat(lua): skeleton for quickdecode.table + sentinel bridge"
```

---

## Task 4: LazyObject metatable — __index for scalars

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Test: `tests/lua/lazy_table_spec.lua` (create)

- [ ] **Step 1: Write the failing test**

Create `tests/lua/lazy_table_spec.lua`:

```lua
local qd = require("quickdecode")
local qt = require("quickdecode.table")

describe("LazyObject __index — scalars", function()
    it("reads a string field", function()
        local t = qt.decode('{"k":"hello"}')
        assert.are.equal("hello", t.k)
    end)

    it("reads a number field", function()
        local t = qt.decode('{"n":42.5}')
        assert.are.equal(42.5, t.n)
    end)

    it("reads a boolean field", function()
        local t = qt.decode('{"b":true,"c":false}')
        assert.is_true(t.b)
        assert.is_false(t.c)
    end)

    it("returns nil for missing key", function()
        local t = qt.decode('{"a":1}')
        assert.is_nil(t.missing)
    end)
end)
```

- [ ] **Step 2: Verify it fails**

Test runner not on dev machine; smoke via:

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
require("quickdecode")
local qt = require("quickdecode.table")
local ok, err = pcall(qt.decode, [[{"a":1}]])
print(ok, err)
'
```

Expected: `false ...quickdecode/table.lua: attempt to call field 'decode' (a nil value)`.

- [ ] **Step 3: Implement decode + LazyObject __index for scalars**

Append to `lua/quickdecode/table.lua` (before `return _M`):

```lua
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
local cur_box  = ffi.new("qjd_cursor[1]")
local sz_a     = ffi.new("size_t[1]")
local sz_b     = ffi.new("size_t[1]")

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
    local rc = C.qjd_cursor_field(self._cur, key, #key, cur_box)
    if not check(rc) then return nil end
    local child = cur_box[0]
    local trc = C.qjd_cursor_typeof(child, "", 0, type_box)
    if not check(trc) then return nil end
    local t = type_box[0]
    if t == T_STR then
        local rrc = C.qjd_cursor_get_str(child, "", 0, strp_box, size_box)
        if not check(rrc) then return nil end
        return ffi.string(strp_box[0], size_box[0])
    elseif t == T_NUM then
        local rrc = C.qjd_cursor_get_f64(child, "", 0, f64_box)
        if not check(rrc) then return nil end
        return f64_box[0]
    elseif t == T_BOOL then
        local rrc = C.qjd_cursor_get_bool(child, "", 0, bool_box)
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
    -- Open the root cursor.
    local rc = C.qjd_open(doc._ptr, "", 0, cur_box)
    if not check(rc) then
        error("quickdecode: open root failed")
    end
    local root = cur_box[0]
    -- Determine root container kind (object/array) and wrap accordingly.
    -- Both have meaningful byte spans for encode.
    local trc = C.qjd_cursor_typeof(root, "", 0, type_box)
    check(trc)
    local rt = type_box[0]
    local brc = C.qjd_cursor_bytes(root, sz_a, sz_b)
    check(brc)
    local view = {
        _doc = doc,
        _cur = root,
        _bs  = tonumber(sz_a[0]),
        _be  = tonumber(sz_b[0]),
    }
    if rt == T_OBJ then
        return setmetatable(view, LazyObject)
    elseif rt == T_ARR then
        return setmetatable(view, LazyArray)
    else
        error("quickdecode: top-level JSON value is not an object or array")
    end
end
```

Also at the top of the file, alongside the other requires, add:

```lua
local qd = require("quickdecode")
```

(`qd.parse` is used inside `_M.decode`.)

- [ ] **Step 4: Smoke-verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
local t = qt.decode([[{"k":"hello","n":42.5,"b":true,"c":false}]])
print(t.k, t.n, t.b, t.c, t.missing)
'
```

Expected: `hello   42.5    true    false   nil`.

- [ ] **Step 5: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): LazyObject __index for scalar fields"
```

---

## Task 5: LazyObject __index — nested containers return a proxy

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("LazyObject __index — nested containers", function()
    it("returns a LazyObject for a nested object", function()
        local t = qt.decode('{"a":{"b":"x"}}')
        local inner = t.a
        assert.is_table(inner)
        assert.are.equal("x", inner.b)
    end)

    it("returns a LazyArray for a nested array", function()
        local t = qt.decode('{"xs":[10,20]}')
        local xs = t.xs
        assert.is_table(xs)
        -- LazyArray __index is added in a later task; just verify it's
        -- a table-typed value at this stage.
    end)
end)
```

- [ ] **Step 2: Verify it fails**

Smoke:

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
local t = qt.decode([[{"a":{"b":"x"}}]])
print(type(t.a), t.a and t.a.b)
'
```

Expected: `nil  nil` (current scalar-only `read_object_field` returns nil for containers).

- [ ] **Step 3: Wrap container children in a proxy**

In `lua/quickdecode/table.lua`, extract a reusable "wrap child" helper and use it from `read_object_field`. Add this function near the top, above `read_object_field`:

```lua
local function wrap_child(parent_view, child_cursor)
    -- Caller has already classified child_cursor's type. This helper builds
    -- a new lazy view sharing the same _doc as the parent.
    C.qjd_cursor_bytes(child_cursor, sz_a, sz_b)
    return {
        _doc = parent_view._doc,
        _cur = child_cursor,
        _bs  = tonumber(sz_a[0]),
        _be  = tonumber(sz_b[0]),
    }
end
```

Replace the `return nil` at the end of `read_object_field` with container dispatch:

```lua
    if t == T_OBJ then
        return setmetatable(wrap_child(self, child), LazyObject)
    elseif t == T_ARR then
        return setmetatable(wrap_child(self, child), LazyArray)
    end
    return nil
```

Note: `child` here is the local variable already assigned at the top of `read_object_field` (`local child = cur_box[0]`).

- [ ] **Step 4: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
local t = qt.decode([[{"a":{"b":"x"},"xs":[10,20]}]])
print(type(t.a), t.a.b)
print(type(t.xs))
'
```

Expected: `table   x` and `table` on the second line.

- [ ] **Step 5: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): wrap nested containers as Lazy proxies"
```

---

## Task 6: LazyArray __index — integer keys

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("LazyArray __index", function()
    it("reads scalar elements by integer index (1-based)", function()
        local t = qt.decode('[10,"x",true,null]')
        assert.are.equal(10, t[1])
        assert.are.equal("x", t[2])
        assert.is_true(t[3])
        assert.are.equal(qt.null, t[4])
    end)

    it("returns nil for out-of-range index", function()
        local t = qt.decode('[1,2,3]')
        assert.is_nil(t[0])
        assert.is_nil(t[4])
    end)

    it("returns nil for non-integer key", function()
        local t = qt.decode('[1,2,3]')
        assert.is_nil(t.foo)
        assert.is_nil(t[1.5])
    end)

    it("returns a nested LazyObject", function()
        local t = qt.decode('[{"a":1},{"a":2}]')
        assert.are.equal(1, t[1].a)
        assert.are.equal(2, t[2].a)
    end)
end)
```

- [ ] **Step 2: Smoke-verify it fails**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
local t = qt.decode([[[10,"x",true]]])
print(t[1], t[2], t[3])
'
```

Expected: `nil  nil  nil`.

- [ ] **Step 3: Implement LazyArray.__index**

In `lua/quickdecode/table.lua`, add this read function (the structure mirrors `read_object_field` but uses `qjd_cursor_index` and a 1-based-to-0-based conversion):

```lua
local function read_array_index(self, key)
    if type(key) ~= "number" then return nil end
    -- 1-based external, 0-based internal
    local i = key - 1
    if i < 0 or i ~= math.floor(i) then return nil end
    local rc = C.qjd_cursor_index(self._cur, i, cur_box)
    if not check(rc) then return nil end
    local child = cur_box[0]
    local trc = C.qjd_cursor_typeof(child, "", 0, type_box)
    if not check(trc) then return nil end
    local t = type_box[0]
    if t == T_STR then
        local rrc = C.qjd_cursor_get_str(child, "", 0, strp_box, size_box)
        if not check(rrc) then return nil end
        return ffi.string(strp_box[0], size_box[0])
    elseif t == T_NUM then
        local rrc = C.qjd_cursor_get_f64(child, "", 0, f64_box)
        if not check(rrc) then return nil end
        return f64_box[0]
    elseif t == T_BOOL then
        local rrc = C.qjd_cursor_get_bool(child, "", 0, bool_box)
        if not check(rrc) then return nil end
        return bool_box[0] ~= 0
    elseif t == T_NULL then
        return _M.null
    elseif t == T_OBJ then
        return setmetatable(wrap_child(self, child), LazyObject)
    elseif t == T_ARR then
        return setmetatable(wrap_child(self, child), LazyArray)
    end
    return nil
end

LazyArray.__index = read_array_index
```

This duplicates a lot of `read_object_field`; that's intentional — extracting a shared "decode-by-cursor" helper is on the menu for refactoring once `__pairs` lands and shows the third caller. Premature factoring would obscure the read-path branching at this stage.

- [ ] **Step 4: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
local t = qt.decode([[[10,"x",true,null]]])
print(t[1], t[2], t[3], t[4] == qt.null)
print(t[0], t[4], t[1.5])
local t2 = qt.decode([[[{"a":1},{"a":2}]]])
print(t2[1].a, t2[2].a)
'
```

Expected:
```
10      x       true    true
nil     nil     nil
1       2
```

- [ ] **Step 5: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): LazyArray __index for integer keys"
```

---

## Task 7: __len for both Lazy metatables

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("__len", function()
    it("counts object keys", function()
        local t = qt.decode('{"a":1,"b":2,"c":3}')
        assert.are.equal(3, #t)
    end)

    it("counts array elements", function()
        local t = qt.decode('[10,20,30,40]')
        assert.are.equal(4, #t)
    end)

    it("returns 0 for empty containers", function()
        assert.are.equal(0, #qt.decode('{}'))
        assert.are.equal(0, #qt.decode('[]'))
    end)
end)
```

- [ ] **Step 2: Implement __len**

In `lua/quickdecode/table.lua`, add:

```lua
local function lazy_len(self)
    local rc = C.qjd_cursor_len(self._cur, "", 0, size_box)
    check(rc)
    return tonumber(size_box[0])
end

LazyObject.__len = lazy_len
LazyArray.__len  = lazy_len
```

- [ ] **Step 3: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
print(#qt.decode([[{"a":1,"b":2,"c":3}]]))
print(#qt.decode([[[10,20,30,40]]]))
print(#qt.decode([[{}]]), #qt.decode([[[]]]))
'
```

Expected: `3`, `4`, `0  0`.

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): __len for LazyObject and LazyArray"
```

---

## Task 8: __pairs for LazyObject (LJ52) and qd.pairs wrapper

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("__pairs / qd.pairs over LazyObject", function()
    it("iterates string keys in source order", function()
        local t = qt.decode('{"a":1,"b":2,"c":3}')
        local keys = {}
        local values = {}
        for k, v in qt.pairs(t) do
            keys[#keys+1] = k
            values[#values+1] = v
        end
        assert.are.same({"a","b","c"}, keys)
        assert.are.same({1, 2, 3}, values)
    end)

    it("returns nested containers as lazy proxies, not materialized", function()
        local t = qt.decode('{"a":{"x":1}}')
        for _, v in qt.pairs(t) do
            assert.is_table(v)
            assert.are.equal(1, v.x)
        end
    end)

    it("handles empty object", function()
        local count = 0
        for _ in qt.pairs(qt.decode('{}')) do count = count + 1 end
        assert.are.equal(0, count)
    end)
end)
```

- [ ] **Step 2: Implement __pairs and qd.pairs**

In `lua/quickdecode/table.lua`, add a generic value-from-cursor helper (which factors out the type switch that `read_object_field`, `read_array_index`, and now the iterator all need):

```lua
-- Decode a single cursor into a Lua value, recursing for containers.
-- The parent_view supplies _doc for child wrapping.
local function decode_cursor(parent_view, child)
    local trc = C.qjd_cursor_typeof(child, "", 0, type_box)
    if not check(trc) then return nil end
    local t = type_box[0]
    if t == T_STR then
        local rrc = C.qjd_cursor_get_str(child, "", 0, strp_box, size_box)
        if not check(rrc) then return nil end
        return ffi.string(strp_box[0], size_box[0])
    elseif t == T_NUM then
        local rrc = C.qjd_cursor_get_f64(child, "", 0, f64_box)
        if not check(rrc) then return nil end
        return f64_box[0]
    elseif t == T_BOOL then
        local rrc = C.qjd_cursor_get_bool(child, "", 0, bool_box)
        if not check(rrc) then return nil end
        return bool_box[0] ~= 0
    elseif t == T_NULL then
        return _M.null
    elseif t == T_OBJ then
        return setmetatable(wrap_child(parent_view, child), LazyObject)
    elseif t == T_ARR then
        return setmetatable(wrap_child(parent_view, child), LazyArray)
    end
    return nil
end
```

Refactor `read_object_field` and `read_array_index` to call `decode_cursor`. Replace each function's tail (the `if t == T_STR ... return nil end` block) with `return decode_cursor(self, child)`.

Then add the iterator:

```lua
local function lazy_object_iter(state, _prev_key)
    local i = state.i
    state.i = i + 1
    local rc = C.qjd_cursor_object_entry_at(
        state.view._cur, i, strp_box, size_box, cur_box
    )
    if rc == QJD_NOT_FOUND then return nil end
    check(rc)
    local k = ffi.string(strp_box[0], size_box[0])
    local v = decode_cursor(state.view, cur_box[0])
    return k, v
end

LazyObject.__pairs = function(self)
    return lazy_object_iter, { view = self, i = 0 }, nil
end

function _M.pairs(t)
    local mt = getmetatable(t)
    if mt == LazyObject then
        return LazyObject.__pairs(t)
    elseif mt == LazyArray then
        -- Arrays iterate via ipairs semantics; for compatibility with
        -- cjson semantics, pairs() on a JSON array yields integer indices.
        return _M.ipairs(t)
    end
    return pairs(t)  -- fall through for plain Lua tables
end
```

(`_M.ipairs` is added in Task 9; the `pairs` here references it only inside the function body, so order doesn't matter at module load.)

- [ ] **Step 3: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
local t = qt.decode([[{"a":1,"b":2,"c":3}]])
for k, v in qt.pairs(t) do print(k, v) end
print("---")
local t2 = qt.decode([[{"a":{"x":1}}]])
for k, v in qt.pairs(t2) do print(k, type(v), v.x) end
'
```

Expected:
```
a       1
b       2
c       3
---
a       table   1
```

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): __pairs/qd.pairs for LazyObject + factor decode_cursor"
```

---

## Task 9: __ipairs for LazyArray and qd.ipairs wrapper

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("__ipairs / qd.ipairs over LazyArray", function()
    it("iterates elements 1..n in order", function()
        local t = qt.decode('[10,20,30]')
        local got = {}
        for i, v in qt.ipairs(t) do got[i] = v end
        assert.are.same({10,20,30}, got)
    end)

    it("yields lazy proxies for nested containers", function()
        local t = qt.decode('[{"a":1},{"a":2}]')
        local seen = {}
        for _, v in qt.ipairs(t) do
            assert.is_table(v)
            seen[#seen+1] = v.a
        end
        assert.are.same({1, 2}, seen)
    end)

    it("handles empty array", function()
        local count = 0
        for _ in qt.ipairs(qt.decode('[]')) do count = count + 1 end
        assert.are.equal(0, count)
    end)
end)
```

- [ ] **Step 2: Implement ipairs**

In `lua/quickdecode/table.lua`, add:

```lua
local function lazy_array_iter(state, _prev_i)
    local i = state.i
    local rc = C.qjd_cursor_index(state.view._cur, i, cur_box)
    if rc == QJD_NOT_FOUND then return nil end
    check(rc)
    state.i = i + 1
    local v = decode_cursor(state.view, cur_box[0])
    return i + 1, v   -- external index is 1-based
end

LazyArray.__ipairs = function(self)
    return lazy_array_iter, { view = self, i = 0 }, 0
end

function _M.ipairs(t)
    local mt = getmetatable(t)
    if mt == LazyArray then
        return LazyArray.__ipairs(t)
    end
    return ipairs(t)
end
```

- [ ] **Step 3: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
for i, v in qt.ipairs(qt.decode([[[10,20,30]]])) do print(i, v) end
print("---")
for i, v in qt.ipairs(qt.decode([[[{"a":1},{"a":2}]]])) do print(i, type(v), v.a) end
'
```

Expected:
```
1       10
2       20
3       30
---
1       table   1
2       table   2
```

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): __ipairs/qd.ipairs for LazyArray"
```

---

## Task 10: __newindex — first-write materialization

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("__newindex — first-write materialization", function()
    it("converts LazyObject into a plain table preserving existing keys", function()
        local t = qt.decode('{"a":1,"b":2}')
        t.c = 3
        -- After materialization, metatable is gone, so direct access by key
        -- goes through rawget (no FFI).
        assert.is_nil(getmetatable(t))
        assert.are.equal(1, t.a)
        assert.are.equal(2, t.b)
        assert.are.equal(3, t.c)
    end)

    it("nested containers remain lazy after parent materialization", function()
        local t = qt.decode('{"inner":{"x":1}}')
        t.extra = "y"
        assert.is_nil(getmetatable(t))
        local inner = t.inner
        assert.are.equal(qt._LazyObject, getmetatable(inner))   -- still a lazy proxy
        assert.are.equal(1, inner.x)
    end)

    it("LazyArray materializes preserving empty_array_mt", function()
        local t = qt.decode('[]')
        t[1] = "x"
        assert.are.equal(qt.empty_array_mt, getmetatable(t))
        assert.are.equal("x", t[1])
    end)

    it("simple write leaves other keys intact", function()
        local t = qt.decode('{"a":1}')
        t.b = 2
        assert.are.equal(1, t.a)
        assert.are.equal(2, t.b)
    end)
end)
```

The test compares the metatable against `qt._LazyObject` — an underscore-prefixed export of the implementation-private `LazyObject` for test inspection. Add it to `lua/quickdecode/table.lua`, just before `return _M`:

```lua
_M._LazyObject = LazyObject
_M._LazyArray  = LazyArray
```

- [ ] **Step 2: Implement materialization**

In `lua/quickdecode/table.lua`, add helpers and `__newindex`:

```lua
-- Walk a LazyObject's direct children and return a sequence of (key, value)
-- pairs, with nested containers still as Lazy proxies.
local function materialize_object_contents(view)
    local i = 0
    local pairs_out = {}
    while true do
        local rc = C.qjd_cursor_object_entry_at(view._cur, i, strp_box, size_box, cur_box)
        if rc == QJD_NOT_FOUND then break end
        check(rc)
        local k = ffi.string(strp_box[0], size_box[0])
        local v = decode_cursor(view, cur_box[0])
        pairs_out[#pairs_out+1] = {k, v}
        i = i + 1
    end
    return pairs_out
end

-- Same for arrays: returns a 1-indexed array of values.
local function materialize_array_contents(view)
    local i = 0
    local out = {}
    while true do
        local rc = C.qjd_cursor_index(view._cur, i, cur_box)
        if rc == QJD_NOT_FOUND then break end
        check(rc)
        out[i + 1] = decode_cursor(view, cur_box[0])
        i = i + 1
    end
    return out
end

LazyObject.__newindex = function(t, k, v)
    local contents = materialize_object_contents(t)   -- builds a temporary
    -- Wipe lazy-view fields (they shouldn't survive as object keys).
    t._doc, t._cur, t._bs, t._be = nil, nil, nil, nil
    setmetatable(t, nil)
    for _, kv in ipairs(contents) do
        rawset(t, kv[1], kv[2])
    end
    rawset(t, k, v)
end

LazyArray.__newindex = function(t, k, v)
    local contents = materialize_array_contents(t)
    t._doc, t._cur, t._bs, t._be = nil, nil, nil, nil
    setmetatable(t, _M.empty_array_mt)
    for i, x in ipairs(contents) do
        rawset(t, i, x)
    end
    rawset(t, k, v)
end
```

The temporary-collect-then-swap pattern means an error inside `materialize_*_contents` raises before any mutation to `t` happens, preserving the atomicity invariant from the spec.

- [ ] **Step 3: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
local t = qt.decode([[{"a":1,"b":2}]])
t.c = 3
print(getmetatable(t), t.a, t.b, t.c)
local t2 = qt.decode([[{"inner":{"x":1}}]])
t2.extra = "y"
print(getmetatable(t2), getmetatable(t2.inner) == qt._LazyObject, t2.inner.x)
local arr = qt.decode([[[]]])
arr[1] = "x"
print(getmetatable(arr) == qt.empty_array_mt, arr[1])
'
```

Expected (approximately):
```
nil     1       2       3
nil     true    1
true    x
```

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): __newindex materializes affected level only"
```

---

## Task 11: qd.materialize — recursive deep conversion

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("qt.materialize", function()
    it("converts a LazyObject and its nested containers into real tables", function()
        local m = qt.materialize(qt.decode('{"a":1,"b":{"c":[10,20]}}'))
        assert.is_nil(getmetatable(m))
        assert.are.equal(1, m.a)
        assert.is_nil(getmetatable(m.b))
        assert.are.equal(10, m.b.c[1])
        assert.are.equal(20, m.b.c[2])
    end)

    it("tags empty arrays with empty_array_mt", function()
        local m = qt.materialize(qt.decode('[]'))
        assert.are.equal(qt.empty_array_mt, getmetatable(m))
    end)

    it("preserves cjson.null", function()
        local m = qt.materialize(qt.decode('{"x":null}'))
        assert.are.equal(qt.null, m.x)
    end)

    it("passes through scalars and plain tables unchanged", function()
        assert.are.equal(42, qt.materialize(42))
        assert.are.equal("hi", qt.materialize("hi"))
        local raw = {1, 2, 3}
        assert.are.equal(raw, qt.materialize(raw))
    end)
end)
```

- [ ] **Step 2: Implement materialize**

In `lua/quickdecode/table.lua`:

```lua
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
```

- [ ] **Step 3: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
local m = qt.materialize(qt.decode([[{"a":1,"b":{"c":[10,20]}}]]))
print(m.a, m.b.c[1], m.b.c[2], getmetatable(m), getmetatable(m.b))
print(getmetatable(qt.materialize(qt.decode([[[]]]))) == qt.empty_array_mt)
print(qt.materialize(qt.decode([[{"x":null}]])).x == qt.null)
'
```

Expected:
```
1       10      20      nil     nil
true
true
```

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): qd.materialize for deep conversion to plain tables"
```

---

## Task 12: qd.encode for lazy proxies (substring fast path)

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("qd.encode — lazy proxy substring fast path", function()
    it("re-emits the original JSON for an unmodified LazyObject", function()
        local src = '{"a":1,"b":[2,3],"c":"x"}'
        local t = qt.decode(src)
        assert.are.equal(src, qt.encode(t))
    end)

    it("re-emits the original JSON for an unmodified LazyArray", function()
        local src = '[10,20,{"k":"v"}]'
        local t = qt.decode(src)
        assert.are.equal(src, qt.encode(t))
    end)

    it("trims leading/trailing whitespace at the boundary", function()
        local src = '  {"a":1}  '
        local t = qt.decode(src)
        -- byte span is the value, not its outer whitespace.
        assert.are.equal('{"a":1}', qt.encode(t))
    end)
end)
```

- [ ] **Step 2: Implement encode (proxy branch only — scalars and real tables in next tasks)**

In `lua/quickdecode/table.lua`:

```lua
local function encode_proxy(t)
    -- Slice the original buffer; _hold pins the bytes alive.
    return t._doc._hold:sub(t._bs + 1, t._be)
end

local function encode(v)
    local mt = (type(v) == "table") and getmetatable(v) or nil
    if mt == LazyObject or mt == LazyArray then
        return encode_proxy(v)
    end
    -- Scalar and real-table branches added in subsequent tasks.
    error("qd.encode: unsupported value type at this stage")
end

_M.encode = encode

-- Debug convenience: tostring(lazy_view) returns the original JSON bytes.
-- Not the canonical encoder — callers should still use qd.encode for output.
LazyObject.__tostring = encode_proxy
LazyArray.__tostring  = encode_proxy
```

- [ ] **Step 3: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
print(qt.encode(qt.decode([[{"a":1,"b":[2,3]}]])))
print(qt.encode(qt.decode([[[10,20,30]]])))
print("|" .. qt.encode(qt.decode([[  {"a":1}  ]])) .. "|")
'
```

Expected:
```
{"a":1,"b":[2,3]}
[10,20,30]
|{"a":1}|
```

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): qd.encode proxy fast path (original substring)"
```

---

## Task 13: qd.encode — scalars

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("qd.encode — scalars", function()
    it("encodes strings with JSON escapes", function()
        assert.are.equal('"hello"', qt.encode("hello"))
        assert.are.equal('"a\\nb"', qt.encode("a\nb"))
        assert.are.equal('"a\\\"b"', qt.encode('a"b'))
        assert.are.equal('"a\\\\b"', qt.encode("a\\b"))
    end)

    it("encodes booleans", function()
        assert.are.equal("true", qt.encode(true))
        assert.are.equal("false", qt.encode(false))
    end)

    it("encodes numbers", function()
        assert.are.equal("42", qt.encode(42))
        assert.are.equal("-3.14", qt.encode(-3.14))
    end)

    it("encodes qt.null as JSON null", function()
        assert.are.equal("null", qt.encode(qt.null))
    end)

    it("errors on unsupported values", function()
        assert.has_error(function() qt.encode(function() end) end)
    end)
end)
```

- [ ] **Step 2: Implement scalar encoding**

In `lua/quickdecode/table.lua`, add a string-escape helper and extend `encode`:

```lua
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
    -- Match cjson default: integer-looking numbers without decimal point.
    if n ~= n or n == math.huge or n == -math.huge then
        error("qd.encode: cannot encode non-finite number")
    end
    if n == math.floor(n) and math.abs(n) < 1e15 then
        return string_format("%d", n)
    end
    return string_format("%.14g", n)
end
```

Replace the `encode` function body. Use this version:

```lua
local function encode(v)
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
        -- Real-table branch added in the next task.
        error("qd.encode: real-table encoding not yet implemented")
    end
    error("qd.encode: unsupported value type: " .. tv)
end

_M.encode = encode
```

- [ ] **Step 3: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qt = require("quickdecode.table")
print(qt.encode("hello"))
print(qt.encode("a\nb"))
print(qt.encode(true), qt.encode(false))
print(qt.encode(42), qt.encode(-3.14))
print(qt.encode(qt.null))
local ok, err = pcall(qt.encode, function() end)
print(ok, err)
'
```

Expected:
```
"hello"
"a\nb"
true    false
42      -3.14
null
false   ...qd.encode: unsupported value type: function
```

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): qd.encode scalars (string/number/bool/null)"
```

---

## Task 14: qd.encode — real (and mixed) tables

**Files:**
- Modify: `lua/quickdecode/table.lua`
- Modify: `tests/lua/lazy_table_spec.lua`

- [ ] **Step 1: Add the failing test**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
describe("qd.encode — real and mixed tables", function()
    it("encodes a real Lua object", function()
        -- Build {"a":1,"b":"x"} via plain Lua. Key order matters for the
        -- assertion; cjson and qd.encode both walk via lua_next which is
        -- implementation-defined, so we re-parse for structural equality.
        local cjson = require("cjson")
        local s = qt.encode({a = 1, b = "x"})
        assert.are.same({a = 1, b = "x"}, cjson.decode(s))
    end)

    it("encodes a real Lua array", function()
        assert.are.equal("[1,2,3]", qt.encode({1,2,3}))
    end)

    it("encodes a hand-built empty array with empty_array_mt", function()
        local arr = setmetatable({}, qt.empty_array_mt)
        assert.are.equal("[]", qt.encode(arr))
    end)

    it("encodes mixed lazy + materialized", function()
        local t = qt.decode('{"keep":{"x":1},"changed":{"y":2}}')
        t.changed = "now a string"
        -- After this:  t  is real, t.keep is still lazy.
        local out = qt.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.same({x=1}, parsed.keep)
        assert.are.equal("now a string", parsed.changed)
    end)
end)
```

- [ ] **Step 2: Implement real-table encoding**

In `lua/quickdecode/table.lua`, add array detection and object encoding:

```lua
-- Decide whether a plain Lua table should serialize as a JSON array or object.
-- Matches cjson's default rule: empty_array_mt → array; otherwise, if every
-- key is an integer in 1..n where n = #t, it's an array; otherwise object.
local function is_array(t)
    local mt = getmetatable(t)
    if mt == _M.empty_array_mt then return true end
    local n = #t
    -- Quick check: if there is any non-integer or out-of-range key, it's an object.
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
            error("qd.encode: object key must be a string, got " .. type(k))
        end
        parts[#parts+1] = encode_string(k) .. ":" .. encode(v)
    end
    return "{" .. table.concat(parts, ",") .. "}"
end
```

Note: `encode` is referenced before its forward declaration; in Lua, that's resolved at call time, so as long as `encode` is defined in the same chunk before the encode call actually runs, this is fine. (It is — `_M.encode = encode` happens later in the file.)

Replace the placeholder `error("qd.encode: real-table encoding not yet implemented")` in the existing `encode` function with:

```lua
        if is_array(v) then
            return encode_array(v)
        end
        return encode_object(v)
```

- [ ] **Step 3: Verify it passes**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local cjson = require("cjson")
local qt = require("quickdecode.table")
print(qt.encode({1,2,3}))
print(qt.encode(setmetatable({}, qt.empty_array_mt)))
local t = qt.decode([[{"keep":{"x":1},"changed":{"y":2}}]])
t.changed = "now"
print(qt.encode(t))
'
```

Expected (last line modulo key order):
```
[1,2,3]
[]
{"changed":"now","keep":{"x":1}}
```

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode/table.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): qd.encode for real tables + mixed lazy/materialized"
```

---

## Task 15: Wire qd.decode / encode / etc into top-level lua/quickdecode.lua

**Files:**
- Modify: `lua/quickdecode.lua`
- Modify: `tests/lua/lazy_table_spec.lua` (drop `require("quickdecode.table")` and use `qd` instead, since the user-facing API is at the top level)

- [ ] **Step 1: Update the public surface**

At the bottom of `lua/quickdecode.lua` (before `return _M`), add:

```lua
-- Lazy table API (cjson-shaped surface). See lua/quickdecode/table.lua.
local _lazy = require("quickdecode.table")
_M.decode         = _lazy.decode
_M.encode         = _lazy.encode
_M.materialize    = _lazy.materialize
_M.pairs          = _lazy.pairs
_M.ipairs         = _lazy.ipairs
_M.null           = _lazy.null
_M.empty_array_mt = _lazy.empty_array_mt
```

- [ ] **Step 2: Update the spec file's require**

At the top of `tests/lua/lazy_table_spec.lua`:

```lua
local qd = require("quickdecode")
local qt = qd                 -- keep tests reading naturally
```

(Tests that compare against `qt._LazyObject` still work because `_M._LazyObject` was set on `quickdecode.table`'s module table, not re-exported from top level. To make that comparison work post-rewire, also add to the bottom of `lua/quickdecode.lua`:

```lua
_M._LazyObject = _lazy._LazyObject
_M._LazyArray  = _lazy._LazyArray
```

These are intentionally `_`-prefixed — implementation detail, exposed for tests only.)

- [ ] **Step 3: Smoke-verify the merged surface works**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qd = require("quickdecode")
local t = qd.decode([[{"model":"x","messages":[{"role":"user"}]}]])
print(t.model, t.messages[1].role)
print(qd.encode(t))
'
```

Expected:
```
x       user
{"model":"x","messages":[{"role":"user"}]}
```

- [ ] **Step 4: Commit**

```bash
git add lua/quickdecode.lua tests/lua/lazy_table_spec.lua
git commit -m "feat(lua): re-export lazy table API from top-level quickdecode"
```

---

## Task 16: Round-trip and sentinel coverage in busted spec

**Files:**
- Modify: `tests/lua/lazy_table_spec.lua`

These tests gate the "cjson compat" promise from the spec — full round-trip equivalence with `cjson.decode + cjson.encode` over a representative fixture set.

- [ ] **Step 1: Add the round-trip tests**

Append to `tests/lua/lazy_table_spec.lua`:

```lua
local cjson = require("cjson")

-- Deep-equal aware of cjson.null and empty_array_mt (which qd aliases).
local function deep_equal(a, b)
    if a == b then return true end
    if type(a) ~= "table" or type(b) ~= "table" then return false end
    for k, v in pairs(a) do
        if not deep_equal(v, b[k]) then return false end
    end
    for k in pairs(b) do
        if a[k] == nil then return false end
    end
    return true
end

describe("cjson round-trip equivalence", function()
    local fixtures = {
        '{"a":1,"b":"x","c":null,"d":true,"e":false,"f":[1,2,3],"g":{"h":4.5}}',
        '[1,"x",true,null,{},[]]',
        '{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}',
        '{}',
        '[]',
        '{"escapes":"a\\nb\\tc\\\"d\\\\e"}',
    }
    for _, src in ipairs(fixtures) do
        it("materialize matches cjson.decode for: " .. src:sub(1, 40), function()
            local from_qd = qd.materialize(qd.decode(src))
            local from_cj = cjson.decode(src)
            assert.is_true(deep_equal(from_qd, from_cj))
        end)

        it("encode round-trips for: " .. src:sub(1, 40), function()
            local out = qd.encode(qd.decode(src))
            local back_qd = cjson.decode(out)
            local back_cj = cjson.decode(src)
            assert.is_true(deep_equal(back_qd, back_cj))
        end)
    end
end)

describe("sentinel handling", function()
    it("JSON null reads as qd.null and encodes back", function()
        local t = qd.decode('{"x":null}')
        assert.are.equal(qd.null, t.x)
        assert.are.equal('{"x":null}', qd.encode(t))
    end)

    it("empty array stays an array through materialize and encode", function()
        local t = qd.decode('{"xs":[]}')
        local m = qd.materialize(t)
        assert.are.equal(qd.empty_array_mt, getmetatable(m.xs))
        assert.are.equal('{"xs":[]}', qd.encode(t))
    end)
end)
```

- [ ] **Step 2: Smoke-verify (manual since busted is not on this machine)**

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit -e '
package.path = package.path .. ";./lua/?.lua;./lua/?/init.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"
local qd = require("quickdecode")
local cjson = require("cjson")
local src = [[{"a":1,"b":"x","c":null,"d":true,"e":false,"f":[1,2,3]}]]
print(qd.encode(qd.decode(src)) == src)            -- substring fast path
print(cjson.encode(cjson.decode(qd.encode(qd.decode(src)))) ~= "")
'
```

Expected: `true` then `true`.

- [ ] **Step 3: Commit**

```bash
git add tests/lua/lazy_table_spec.lua
git commit -m "test(lua): cjson round-trip equivalence + sentinel coverage"
```

---

## Task 17: Bench scenarios for the lazy API

**Files:**
- Modify: `benches/lua_bench.lua`

- [ ] **Step 1: Add the new bench rows**

In `benches/lua_bench.lua`, find the scenario loop body (the `for _, s in ipairs(scenarios) do ... end` block). After the existing `bench("quickdecode.parse + access 3 fields", ...)` call, add:

```lua
    bench("qd.decode + t.field x3", s.iters, function()
        local t = qd.decode(s.payload)
        local _ = t.model
        local _ = t.temperature
        local _ = t.messages and t.messages[1] and t.messages[1].role
    end)

    bench("qd.decode + qd.encode (unmodified)", s.iters, function()
        local t = qd.decode(s.payload)
        local _ = qd.encode(t)
    end)
```

Add the same pair to the `interleaved` block at the bottom (after the existing `quickdecode.parse + access 3 fields` row there).

- [ ] **Step 2: Run the bench and eyeball the targets**

Run:

```bash
LD_LIBRARY_PATH=$PWD/target/release /usr/local/openresty/luajit/bin/luajit benches/lua_bench.lua | head -40
```

Expected: `qd.decode + t.field x3` median is within ±10–20% of `quickdecode.parse + access 3 fields`. `qd.decode + qd.encode (unmodified)` is significantly faster than running `cjson.decode + cjson.encode` (the substring fast path is essentially a memcpy).

If `qd.decode + t.field x3` lands much slower (>30% behind `qd.parse`), pause and investigate — likely culprit is excess FFI boxing or a missing JIT-friendly path. Do not optimize speculatively without bench evidence.

- [ ] **Step 3: Commit**

```bash
git add benches/lua_bench.lua
git commit -m "bench: add qd.decode/qd.encode rows"
```

---

## Task 18: README usage section + deferred-items entry

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add a usage section for the lazy API**

In `README.md`, find the existing usage section (search for `qd.parse`). After the `qd.parse` example block, insert a new subsection:

```markdown
### Lazy table API (`qd.decode` / `qd.encode`)

For callers migrating from `cjson`, an alternative API returns a table-shaped
lazy view. Reads, iteration, and length all work like a `cjson.decode`'d
table; writes materialize the affected level into a plain Lua table.

```lua
local qd    = require("quickdecode")
local cjson = require("cjson")          -- optional; provides null / empty_array sentinels

local t = qd.decode(json_str)

print(t.model)
for _, m in qd.ipairs(t.messages) do
    print(m.role, m.content)
end

t.extra = "x"

local s = qd.encode(t)                  -- drop-in replacement for cjson.encode
```

`qd.encode` works on lazy proxies (re-emitting unmodified subtrees as the
original JSON bytes), real Lua tables (matching `cjson.encode` output), and
mixed trees. Callers cannot pass a lazy proxy directly to `cjson.encode`
(cjson bypasses metamethods in C); use `qd.encode` instead, or call
`qd.materialize(t)` to get a plain Lua table that any third-party encoder
can handle.
```

- [ ] **Step 2: Add a deferred item for the iteration cost**

In `README.md` under **Roadmap / Deferred**, add the following bullet (place it near the other perf-related entries):

```markdown
- **Stateful O(N) iterator FFI** — current `qd.pairs` and the `__newindex`
  materialization path walk the object cursor from the start on every
  step, giving O(N²) total cost for full enumeration. Acceptable for the
  "read a few keys" use case the library is optimized for; full-iteration
  workloads (e.g. encoding a deeply-keyed object that has been materialized)
  would benefit from a `qjd_iter_init` / `qjd_iter_next` pair that holds
  position state across calls.
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: add lazy table API usage + iteration cost roadmap item"
```

---

## Task 19: Lua busted suite runs from Makefile

**Files:**
- Verify: `Makefile` already invokes `busted` for `tests/lua/*_spec.lua` — no change expected.

- [ ] **Step 1: Confirm the new spec file is picked up by `make test`**

Run (on a machine that has busted installed; the dev machine here does not, so CI will validate):

```bash
make test
```

Expected: all existing Rust tests pass, and the new `tests/lua/lazy_table_spec.lua` is invoked and passes.

If on a machine without busted: run the smoke command from each prior task in sequence and verify the expected output. CI is the source of truth for the busted spec.

- [ ] **Step 2: Final commit gate**

Run the three Rust CI gates locally one more time:

```bash
cargo test --release
cargo test --release --no-default-features
cargo test --features test-panic --release
```

Expected: all PASS, no regressions.

- [ ] **Step 3: No code commit for this task** — if `make test` passes cleanly, the work is done. If a regression surfaces (e.g. a stale `ffi.cdef` collision because LuaJIT is re-cdef'ing `qjd_cursor_bytes`), fix it inline and commit with `fix: ...` as appropriate.

---

## Notes for the implementing engineer

- **busted not on this dev machine.** Each task lists a `luajit -e` smoke command. Run those locally; CI will run the full busted spec. Do not skip writing the busted spec — it is the regression net.
- **Reuse, don't refactor.** A few of the early tasks introduce code (`read_object_field`, `read_array_index`) that is then refactored when `decode_cursor` lands in Task 8. That sequence is intentional; do not pre-emptively factor out the helper, the test for it isn't in place yet.
- **`encode` referenced before its definition.** In Lua, forward references inside function bodies are resolved at call time, so the structure used in Task 14 (where `encode_array` and `encode_object` call `encode`) is fine even though `encode` is the local variable being defined just below them. Don't get clever and reorder; the assignment to `_M.encode = encode` at the end is what makes the recursion close cleanly.
- **Frequent commits.** Each task ends with a commit. Do not batch.
- **No `--no-verify` on commits.** Honor any pre-commit hooks the repo sets up.

## Self-review checklist (filled in)

| Spec section | Implemented by |
|---|---|
| `qd.decode` returns a Lua table with metatable | Task 4 (root + LazyObject) + Task 6 (LazyArray); wired at top level in Task 15 |
| `__index` for object/array | Tasks 4, 5, 6 |
| `__newindex` materializes shallow | Task 10 |
| `__len` | Task 7 |
| `__pairs` / `__ipairs` + `qd.pairs` / `qd.ipairs` | Tasks 8, 9 |
| `qd.encode` (proxy, scalars, real, mixed) | Tasks 12, 13, 14 |
| `qd.materialize` | Task 11 |
| Sentinel bridging (`qd.null`, `qd.empty_array_mt`) | Task 3, propagated by Tasks 6, 10, 11 |
| `qjd_cursor_bytes` FFI | Task 1 |
| Object-iteration FFI (`qjd_cursor_object_entry_at`) | Task 2 |
| C header updated | Tasks 1, 2 |
| Round-trip equivalence tests | Task 16 |
| Bench scenarios for `qd.decode` / `qd.encode` | Task 17 |
| README usage + deferred item | Task 18 |
