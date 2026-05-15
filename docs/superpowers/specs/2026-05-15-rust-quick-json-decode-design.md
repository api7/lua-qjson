# Rust Quick JSON Decode — Design (v1)

**Date:** 2026-05-15
**Status:** Design approved, awaiting implementation plan
**Project:** `lua-quick-decode`

---

## 1. Purpose & Non-Goals

### Purpose

A Rust-implemented JSON decoder exposed to LuaJIT via FFI, optimized for the case where:

- A large-ish JSON (1 KB – 64 MB) is parsed **once**.
- The caller extracts a **small number of fields** (typically 5–20) via dynamic paths.
- The document is then **discarded**.

The library's competitive advantage over `lua-cjson` is that it skips:

- Constructing a full Lua `table` for the parsed document.
- Copying / interning every string value.
- Allocating GC objects for every nested object/array.

It does so by performing a **single fast SIMD structural scan** in Phase 1 (only recording byte offsets of structural characters) and then **lazily decoding** only the fields the caller actually requests in Phase 2.

### Non-Goals

- Full JSON RFC 8259 validation. We perform shallow structural validation only; value-level errors (invalid escapes, malformed numbers, invalid UTF-8 in `\u` sequences) are deferred to lazy decode and surfaced only if the offending field is accessed.
- Building a Lua table representation. The library never produces a Lua table from JSON; callers must request fields explicitly.
- Streaming / incremental parse. The whole input must be available as a contiguous `&[u8]` before parsing begins.
- Thread safety. A `qjd_doc` is single-threaded. Document objects must not be shared across threads.
- JSON encoding / serialization. Decode only.

---

## 2. Confirmed Decisions

| Aspect | Decision |
|---|---|
| Output artifact | Rust `cdylib` → `.so`, plus `rlib` for Rust-side tests/integration |
| Caller binding | LuaJIT via `ffi.cdef` + `ffi.load`; **no** dependency on `lua.h` |
| Access pattern | Fully dynamic, runtime path resolution |
| Access frequency | One parse, few accesses, then discard |
| Input size | 1 KB – 64 MB (32-bit offsets sufficient) |
| Portability | Scalar fallback required; SIMD as runtime-detected acceleration |
| Validation level | Shallow (brace/quote/structure only); value validity deferred to lazy decode |
| Buffer ownership | Borrow `&[u8]`; `Document` holds a reference for its lifetime |
| Field API | Both root-path (`get_str(doc, "body.model")`) and cursor (`open(doc, "body")` → sub-cursor) |
| Error model | `errcode` return + output pointer; static `qjd_strerror(code)` for descriptions |
| Lua wrapper | Full `quickdecode.lua` shipped as deliverable |
| Benchmark targets | 100 KB – 1 MB / 5-20 fields; 10 MB – 64 MB / few fields |
| Backends in v1 | `ScalarScanner` (fallback) + `Avx2Scanner` (x86_64); NEON deferred |

---

## 3. Architecture

### 3.1 Module Layout

```
src/
├── lib.rs               — crate root, re-exports
├── ffi.rs               — pub extern "C" symbols (C ABI layer)
├── doc.rs               — Document type (Phase 1 + container helpers)
├── cursor.rs            — Cursor, path resolution, skip-cache walk
├── path.rs              — path string parse (zero-alloc iterator)
├── error.rs             — error / type enums
├── scan/
│   ├── mod.rs           — Scanner trait + runtime dispatch (OnceCell-cached)
│   ├── scalar.rs        — scalar fallback
│   └── avx2.rs          — x86_64 AVX2 + PCLMUL (gated by `avx2` feature)
├── decode/
│   ├── mod.rs
│   ├── number.rs        — lazy i64/f64 parse
│   └── string.rs        — lazy escape decode + UTF-8 check on \u
└── skip_cache.rs        — Phase 2 sibling-skip cache

lua/
└── quickdecode.lua      — LuaJIT wrapper module

tests/
├── integration.rs       — Rust-side C ABI tests
└── lua/                 — busted Lua tests

benches/
├── rust_bench.rs        — Rust criterion benches
├── lua_bench.lua        — Lua bench vs lua-cjson
└── fixtures/            — JSON fixtures

include/
└── lua_quick_decode.h   — public C header

docs/
└── superpowers/specs/
    └── 2026-05-15-rust-quick-json-decode-design.md   (this file)
```

### 3.2 Layered Data Flow

```
caller buf:&[u8]
        │
        ▼ ffi::qjd_parse
   Document::parse
        │
        ▼ scan::dispatch (cached function pointer)
   { ScalarScanner | Avx2Scanner }
        │
        ▼
   indices: Vec<u32>  (Phase 1 complete)
        │
caller: doc:get_str("body.model")
        │
        ▼ path::parse  (zero-alloc iterator)
   Cursor::resolve
        │      │
        │      └─► skip_cache (lazy fill)
        ▼
   decode::string
        │
        ▼
   (ptr, len) → LuaJIT side ffi.string()
```

### 3.3 Key Invariants

- `Document<'a>` borrows `'a` from the caller's input buffer; the FFI layer erases `'a` to `'static`, and the LuaJIT wrapper enforces lifetime via Lua-side strong references.
- `indices: Vec<u32>` is write-once: filled during Phase 1, read-only thereafter.
- `skip_cache` lives on `Document`; populated lazily during Phase 2 access.
- `scratch: Vec<u8>` (for escape decode) lives on `Document`. **Invariant: only the most recent `get_str` result's pointer is valid.** The LuaJIT wrapper calls `ffi.string(ptr, len)` immediately to copy into a Lua string.
- `indices` records only byte offsets, **not** token types. Type is recovered from `buf[indices[i]]`. This saves 25% memory vs storing a type tag.

---

## 4. C ABI

Public header: `include/lua_quick_decode.h`. Symbols all `extern "C"`, `#[no_mangle]`.

### 4.1 Types

```c
typedef struct qjd_doc qjd_doc;   /* opaque */

typedef struct {
    const qjd_doc* doc;
    uint32_t       idx_start;     /* opener position in doc.indices */
    uint32_t       idx_end;       /* one past closer */
    uint32_t       _reserved0;    /* reserved for future fast-path */
    uint32_t       _reserved1;    /* reserved / padding */
} qjd_cursor;   /* 24 bytes, by-value, no allocation */
```

### 4.2 Error Codes

```c
typedef enum {
    QJD_OK              = 0,
    QJD_PARSE_ERROR     = 1,   /* Phase 1 structural failure */
    QJD_NOT_FOUND       = 2,   /* path does not exist */
    QJD_TYPE_MISMATCH   = 3,   /* path target is wrong JSON type for getter */
    QJD_OUT_OF_RANGE    = 4,   /* numeric overflow for requested integer type */
    QJD_DECODE_FAILED   = 5,   /* malformed escape / UTF-8 / number */
    QJD_INVALID_PATH    = 6,   /* path string syntax error */
    QJD_INVALID_ARG     = 7,   /* NULL pointer etc. */
    QJD_OOM             = 8,
} qjd_err;

const char* qjd_strerror(int code);   /* static; caller must not free */
```

### 4.3 Phase 1

```c
qjd_doc* qjd_parse(const uint8_t* buf, size_t len, int* err_out);
void     qjd_free (qjd_doc* doc);
```

Returns NULL on failure with `*err_out` set. `qjd_free(NULL)` is a no-op. The caller must keep `buf` valid for the lifetime of the returned `qjd_doc`.

### 4.4 Phase 2 — Root-Path API

```c
int qjd_get_str  (qjd_doc*, const char* path, size_t path_len,
                  const uint8_t** out_ptr, size_t* out_len);
int qjd_get_i64  (qjd_doc*, const char* path, size_t path_len, int64_t* out);
int qjd_get_f64  (qjd_doc*, const char* path, size_t path_len, double*  out);
int qjd_get_bool (qjd_doc*, const char* path, size_t path_len, int*     out);
int qjd_is_null  (qjd_doc*, const char* path, size_t path_len, int*     out);

typedef enum {
    QJD_T_NULL = 0, QJD_T_BOOL = 1, QJD_T_NUM = 2,
    QJD_T_STR  = 3, QJD_T_ARR  = 4, QJD_T_OBJ = 5,
} qjd_type;
int qjd_typeof   (qjd_doc*, const char* path, size_t path_len, int* type_out);
int qjd_len      (qjd_doc*, const char* path, size_t path_len, size_t* out);
```

### 4.5 Phase 2 — Cursor API

```c
int qjd_open            (qjd_doc*, const char* path, size_t path_len, qjd_cursor* out);

int qjd_cursor_get_str  (qjd_cursor*, const char* path, size_t path_len,
                         const uint8_t** out_ptr, size_t* out_len);
int qjd_cursor_get_i64  (qjd_cursor*, const char* path, size_t path_len, int64_t* out);
int qjd_cursor_get_f64  (qjd_cursor*, const char* path, size_t path_len, double*  out);
int qjd_cursor_get_bool (qjd_cursor*, const char* path, size_t path_len, int*     out);
int qjd_cursor_typeof   (qjd_cursor*, const char* path, size_t path_len, int* out);
int qjd_cursor_len      (qjd_cursor*, const char* path, size_t path_len, size_t* out);

/* sub-cursor; key/index avoids path-string composition */
int qjd_cursor_open     (qjd_cursor*, const char* path, size_t path_len, qjd_cursor* out);
int qjd_cursor_field    (qjd_cursor*, const char* key,  size_t key_len, qjd_cursor* out);
int qjd_cursor_index    (qjd_cursor*, size_t i, qjd_cursor* out);
```

### 4.6 Path Syntax

```
path     := segment ( '.' segment | '[' digit+ ']' )*
segment  := key | '[' digit+ ']'
key      := characters not containing '.' or '['
```

Empty path / NULL path = root.

Keys containing `.` or `[` are **not supported** via path strings — use `qjd_cursor_field()` instead. Attempting to parse such a path returns `QJD_INVALID_PATH`.

### 4.7 String Output Pointer Lifetime

The `out_ptr` returned by `qjd_get_str` / `qjd_cursor_get_str` points to either:

1. The original input buffer (when the string contains no escape sequences); or
2. A document-internal scratch buffer (when escape decode was required).

**The caller must consume the result before the next call to any `*_get_str` function on the same document.** Any subsequent `get_str` may invalidate prior pointers. The LuaJIT wrapper handles this by calling `ffi.string(ptr, len)` immediately, copying into a Lua string.

---

## 5. Phase 1 — Structural Scan

### 5.1 Goal

Given `buf: &[u8]`, produce `indices: Vec<u32>` listing the byte offset of every structural character (`{`, `}`, `[`, `]`, `:`, `,`, `"`) that is **not inside a string literal**.

### 5.2 Quote Handling

The hard part is correctly identifying which `"` characters open/close strings versus being escaped. We use the classical SIMD algorithm (simdjson):

For each 64-byte chunk:

1. Build `quote_mask` (bit per byte = `"`)
2. Build `backslash_mask` (bit per byte = `\`)
3. Build `structural_mask` (bit per byte = one of `{}[]:,`)
4. Compute `escaped_quote_mask` from `backslash_mask` using bit arithmetic that accounts for consecutive backslash runs (odd-length run = next char escaped; even-length = next char literal).
5. `real_quote_mask = quote_mask & ~escaped_quote_mask`
6. Use PCLMUL (or scalar prefix XOR on fallback) to turn `real_quote_mask` into `inside_string_mask` (1 between consecutive quote pairs).
7. `output_mask = structural_mask & ~inside_string_mask`, plus `real_quote_mask` itself (strings' boundaries are also structural).
8. Iterate set bits in `output_mask` and append byte offsets to `indices`.

The "carry-over" state across chunks: whether the chunk begins inside a string, and the trailing backslash count of the previous chunk.

### 5.3 Backend Trait

```rust
pub(crate) trait StructScanner {
    /// Scan `buf`, appending offsets to `out`.
    /// On shallow validation failure (unclosed string, unmatched bracket),
    /// returns `Err(byte_offset)` (offset not exposed in v1 errors).
    fn scan(buf: &[u8], out: &mut Vec<u32>) -> Result<(), usize>;
}

pub(crate) struct ScalarScanner;
#[cfg(target_arch = "x86_64")] pub(crate) struct Avx2Scanner;
```

### 5.4 Runtime Dispatch

```rust
static SCAN_FN: OnceCell<fn(&[u8], &mut Vec<u32>) -> Result<(), usize>>
    = OnceCell::new();

fn dispatch() -> fn(&[u8], &mut Vec<u32>) -> Result<(), usize> {
    *SCAN_FN.get_or_init(|| {
        #[cfg(target_arch = "x86_64")]
        if is_x86_feature_detected!("avx2")
            && is_x86_feature_detected!("pclmulqdq")
        {
            return Avx2Scanner::scan;
        }
        ScalarScanner::scan
    })
}
```

First call detects CPU features; subsequent calls use a cached function pointer (no `cpuid` overhead).

### 5.5 Indices Capacity

Initial capacity = `buf.len() / 6` (≈17 % of input bytes). Empirically structural characters make up 5–25 % of a typical JSON. Under-allocation triggers `Vec` doubling, costing one realloc; over-allocation wastes ≤17 % of input size.

For very small documents (< 4 KB), the wasted bytes are negligible. A stack-allocated SmallVec fast path is **deferred to Roadmap**.

### 5.6 Shallow Validation Coverage

Phase 1 detects and rejects:

- Unclosed string at end of buffer
- Mismatched bracket types (`{` paired with `]` etc.)
- Unbalanced closers (more `}` than `{` etc.)

Phase 1 does **not** check:

- Semantic position of `:` `,` (extraneous commas, missing colons)
- Escape sequence validity inside strings
- UTF-8 validity (multi-byte UTF-8 cannot be confused with ASCII structural chars)
- Number format validity
- Duplicate keys

### 5.7 Expected Throughput

| Backend | Target |
|---|---|
| Scalar | 500 MB/s – 1 GB/s |
| AVX2 (+ PCLMUL) | 3 – 6 GB/s |

---

## 6. Phase 2 — Path Resolution & Cursor

### 6.1 Cursor Internal Representation

```rust
#[derive(Copy, Clone)]
pub(crate) struct Cursor<'d> {
    doc:        &'d Document<'d>,
    /// Slice of doc.indices covered by this cursor.
    /// idx_start points at '{' or '['; idx_end points one past matching '}' / ']'.
    idx_start:  u32,
    idx_end:    u32,
}
```

The published `qjd_cursor` carries two `_reservedN` slots beyond `idx_start`/`idx_end`; they are unused in v1 but reserved so a future per-cursor skip-cache fast-path can be added without breaking the ABI.

`Cursor` is `Copy` and never allocates. `open()`, `field()`, `index()` return new cursors by value.

### 6.2 Resolution Algorithm

```text
for seg in path:
    Confirm cursor points at correct container type:
        seg=Key  → require '{' at cursor opener; else TYPE_MISMATCH
        seg=Idx  → require '[' at cursor opener; else TYPE_MISMATCH

    Walk children of the container:
        - If cache_slot is populated: directly read child_starts[i] /
          probe child_starts for matching key.
        - Otherwise: brace-counting scan from opener+1 to find each child,
          populating cache_slot as we go (incremental fill).

    On match: advance cursor to child's [idx_start, idx_end).
    On exhaustion: NOT_FOUND.
```

### 6.3 Sibling-Skip Cache

```rust
pub(crate) struct SkipCache {
    slots:     Vec<SkipSlot>,                  // slot 0 reserved
    by_opener: rustc_hash::FxHashMap<u32, u32>,// opener idx → slot number
}

pub(crate) struct SkipSlot {
    /// child_starts[i] = position in doc.indices where i-th child begins
    /// (for object: pointing at the key's opening '"';
    ///  for array: pointing at the value's first token).
    child_starts: Vec<u32>,
    /// child_ends[i] = idx_end for a Cursor pointing at the i-th child's value.
    /// Storing this lets cache-hit lookups skip the brace-counting walk.
    child_ends:   Vec<u32>,
}
```

**Build-on-first-access:** when a container is entered for the first time, its `SkipSlot` is built incrementally as the resolver walks its children. The walk uses brace-counting (the cheap operation on the `indices` array, not on the original buffer). Subsequent accesses to the same container are O(N_keys) field comparisons with no brace counting.

**Memory cost analysis:** worst case is when the caller enters every child of a large array (e.g. iterates 100 `messages[i]` and descends into each). Each entered container costs roughly `8 * num_children` bytes. For a 1 MB / 100-message JSON this stays below 5 MB total — acceptable. No LRU eviction in v1.

### 6.4 Field-Type Dispatch

Typed getters (`get_str`, `get_i64`, ...) inspect `buf[doc.indices[cursor.idx_start]]` after path resolution:

| First byte | Inferred type | Behavior |
|---|---|---|
| `"` | string | `get_str` → decode; `get_i64`/`get_f64`/`get_bool` → TYPE_MISMATCH |
| `0`-`9`, `-` | number | `get_i64`/`get_f64` → parse; others → TYPE_MISMATCH |
| `t`, `f` | bool | `get_bool` → parse; others → TYPE_MISMATCH |
| `n` | null | `is_null` → true; others → TYPE_MISMATCH |
| `{` | object | `typeof` → OBJ; getters → TYPE_MISMATCH |
| `[` | array | `typeof` → ARR; getters → TYPE_MISMATCH |

`qjd_typeof` only inspects the first byte; no value decoding.

`qjd_typeof` on a non-existent path returns `QJD_NOT_FOUND`, **not** `QJD_T_NULL`. The two are distinct.

### 6.5 String Escape Decode

```rust
fn decode_string(
    buf: &[u8], start: usize, end: usize,
    scratch: &mut Vec<u8>,
) -> Result<(*const u8, usize), qjd_err> {
    // Fast path: no backslash in range → return original slice.
    if memchr::memchr(b'\\', &buf[start..end]).is_none() {
        return Ok((buf.as_ptr().wrapping_add(start), end - start));
    }
    // Slow path: decode into scratch.
    scratch.clear();
    // Handle: \" \\ \/ \b \f \n \r \t \u XXXX with surrogate pair join
    // ...
    Ok((scratch.as_ptr(), scratch.len()))
}
```

UTF-8 validity of `\u XXXX` sequences (correct surrogate pairing) is checked here and surfaced as `QJD_DECODE_FAILED`. Other bytes are passed through without UTF-8 validation, consistent with our shallow-validation policy.

A SIMD-accelerated backslash search in the fast path is **deferred to Roadmap**.

### 6.6 Number Decode

- `get_i64`: hand-written fast parse, accepts JSON-number integer form (`-?[0-9]+`), rejects `.`, `e`, `E`. Overflow → `QJD_OUT_OF_RANGE`.
- `get_f64`: `core::str::FromStr` on a verified-ASCII slice. If first benchmark shows this dominating, switch to `lexical` — **deferred to Roadmap**.
- Integers > 2⁵³ requested via `get_f64` will return with precision loss per IEEE 754 (no error). Integers > i64 range via `get_i64` return `QJD_OUT_OF_RANGE`.

A "lossless integer" mode returning `int64_t` as cdata (preserving full precision on the Lua side) is **deferred to Roadmap**.

---

## 7. Memory Management & Safety

### 7.1 Document Layout

```rust
pub struct Document<'a> {
    buf:     &'a [u8],
    indices: Vec<u32>,      // appended sentinel u32::MAX at end
    scratch: Vec<u8>,       // lazy; populated on first escape-decode
    skip:    SkipCache,     // lazy; populated on first Phase 2 access
}
```

### 7.2 Allocation Budget

| Phase | Item | Count |
|---|---|---|
| Phase 1 | `Box<Document>` | 1 |
| Phase 1 | `indices` initial reserve | 1 |
| Phase 1 | `indices` doubling (worst case) | 0–2 |
| Phase 2 | `scratch` first escape | 0 or 1 |
| Phase 2 | `skip.slots[i].child_starts` per first-entered container | 1 each |
| Phase 2 | path parse / cursor ops | 0 |

### 7.3 FFI Safety

All FFI entry points:

- Reject NULL pointers with `QJD_INVALID_ARG` (no panic, no UB).
- Trust `len` (cannot validate at runtime).
- Wrap their body in `std::panic::catch_unwind` to prevent unwinding across the C boundary. Internal panics convert to `QJD_OOM`.
- Use `unsafe extern "C"`.

Rust internal code is panic-free in steady state: no `.unwrap()`, no `.expect()`, no array indexing where bounds aren't pre-validated. Errors propagate via `Result<_, qjd_err>` to the FFI layer.

### 7.4 Lifetime Erasure

The FFI layer materializes a `Document<'static>` from a `&'static [u8]` made via `slice::from_raw_parts`. The actual lifetime equals the caller's input buffer, which Rust cannot enforce. The LuaJIT wrapper (§8) enforces it by holding a strong reference to the original Lua string.

### 7.5 Threading

Single-threaded per `qjd_doc`. No internal locking. Documented in the public header.

---

## 8. LuaJIT Wrapper (`lua/quickdecode.lua`)

### 8.1 Responsibilities

1. Declare the C ABI via `ffi.cdef`.
2. Load the shared library via `ffi.load("quickdecode")`.
3. Wrap raw C calls into OO-style methods on `Doc` and `Cursor`.
4. **Strong-hold the original JSON string** to prevent GC while the document is alive.
5. Register `qjd_free` via `ffi.gc` for automatic cleanup.
6. Translate `QJD_NOT_FOUND` to Lua `nil`; other errors to `error(qjd_strerror(code))`.
7. Call `ffi.string(ptr, len)` immediately on string results, eliminating the scratch-invalidate hazard.

### 8.2 API Surface

`Doc` methods: `get_str`, `get_i64`, `get_f64`, `get_bool`, `is_null`, `typeof`, `len`, `open(path)`.

`Cursor` methods: same set + `open(path)`, `field(key)`, `index(i)`.

`#cursor` via `__len` is **not** implemented (Lua 5.1 / LuaJIT compatibility). Use `cursor:len("")`.

### 8.3 Output-Box Reuse

Module-level pre-allocated `ffi.new` buffers (`err_box`, `i64_box`, `strp_box`, `cur_box`, ...) are reused across all calls. New cdata allocation in the hot path would abort LuaJIT traces.

### 8.4 Lifetime Holding

```lua
function _M.parse(json_str)
    local err = err_box
    local ptr = C.qjd_parse(json_str, #json_str, err)
    if ptr == nil then error(...) end
    return setmetatable({
        _ptr  = ffi.gc(ptr, C.qjd_free),
        _hold = json_str,            -- strong ref keeps buffer alive
    }, Doc)
end
```

Cursors hold a back-reference to their `Doc` to prevent the `Doc` (and therefore the buffer) from being collected while cursors exist.

### 8.5 Integer Precision Caveat

`tonumber(int64_t)` truncates to double; values exceeding 2⁵³ lose precision silently. Documented in the wrapper. A lossless-integer mode returning cdata is on the Roadmap.

---

## 9. Testing & Benchmarking

### 9.1 Test Layers

| Layer | Framework | Approx Cases |
|---|---|---|
| Rust unit (`#[cfg(test)]`) | `cargo test` | ~100 |
| Rust integration (`tests/integration.rs`) | `cargo test` | ~30 |
| Property / fuzz | `proptest`, `cargo-fuzz` | ongoing |
| Lua integration (`tests/lua/`) | `busted` | ~50 |

### 9.2 Critical Test Matrix

**Phase 1 correctness:**
- ScalarScanner vs Avx2Scanner produce **bit-identical** `indices` on the same input. Enforced by proptest cross-check.
- Buffer length boundaries: `len % 64 ∈ {0, 1, 31, 32, 33, 63}`.
- Pure ASCII vs multi-byte UTF-8 content in strings.
- Adversarial escape patterns: `\\\"`, `\\\\\"`, `\\\\\\"`, long runs of backslashes.
- Extreme depth (stack tolerance).
- Extreme width (10K+ keys / array elements).

**Phase 2 correctness:**
- Path syntax variants and parsing failures.
- Non-existence at each path depth.
- Type-mismatch at each typed getter.
- Full escape-decode coverage including surrogate pairs (`😀`).
- Numeric boundaries: `INT64_MIN`, `INT64_MAX`, `2^63`, `1.7e308`, JSON-illegal forms.
- Wide objects (5K keys) → skip-cache correctness.

**FFI boundary:**
- Every entry point handles NULL pointers gracefully.
- `qjd_free(NULL)` is a no-op.
- `qjd_parse` failure path correctly populates `err_out`.
- Internal panic surfaces as `QJD_OOM`, not unwinding.

**Lua wrapper (busted):**
- `nil` on `NOT_FOUND`, `error()` on other failures.
- GC of `Doc` triggers `qjd_free`.
- Original JSON string is held against premature GC.
- Same-fixture value-equivalence with `lua-cjson`.

### 9.3 Benchmark

`benches/lua_bench.lua` directly compares against `lua-cjson` on the same fixtures using `os.clock()` and `collectgarbage('count')` for allocation pressure. No busted involvement (busted overhead is unsuitable for microbenchmarks).

**Fixtures:**
- `small_api.json` (~5 KB, LLM API request shape)
- `medium_resp.json` (~200 KB)
- `large_dump.json` (~20 MB)
- `deep_nest.json` (depth stress test)

**Acceptance targets (first cut; revise after measurement):**

| Scenario | Target | vs lua-cjson |
|---|---|---|
| 200 KB / 5 fields | Phase 1 ≥ 800 MB/s | 3-5× faster |
| 20 MB / 5 fields | Phase 1 ≥ 2 GB/s (AVX2) | 5-10× faster |
| Cursor repeated access | < 200 ns / get_str (AVX2) | — |

### 9.4 CI

- `cargo test --features default` (scalar + AVX2)
- `cargo test --no-default-features` (scalar only, simulates non-AVX2 host)
- `busted tests/lua/` after building the `.so`
- Short fuzz runs (1–5 min) per push

---

## 10. Roadmap / Deferred

Tracked in `README.md` and to be picked up individually. Items deferred from this design:

- **ARM64 NEON scanner backend** — for Apple Silicon, Graviton, 鲲鹏.
- **SmallVec fast path for small documents** (< 4 KB) — avoid heap allocation for `indices` on tiny inputs.
- **SIMD-accelerated backslash search** in the `decode_string` fast path.
- **`lexical` fast float parser** if `<f64>::from_str` benchmarks as a bottleneck.
- **Lossless 64-bit integer mode** — return cdata `int64_t` to preserve precision > 2⁵³.
- **Skip-cache LRU eviction** — only if memory pressure on huge documents proves problematic in practice.
- **Path-position info on Phase 1 errors** — currently only an opaque `QJD_PARSE_ERROR`.

---

## 11. Open Questions for Implementation Plan

The implementation plan (next phase) should resolve:

1. Exact crate features and Cargo.toml shape (workspace vs single crate? feature flags for scalar-only builds?).
2. Choice of `proptest` vs `quickcheck`.
3. Whether to vendor `memchr` and `rustc-hash` or add as direct dependencies.
4. Whether `cargo fuzz` integration runs in CI or only on-demand.
5. Build flow for LuaJIT tests (must build `.so` first; how to chain `cargo build` → `busted`).

These are tactical decisions deferred to the implementation plan.
