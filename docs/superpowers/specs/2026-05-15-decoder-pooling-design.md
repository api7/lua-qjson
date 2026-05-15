# Decoder/Document Instance Pooling — Design (v1)

**Date:** 2026-05-15
**Status:** Design approved, awaiting implementation plan
**Project:** `lua-quick-decode`
**Issue:** [#6](https://github.com/membphis/lua-quick-decode/issues/6)
**Related:** [2026-05-15-rust-quick-json-decode-design.md](./2026-05-15-rust-quick-json-decode-design.md)

---

## 1. Purpose & Non-Goals

### Purpose

Today every `qd.parse(payload)` constructs a fresh `Document` with a fresh `indices: Vec<u32>` (reserved at `buf.len() / 6`), plus a fresh `scratch` buffer and `SkipCache`. For a 10 MB payload the `indices` reservation alone is ~1.7 MB — large enough to take the `mmap` allocation path on glibc, costing roughly 10–50 µs per parse plus the symmetric dealloc on drop.

This design adds a reusable `Decoder` that owns those buffers across parses. A `decoder:parse(payload)` call truncates the buffers (preserving capacity) and re-fills them, eliminating per-parse allocation overhead in steady state. Expected wins by payload size:

| size | est. speedup |
|---|---:|
| small (2 KB) | ~5–10% |
| 100 KB – 1 MB | ~5–15% |
| 10 MB | ~1–3% (alloc is a small fraction of 2.9 ms) |

### Non-Goals

- **No change to validation semantics.** Phase 1 still performs the same shallow structural scan; Phase 2 still lazily decodes. The bytes returned by every accessor must be byte-identical to the existing `qd.parse()` path. This is enforced by a cross-equivalence test (§7).
- **No concurrent docs per decoder.** Only one live `Document` per `Decoder` at a time. Earlier docs become invalid as soon as `parse()` is called again (and are detected — see §4).
- **No thread safety.** A `Decoder` is single-threaded, same constraint as `qjd_doc` today.
- **No streaming.** Each `parse()` still requires a contiguous `&[u8]`.

---

## 2. Confirmed Decisions

| Aspect | Decision |
|---|---|
| API style | Parallel — `qd.new_decoder()` added; existing `qd.parse()` unchanged |
| Lua surface | Two distinct objects: `decoder` and `doc` |
| Liveness | One live `doc` per `decoder` at a time |
| Stale-doc safety | Generation counter; stale access returns `QJD_STALE_DOC` (Lua: `nil`) |
| Lifecycle methods | `decoder:reset()` + `decoder:destroy()` + ffi.gc fallback |
| Parse-error recovery | `parse()` auto-truncates on entry; partial state cannot leak |
| Rust architecture | `Document` renamed to `Decoder`, becomes stateful; `qjd_doc` becomes a thin `{decoder, gen}` handle |
| Backward compat | `qjd_parse()` + `qjd_free()` + all `qjd_get_*` / cursor APIs unchanged at the C ABI |

---

## 3. API Surface

### 3.1 Lua

```lua
local qd = require("quickdecode")

-- One-shot (unchanged, backward compatible)
local doc = qd.parse(payload)
doc:get_str("body.model")

-- Pooled (new)
local decoder = qd.new_decoder()
for _, payload in ipairs(payloads) do
  local doc = decoder:parse(payload)
  -- ...access doc / cursor...
end
decoder:reset()      -- optional: shrink buffers to zero capacity
decoder:destroy()    -- optional: early release; decoder is dead afterwards
```

The returned `doc` uses the same `Doc` metatable as today. Every existing accessor (`get_str`, `get_i64`, `get_f64`, `get_bool`, `is_null`, `typeof`, `len`, `open`) works without change. The only new failure mode surfaced through the existing accessors is `QJD_STALE_DOC`, which the wrapper translates to `nil` — the same convention as `QJD_NOT_FOUND`. Callers migrating from `qd.parse` need no `pcall` additions.

### 3.2 C ABI

New symbols added to `include/lua_quick_decode.h`:

```c
typedef struct qjd_decoder qjd_decoder;

qjd_decoder* qjd_decoder_new(void);
void         qjd_decoder_free(qjd_decoder*);
void         qjd_decoder_reset(qjd_decoder*);
void         qjd_decoder_destroy(qjd_decoder*);

qjd_doc* qjd_decoder_parse(qjd_decoder*, const uint8_t* buf, size_t len, int* err_out);
```

The returned `qjd_doc*` is the same opaque type as today. All existing `qjd_get_*`, `qjd_open`, and `qjd_cursor_*` functions accept it. The cursor struct is unchanged; its freshness check is derived through its `doc` pointer — see §4.4.

A new error code is added to `src/error.rs` and the header:

```c
QJD_STALE_DOC = 9   // doc/cursor's generation no longer matches its decoder
```

`qjd_strerror(9)` returns `"stale document or cursor"`.

### 3.3 Lua wrapper

```lua
-- In lua/quickdecode.lua
local NOT_FOUND  = 2
local STALE_DOC  = 9

local function check_err(rc)
    if rc == 0 then return true end
    if rc == NOT_FOUND or rc == STALE_DOC then return false end  -- nil-return path
    error("quickdecode: " .. ffi.string(C.qjd_strerror(rc)))
end

local Decoder = {}; Decoder.__index = Decoder

function _M.new_decoder()
    local ptr = C.qjd_decoder_new()
    if ptr == nil then error("quickdecode: decoder alloc failed") end
    return setmetatable({
        _ptr = ffi.gc(ptr, C.qjd_decoder_free),
    }, Decoder)
end

function Decoder:parse(payload)
    self._payload = payload                       -- pin against Lua GC
    local doc_ptr = C.qjd_decoder_parse(self._ptr, payload, #payload, err_box)
    if doc_ptr == nil then
        error("quickdecode: " .. ffi.string(C.qjd_strerror(err_box[0])))
    end
    return setmetatable({
        _ptr      = ffi.gc(doc_ptr, C.qjd_free),
        _decoder  = self,                          -- transitive payload pin
    }, Doc)
end

function Decoder:reset()   C.qjd_decoder_reset(self._ptr) end
function Decoder:destroy() C.qjd_decoder_destroy(self._ptr) end
```

The reference chain `doc → decoder → payload` ensures: while any doc is reachable, its decoder stays alive; while the decoder is alive, the *current* payload stays alive. The previous parse's payload is dropped from `_payload` on the next `parse()` and becomes GC-eligible — old docs that referenced it are already stale (see §4) and cannot dereference it.

---

## 4. Rust Architecture

### 4.1 `Decoder` struct

Replaces today's `Document`:

```rust
pub struct Decoder {
    indices:     Vec<u32>,
    scratch:     RefCell<Vec<u8>>,
    skip:        RefCell<SkipCache>,
    current_buf: Option<&'static [u8]>,
    gen:         u32,
    state:       DecoderState,
}

enum DecoderState {
    Ready,
    Parsed,
    Destroyed,
}
```

No `Errored` state. A failed `parse()` returns the decoder to `Ready` — the next `parse()` truncates before scanning, so partial indices/scratch can never be observed (and the gen has already bumped, so any leftover doc is stale).

### 4.2 `parse()` flow

```rust
impl Decoder {
    pub fn parse(&mut self, input: &[u8]) -> Result<DocHandle, qjd_err> {
        if matches!(self.state, DecoderState::Destroyed) {
            return Err(qjd_err::QJD_INVALID_ARG);
        }
        self.gen = self.gen.wrapping_add(1);          // invalidate all prior docs/cursors
        self.indices.truncate(0);
        self.scratch.borrow_mut().truncate(0);
        self.skip.borrow_mut().clear();

        match crate::scan::scan(input, &mut self.indices) {
            Ok(_) => {}
            Err(_) => {
                self.state = DecoderState::Ready;
                self.current_buf = None;
                return Err(qjd_err::QJD_PARSE_ERROR);
            }
        }
        self.indices.push(u32::MAX);                  // sentinel
        self.current_buf = Some(unsafe { std::mem::transmute(input) });
        self.state = DecoderState::Parsed;
        Ok(DocHandle { gen: self.gen })
    }
}
```

### 4.3 FFI doc handle

```rust
pub struct qjd_doc {
    decoder:      NonNull<Decoder>,
    gen:          u32,
    owns_decoder: bool,
}
```

- `qjd_decoder_parse()` returns a doc with `owns_decoder = false`. `qjd_free` drops only the doc box.
- `qjd_parse()` (legacy path) internally `Box::new(Decoder::new())`, parses into it, wraps in a doc with `owns_decoder = true`. `qjd_free` additionally `Box::from_raw`s the private decoder.
- All `qjd_get_*` / `qjd_open` / cursor functions are oblivious to `owns_decoder`.

### 4.4 Cursor

The `qjd_cursor` C struct is unchanged — both `_reserved0` and `_reserved1` stay reserved. A cursor's freshness is derived through its `doc` pointer:

```rust
// In cursor_to_internal:
let doc: &qjd_doc = &*(c.doc as *mut qjd_doc);   // already pinned by Lua wrapper's _doc ref
check_doc_alive(c.doc as *mut qjd_doc)?;          // doc.gen vs decoder.gen + state check
```

Since the Lua wrapper's `Cursor` table keeps a strong `_doc = self._doc` reference (preserving today's pattern), `cursor.doc` is always a valid pointer while the cursor is reachable. The gen check on the doc handles staleness for both the doc itself and any cursor opened from it: once the decoder reparses, both the doc and all its cursors fail the gen check.

No ABI change to `qjd_cursor`.

### 4.5 Refactor mechanics

- `git mv src/doc.rs src/decoder.rs` — preserves blame.
- Inside the renamed file: `Document` → `Decoder`, add the new fields, replace `Document::parse(buf)` with `Decoder::new()` + `Decoder::parse(&mut self, buf)`.
- `src/cursor.rs`, `src/decode/*`, `src/scan/*` callers: function signatures change `&Document` → `&Decoder`. Logic untouched.
- `src/ffi.rs`: extract a small `check_doc_alive` helper used by every public entry point. The helper checks **state first** (`Destroyed` → `QJD_INVALID_ARG`) then **gen** (mismatch → `QJD_STALE_DOC`); this ordering matches §5.4. Add the four new `qjd_decoder_*` exports, each wrapped in `ffi_catch!` per the existing convention.
- `src/skip_cache.rs`: add `pub(crate) fn clear(&mut self)` (drops all entries but keeps slot 0) and `pub(crate) fn clear_and_shrink(&mut self)` (also calls `shrink_to_fit` on the inner `Vec` and `FxHashMap`). Both are needed by §5.

---

## 5. Lifecycle Semantics

### 5.1 `parse()` after `parse()`

Generation bumps first thing in the new `parse()`. All prior docs and cursors become stale. Buffers truncate but keep capacity. After a successful parse, the new doc holds the new gen.

### 5.2 `parse()` after parse error

State is `Ready`, gen has already advanced, buffers are partially written but unreadable (all prior docs are stale, no doc was returned for the failed parse). The next `parse()` truncates again and proceeds.

### 5.3 `reset()`

```rust
pub fn reset(&mut self) {
    self.gen = self.gen.wrapping_add(1);
    self.indices = Vec::new();                          // returns memory to allocator
    self.scratch.borrow_mut().shrink_to(0);
    self.skip.borrow_mut().clear_and_shrink();
    self.current_buf = None;
    self.state = DecoderState::Ready;
}
```

Use case: just processed a one-off huge payload, don't want the decoder to keep that capacity around for the worker's lifetime.

### 5.4 `destroy()`

```rust
pub fn destroy(&mut self) {
    self.gen = self.gen.wrapping_add(1);
    let _ = std::mem::take(&mut self.indices);
    let _ = std::mem::take(&mut *self.scratch.borrow_mut());
    self.skip.borrow_mut().clear_and_shrink();
    self.current_buf = None;
    self.state = DecoderState::Destroyed;
}
```

After `destroy()`, every subsequent FFI entry returns `QJD_INVALID_ARG`. The decoder's own memory is reclaimed only when ffi.gc fires on `qjd_decoder_free` — `destroy()` just shaves off the bulk allocations early.

### 5.5 Gen overflow

`wrapping_add(1)`. At 1 ms/parse the counter wraps after ~50 days of *continuous* `parse()` calls on the same decoder. By that point any old doc reference is long collected by Lua GC. The theoretical risk is documented but not engineered against in v1 — listed in `README.md` under _Roadmap / Deferred_.

---

## 6. Error Handling

| Code | Name | When | Lua wrapper |
|------|------|------|-------------|
| 0 | OK | success | true |
| 1 | PARSE_ERROR | scan failed | raises |
| 2 | NOT_FOUND | path missing | nil |
| 3 | TYPE_MISMATCH | wrong type at path | raises |
| 4 | OUT_OF_RANGE | numeric overflow | raises |
| 5 | DECODE_FAILED | lazy decode failed | raises |
| 6 | INVALID_PATH | path syntax | raises |
| 7 | INVALID_ARG | null arg / destroyed decoder | raises |
| 8 | OOM | panic caught by `ffi_catch!` | raises |
| **9** | **STALE_DOC** | **gen mismatch** | **nil** |

The `QJD_STALE_DOC` code value, `qjd_strerror` entry, and `lua/quickdecode.lua` mirror must all be kept in sync (per the existing convention noted in CLAUDE.md).

---

## 7. Testing & Validation

### 7.1 Rust unit tests (`src/decoder.rs::tests`)

- `parse_then_parse_bumps_gen` — two successive parses, second doc's gen ≠ first's
- `parse_error_returns_to_ready` — malformed input leaves state at `Ready` and gen bumped
- `reset_shrinks_capacity` — large parse + reset → `indices.capacity() == 0`
- `destroy_sets_terminal_state` — post-destroy `parse()` / `reset()` return `QJD_INVALID_ARG`
- `gen_wraps_at_u32_max` — set gen near `u32::MAX`, confirm wrap behavior

### 7.2 Rust integration tests

New file `tests/decoder_ffi.rs`:

- `decoder_doc_equivalence` — for every fixture under `benches/fixtures/`, parse via both `qjd_parse` and `qjd_decoder_parse`, run the same battery of accessors, assert byte-identical results. This is the load-bearing guarantee that validation semantics are unchanged.
- `stale_doc_returns_error` — parse → hold doc → parse again → call `qjd_get_str` on the old doc, expect `QJD_STALE_DOC`
- `stale_cursor_returns_error` — same but the stale entity is a cursor opened from the first doc
- `reset_invalidates_cursors` — parse → open cursor → `reset()` → cursor access returns `QJD_STALE_DOC`
- `destroyed_decoder_rejects_all_ops` — destroy → parse/reset/get_str all return `QJD_INVALID_ARG`

### 7.3 Lua busted tests (`tests/lua/spec/decoder_spec.lua`)

- `new_decoder` returns a usable object
- `parse` returns a `Doc` with all existing methods
- multiple successive parses on the same decoder return correct results
- stale doc access returns `nil`, not an error
- `reset` and `destroy` work; post-destroy ops raise (because the FFI returns `QJD_INVALID_ARG`, not the nil-coded `QJD_STALE_DOC`)

### 7.4 Allocation counting

A new test-only Cargo feature `count-allocs` installs a counting `GlobalAlloc` in `tests/alloc_count.rs`:

```rust
#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

#[test]
fn pooled_path_amortizes_allocations() {
    let mut decoder = Decoder::new();
    for _ in 0..3 { decoder.parse(PAYLOAD).unwrap(); }   // warmup
    let baseline = ALLOC.count();
    for _ in 0..1_000 { decoder.parse(PAYLOAD).unwrap(); }
    let delta = ALLOC.count() - baseline;
    assert!(delta < 50, "expected ≈0 allocs, got {}", delta);
}

#[test]
fn fresh_decoder_per_parse_allocates() {
    let baseline = ALLOC.count();
    for _ in 0..1_000 {
        let mut d = Decoder::new();      // mimics the cost of the legacy qjd_parse path
        d.parse(PAYLOAD).unwrap();
    }
    assert!(ALLOC.count() - baseline > 1_000);
}
```

The feature gates the global allocator swap so it doesn't interfere with other tests. Run target:

```sh
cargo test --release --features count-allocs --test alloc_count
```

### 7.5 Bench harness (`benches/lua_bench.lua`)

Adds two cases:

- `decoder:parse` reuse loop (new)
- `lua-cjson` per-iter (existing baseline, kept)

Output is `wall_ms` (3-run median) per fixture per case. Existing `qd.parse` case stays as the baseline to measure improvement against.

### 7.6 CI

`.github/workflows/ci.yml` gains a fourth Rust matrix point:

4. `cargo test --release --features count-allocs --test alloc_count`

The three existing gates (default features, `--no-default-features`, `--features test-panic`) and the Lua busted job continue to run unchanged. The cross-equivalence test (§7.2) runs under gate 1 and gate 2 — catches any scanner-vs-decoder divergence as a side effect.

### 7.7 Lint baseline

`make lint`'s current 22 `missing_safety_doc` warnings on `qjd_*` exports will grow by ~5 (one per new `qjd_decoder_*` symbol). This is consistent with the existing README _Roadmap / Deferred_ entry; the bump will be noted there rather than treated as a regression.

---

## 8. Deferred

- Per-symbol safety docs on FFI exports (existing deferred item; grows by ~5 entries)
- Gen counter wrap protection (50-day continuous-use horizon; not engineered against in v1)
- Implicit module-level shared decoder (`qd.parse` keeping pooled buffers transparently) — possible future optimization once the explicit decoder API stabilizes
