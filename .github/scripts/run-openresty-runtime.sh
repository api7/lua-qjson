#!/usr/bin/env bash
set -euo pipefail

: "${OPENRESTY_IMAGE:?OPENRESTY_IMAGE is required}"

workspace="${GITHUB_WORKSPACE:-$(pwd)}"
libqjson="$workspace/target/release/libqjson.so"
full_busted="${OPENRESTY_FULL_BUSTED:-false}"

if [[ ! -f "$libqjson" ]]; then
    echo "missing release cdylib: $libqjson" >&2
    exit 1
fi

docker run --rm \
    -e DEBIAN_FRONTEND=noninteractive \
    -e OPENRESTY_FULL_BUSTED="$full_busted" \
    -v "$workspace:/workspace" \
    -w /workspace \
    "$OPENRESTY_IMAGE" \
    bash -lc '
set -euo pipefail

OPENRESTY=/usr/local/openresty
RESTY="$OPENRESTY/bin/resty"
LUAJIT="$OPENRESTY/luajit/bin/luajit"

test -x "$RESTY"
test -x "$LUAJIT"

"$RESTY" -V
"$LUAJIT" -e '\''assert(jit, "LuaJIT required"); print(jit.version)'\''

export LD_LIBRARY_PATH=/workspace/target/release
export LUA_PATH="/workspace/lua/?.lua;;"

"$RESTY" /workspace/.github/scripts/openresty-smoke.lua

if [ "${OPENRESTY_FULL_BUSTED:-false}" = "true" ]; then
    apt-get update
    apt-get install -y --no-install-recommends \
        ca-certificates \
        liblua5.1-0-dev \
        lua5.1 \
        luarocks

    # Use Ubuntu LuaRocks here. The OpenResty-bundled luarocks command runs under
    # LuaJIT and can fail to load the current public manifest due to bytecode limits.
    /usr/bin/luarocks install busted
    /usr/bin/luarocks install lua-cjson

    BUSTED="$(command -v busted)"
    test -x "$BUSTED"
    "$BUSTED" --lua="$LUAJIT" /workspace/tests/lua \
        --lpath="/workspace/lua/?.lua"
fi
'
