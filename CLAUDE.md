# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Rust JSON decoder (`cdylib` + `rlib`) exposed to LuaJIT via FFI. Optimized for parse-once / extract-a-few-fields / discard. The competitive edge over `lua-cjson` comes from **never building a Lua table** — Phase 1 records only structural offsets, Phase 2 lazily decodes the fields the caller actually asks for. Crate name in `Cargo.toml` is `lua-quick-decode`; the compiled artifact is `libquickdecode.so`.

Authoritative design doc: `docs/superpowers/specs/2026-05-15-rust-quick-json-decode-design.md`.

## Common commands

The `Makefile` is the canonical entry point; `make help` lists targets.

```sh
make build              # cargo build --release  → target/release/libquickdecode.so
make test               # cargo test --release + busted Lua tests
make lint               # cargo clippy -D warnings + cargo fmt --check
make bench              # LuaJIT vs lua-cjson on benches/fixtures
```

Under the hood / for narrower invocations:

```sh
# Single Rust integration test
cargo test --release --test ffi_smoke parse_and_free_roundtrip

# Single Rust unit test (e.g. inside src/doc.rs)
cargo test --release doc::tests::parses_simple_object

# Lua tests bypassing the Makefile
LD_LIBRARY_PATH=./target/release \
  busted --lua=$(command -v luajit) tests/lua --lpath='./lua/?.lua'

# Scalar-only test run (no SIMD) — CI runs this gate
cargo test --release --no-default-features

# Force the FFI panic-barrier code path
cargo test --features test-panic --release
```

`ffi.load("quickdecode")` uses `dlopen`, which respects `LD_LIBRARY_PATH` — **not** LuaJIT's `package.cpath`. The Makefile sets `LD_LIBRARY_PATH=target/release` for `test`/`bench`; if you invoke `busted` or `luajit` directly, set it yourself.

`make lint` runs clippy only (with `-D warnings`); `cargo fmt --check` is intentionally **not** part of the lint gate because the codebase uses manual column alignment in struct definitions and compact single-line literals that default rustfmt would reflow. See the README "Roadmap / Deferred" entry on fmt for context.

## Architecture

### Two-phase parse

**Phase 1** (`src/scan/`, called from `Document::parse`): a structural scanner walks the input once and writes the byte offset of every non-string-interior `{ } [ ] : , "` into `doc.indices: Vec<u32>`. A `u32::MAX` sentinel is appended. The scanner is selected at first use via `OnceCell` in `src/scan/mod.rs`:

- `Avx2Scanner` (gated by the `avx2` cargo feature, default-on) when both `avx2` and `pclmulqdq` are detected at runtime.
- `ScalarScanner` otherwise.

Validation is shallow — bracket/quote balance only. Value-level errors (bad escapes, malformed numbers, invalid UTF-8 in `\u`) are deferred to Phase 2 and surface only if that field is accessed.

**Phase 2** (`src/cursor.rs`, `src/path.rs`, `src/decode/`): path strings are parsed by a zero-alloc `PathIter` into `PathSeg::Key | Idx`. A `Cursor` (a `(idx_start, idx_end)` pair into `doc.indices`) is walked to the target, optionally caching sibling spans in `doc.skip` (`SkipCache`) so repeated lookups on the same container skip brace-counting. Strings are decoded into `doc.scratch` only when they contain escapes; otherwise the original buffer slice is handed back.

### Critical invariants (these will bite you if violated)

- **`get_str` pointer lifetime.** The `(ptr, len)` returned by `qjd_get_str` / `qjd_cursor_get_str` points into either the original input buffer or `doc.scratch`. **Any subsequent `*_get_str` call on the same doc may invalidate prior pointers** (scratch buffer reuse). The LuaJIT wrapper preserves this contract by calling `ffi.string(ptr, len)` immediately to copy into a Lua string — do not change that.
- **Buffer lifetime.** `Document<'a>` borrows the input slice. `qjd_parse` transmutes `'a` to `'static` and trusts the caller to keep the buffer alive for the document's lifetime. The LuaJIT wrapper enforces this by stashing the original string under `_hold` on the Doc table so Lua GC keeps it pinned.
- **`indices` stores offsets only, not types.** Token type is recovered from `buf[indices[i]]`. Do not add a type tag — the 25% memory win is intentional.
- **Single-threaded.** `qjd_doc` is not Sync/Send across threads; `RefCell` is used for `scratch` and `skip`.
- **FFI panic barrier.** Every `pub unsafe extern "C"` function in `src/ffi.rs` wraps its body in `catch_unwind` and converts a panic into `QJD_OOM`. Preserve this pattern on any new export — a panic crossing the FFI boundary is undefined behavior.

### Layout

```
src/
  lib.rs          crate root
  ffi.rs          extern "C" surface, qjd_* symbols, panic barrier
  doc.rs          Document (indices + scratch + skip cache)
  cursor.rs       Cursor + path resolution + skip-cache walk
  path.rs         zero-alloc path-string iterator
  decode/         lazy string / number decode
  scan/           ScalarScanner, Avx2Scanner, runtime dispatch
  skip_cache.rs   Phase 2 sibling-skip cache
  error.rs        qjd_err + qjd_type enums (must stay in sync with include/lua_quick_decode.h and lua/quickdecode.lua)

lua/quickdecode.lua    LuaJIT wrapper (ffi.cdef + Doc/Cursor metatables)
include/lua_quick_decode.h  public C header
tests/                Rust integration tests + tests/lua/ busted suite
benches/              lua_bench.lua vs lua-cjson; fixtures/ has small_api.json + medium_resp.json
```

The enum values in `src/error.rs` are duplicated in `include/lua_quick_decode.h` and `lua/quickdecode.lua` (the latter only encodes the `T_*` type tags and `NOT_FOUND = 2`). Keep all three in sync when adding/renumbering codes.

### CI gates worth knowing

`.github/workflows/ci.yml` runs three Rust matrix points and one Lua job:

1. `cargo test --release` (default features → AVX2 on, falls through to scalar on non-AVX2 hardware at runtime)
2. `cargo test --release --no-default-features` — scalar scanner only; catches AVX2-vs-scalar divergence
3. `cargo test --features test-panic --release` — exercises the FFI panic barrier
4. Lua busted suite under LuaJIT (depends on the Rust job passing)

If you add a scanner code path, run gate 2 locally; the cross-check test (`tests/scanner_crosscheck.rs`) is the main defense against backend drift and uses `proptest` — the `.proptest-regressions` file is intentionally committed.

## Conventions

- Deferred / "we'll do this later" decisions go in `README.md` under **Roadmap / Deferred**, one bullet per item, so each can be picked up individually. Don't park them in code comments or scratch files.
