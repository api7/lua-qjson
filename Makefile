# Overridable: `make bench LUAJIT=/path/to/luajit RESTY=/path/to/resty LUA_CPATH='...'`
OPENRESTY ?= /usr/local/openresty
OPENRESTY_LUAJIT := $(OPENRESTY)/luajit/bin/luajit
OPENRESTY_RESTY  := $(OPENRESTY)/bin/resty
LUAJIT    ?= $(shell if [ -x "$(OPENRESTY_LUAJIT)" ]; then echo "$(OPENRESTY_LUAJIT)"; else command -v luajit 2>/dev/null || echo luajit; fi)
RESTY     ?= $(shell if [ -x "$(OPENRESTY_RESTY)" ]; then echo "$(OPENRESTY_RESTY)"; else command -v resty 2>/dev/null || echo resty; fi)
LUA_PATH  ?= ./lua/?.lua;$(OPENRESTY)/lualib/?.lua;$(OPENRESTY)/lualib/?/init.lua;;
LUA_CPATH ?= ./vendor/lua-cjson/?.so;./target/release/lib?.so;./?.so;$(OPENRESTY)/lualib/?.so;/usr/local/lib/lua/5.1/?.so;$(OPENRESTY)/luajit/lib/lua/5.1/?.so

LUAJIT_PREFIX ?= $(shell dirname $$(dirname $$(command -v $(LUAJIT) 2>/dev/null || echo $(OPENRESTY_LUAJIT))))
LUAJIT_INC    ?= $(LUAJIT_PREFIX)/include/luajit-2.1
QJSON_PROP_CASES ?= 200
QJSON_PROP_SEED  ?= 760076
QJSON_MUT_PROP_CASES ?= 200
QJSON_MUT_PROP_SEED  ?= 104104
QJSON_MUT_PROP_STEPS ?= 24

LIB_DIR := $(CURDIR)/target/release
ifeq ($(shell uname),Darwin)
LUA_ENV := DYLD_LIBRARY_PATH=$(LIB_DIR) LUA_PATH='$(LUA_PATH)' LUA_CPATH='$(LUA_CPATH)'
else
LUA_ENV := LD_LIBRARY_PATH=$(LIB_DIR) LUA_PATH='$(LUA_PATH)' LUA_CPATH='$(LUA_CPATH)'
endif

.PHONY: help build test lua-property-test lua-mutation-property-test lint lua-lint bench clean

help: ## Show this help
	@# FS uses [^#]* (not .*) so a description containing `##` isn't truncated.
	@# Consequence: targets whose prerequisite list contains `#` won't render — none today.
	@awk 'BEGIN {FS = ":[^#]*## "} /^[a-zA-Z_-]+:[^#]*## / {printf "  \033[36m%-10s\033[0m — %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build the release cdylib (target/release/libqjson.so)
	cargo build --release

test: build ## Run cargo tests + busted Lua tests
	cargo test --release
	$(LUA_ENV) busted --lua=$(LUAJIT) tests/lua --lpath='./lua/?.lua'

lua-property-test: build ## Run deterministic Lua encode/materialize property tests
	QJSON_PROP_CASES=$(QJSON_PROP_CASES) QJSON_PROP_SEED=$(QJSON_PROP_SEED) \
		$(LUA_ENV) busted --lua=$(LUAJIT) tests/lua/encode_property_spec.lua --lpath='./lua/?.lua'

lua-mutation-property-test: build ## Run deterministic Lua lazy-mutation property tests
	QJSON_MUT_PROP_CASES=$(QJSON_MUT_PROP_CASES) QJSON_MUT_PROP_SEED=$(QJSON_MUT_PROP_SEED) \
	QJSON_MUT_PROP_STEPS=$(QJSON_MUT_PROP_STEPS) \
		$(LUA_ENV) busted --lua=$(LUAJIT) tests/lua/lazy_mutation_property_spec.lua --lpath='./lua/?.lua'

lint: ## Run clippy with -D warnings
	cargo clippy --release --all-targets -- -D warnings

lua-lint: ## Run luacheck over Lua sources and tests
	@command -v luacheck >/dev/null 2>&1 || { \
		echo "luacheck not found. Install it with: luarocks install luacheck" >&2; \
		exit 127; \
	}
	luacheck lua tests/lua

BENCH_SCENARIOS := small medium github-100k 100k 200k 500k 1m 2m 5m 10m interleaved

bench: build vendor/lua-cjson/cjson.so ## Run each scenario in a fresh LuaJIT process
	@for s in $(BENCH_SCENARIOS); do \
		$(LUA_ENV) $(RESTY) benches/lua_bench.lua $$s; \
	done

vendor/lua-cjson/cjson.so: | vendor/lua-cjson/Makefile
ifeq ($(shell uname),Darwin)
	$(MAKE) -C vendor/lua-cjson PREFIX=$(LUAJIT_PREFIX) LUA_INCLUDE_DIR=$(LUAJIT_INC) LUA=$(LUAJIT) CJSON_LDFLAGS="-bundle -undefined dynamic_lookup"
else
	$(MAKE) -C vendor/lua-cjson PREFIX=$(LUAJIT_PREFIX) LUA_INCLUDE_DIR=$(LUAJIT_INC) LUA=$(LUAJIT)
endif

clean: ## Remove build artifacts
	cargo clean
