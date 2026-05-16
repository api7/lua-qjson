# Overridable: `make bench LUAJIT=/path/to/luajit LUA_CPATH='...'`
LUAJIT    ?= $(shell command -v luajit 2>/dev/null || echo /usr/local/openresty/luajit/bin/luajit)
LUA_CPATH ?= ./vendor/lua-cjson/?.so;./?.so;/usr/local/openresty/lualib/?.so;/usr/local/lib/lua/5.1/?.so;/usr/local/openresty/luajit/lib/lua/5.1/?.so

LUAJIT_PREFIX ?= $(shell dirname $$(dirname $$(command -v $(LUAJIT) 2>/dev/null || echo /usr/local/openresty/luajit/bin/luajit)))
LUAJIT_INC    ?= $(LUAJIT_PREFIX)/include/luajit-2.1

LIB_DIR := $(CURDIR)/target/release
ifeq ($(shell uname),Darwin)
LUA_ENV := DYLD_LIBRARY_PATH=$(LIB_DIR) LUA_CPATH='$(LUA_CPATH)'
else
LUA_ENV := LD_LIBRARY_PATH=$(LIB_DIR) LUA_CPATH='$(LUA_CPATH)'
endif

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

lint: ## Run clippy with -D warnings
	cargo clippy --release --all-targets -- -D warnings

bench: build vendor/lua-cjson/cjson.so ## Run the LuaJIT vs cjson benchmark
	$(LUA_ENV) $(LUAJIT) benches/lua_bench.lua

vendor/lua-cjson/cjson.so: | vendor/lua-cjson/Makefile
ifeq ($(shell uname),Darwin)
	$(MAKE) -C vendor/lua-cjson PREFIX=$(LUAJIT_PREFIX) LUA_INCLUDE_DIR=$(LUAJIT_INC) LUA=$(LUAJIT) CJSON_LDFLAGS="-bundle -undefined dynamic_lookup"
else
	$(MAKE) -C vendor/lua-cjson PREFIX=$(LUAJIT_PREFIX) LUA_INCLUDE_DIR=$(LUAJIT_INC) LUA=$(LUAJIT)
endif

clean: ## Remove build artifacts
	cargo clean
