# lua-quick-decode

Rust-implemented fast JSON decoder exposed to LuaJIT via FFI. Optimized for the common case where a large JSON is parsed once and only a small number of fields are extracted before the document is discarded.

Design document: `docs/superpowers/specs/2026-05-15-rust-quick-json-decode-design.md` (in progress).

## Status

Currently in design phase. No implementation yet.

## Roadmap / Deferred

Items intentionally pushed out of the first implementation. Each will be picked up individually.

- **ARM64 NEON scanner backend** — first version ships with scalar + AVX2 backends only. NEON backend (for Apple Silicon / Graviton / 鲲鹏) is deferred.
- *(more deferred items will be added as design and planning proceed)*
