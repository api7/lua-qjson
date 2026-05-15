# Makefile for lua-quick-decode

Add a root-level `Makefile` that wraps the common Rust + LuaJIT workflows so contributors don't have to remember the env-var dance for bench/test.

## Targets

| Target | Action |
|---|---|
| `help` (default) | Print each target and its `## ` doc-comment via awk |
| `build` | `cargo build --release` (produces `target/release/libquickdecode.so`) |
| `test` | Depends on `build`. Runs `cargo test --release`, then `busted tests/lua --lua=$(LUAJIT) --lpath='./lua/?.lua'` with `LD_LIBRARY_PATH=$(CURDIR)/target/release` and `LUA_CPATH=$(LUA_CPATH)` exported |
| `lint` | `cargo clippy --release --all-targets -- -D warnings` then `cargo fmt --check` |
| `bench` | Depends on `build`. Runs `$(LUAJIT) benches/lua_bench.lua` with the same env exports as `test` |
| `clean` | `cargo clean` |

All targets are `.PHONY`.

## Overridable variables

```make
LUAJIT    ?= $(shell command -v luajit 2>/dev/null || echo /usr/local/openresty/luajit/bin/luajit)
LUA_CPATH ?= ./?.so;/usr/local/openresty/lualib/?.so;/usr/local/lib/lua/5.1/?.so;/usr/local/openresty/luajit/lib/lua/5.1/?.so
```

- `LUAJIT` autodetects: prefers `luajit` on `PATH` (apt/CI install), falls back to OpenResty's path (local dev box).
- `LUA_CPATH` includes OpenResty's `lualib` (where `cjson.so` lives on the local box) plus the standard LuaJIT 5.1 search paths. The default value is intentionally absolute, not appended to LuaJIT's built-in default, so the Makefile is reproducible regardless of which LuaJIT build is invoked.
- Users override per invocation: `make bench LUAJIT=/path/to/luajit`.

## Help format

Each target line carries a `## description` trailing comment. The `help` target greps targets with `## ` and pretty-prints `target — description` via awk. This pattern lets `help` stay in sync automatically when a new target is added.

## Out of scope

- **`luacheck` lint for Lua sources.** Neither the local box nor CI has it installed; adding it now would be dead code. Track in README if/when desired.
- **Separate `release` / `debug` build targets.** The repo only ships release artifacts (bench and Lua FFI tests both require release). Add later if a debug workflow appears.
- **Cross-target dependency on `test` from `bench`.** Bench depends only on `build`; running tests as part of bench would slow down iterative perf work.

## Non-goals

- Replacing CI. The Makefile mirrors CI commands but is not invoked by CI (CI keeps its explicit steps for cache-key clarity).
- Cross-platform. macOS/Windows are not supported; the OpenResty path defaults are Linux-specific. PRs welcome but not required for v1.

## Failure modes (intentional, loud)

- `busted` not installed → `test` fails with a clear `command not found`. Fix: `sudo luarocks install busted`.
- `luajit` not on PATH and OpenResty fallback missing → `bench` and `test` fail at the luajit invocation. Fix: install LuaJIT or pass `LUAJIT=...`.
- `target/release/libquickdecode.so` missing → impossible by construction; `bench` and `test` depend on `build`.
