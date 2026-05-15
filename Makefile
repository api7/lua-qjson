# Overridable: `make bench LUAJIT=/path/to/luajit LUA_CPATH='...'`
LUAJIT    ?= $(shell command -v luajit 2>/dev/null || echo /usr/local/openresty/luajit/bin/luajit)
LUA_CPATH ?= ./?.so;/usr/local/openresty/lualib/?.so;/usr/local/lib/lua/5.1/?.so;/usr/local/openresty/luajit/lib/lua/5.1/?.so

LIB_DIR := $(CURDIR)/target/release
LUA_ENV := LD_LIBRARY_PATH=$(LIB_DIR) LUA_CPATH='$(LUA_CPATH)'

.PHONY: help build test lint bench clean

help: ## Show this help
	@# FS uses [^#]* (not .*) so a description containing `##` isn't truncated.
	@# Consequence: targets whose prerequisite list contains `#` won't render — none today.
	@awk 'BEGIN {FS = ":[^#]*## "} /^[a-zA-Z_-]+:[^#]*## / {printf "  \033[36m%-10s\033[0m — %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build the release cdylib (target/release/libquickdecode.so)
	cargo build --release

test: build ## Run cargo tests + busted Lua tests
	cargo test --release
	$(LUA_ENV) busted --lua=$(LUAJIT) tests/lua --lpath='./lua/?.lua'

lint: ## Run clippy (deny warnings) and rustfmt --check
	cargo clippy --release --all-targets -- -D warnings
	cargo fmt --check

bench: build ## Run the LuaJIT vs cjson benchmark
	$(LUA_ENV) $(LUAJIT) benches/lua_bench.lua

clean: ## Remove build artifacts
	cargo clean
