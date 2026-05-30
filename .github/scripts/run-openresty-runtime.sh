#!/usr/bin/env bash
set -euo pipefail

: "${OPENRESTY_IMAGE:?OPENRESTY_IMAGE is required}"

workspace="${GITHUB_WORKSPACE:-$(pwd)}"
libqjson="$workspace/target/release/libqjson.so"

if [[ ! -f "$libqjson" ]]; then
    echo "missing release cdylib: $libqjson" >&2
    exit 1
fi

docker run --rm \
    -e DEBIAN_FRONTEND=noninteractive \
    -v "$workspace:/workspace" \
    -w /workspace \
    "$OPENRESTY_IMAGE" \
    bash -lc '
set -euo pipefail

OPENRESTY=/usr/local/openresty
RESTY="$OPENRESTY/bin/resty"
LUAJIT="$OPENRESTY/luajit/bin/luajit"
BUSTED="$OPENRESTY/luajit/bin/busted"

test -x "$RESTY"
test -x "$LUAJIT"

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

test -x "$BUSTED"
"$RESTY" -V >/dev/null 2>&1
"$LUAJIT" -e '\''assert(jit, "LuaJIT required"); print(jit.version)'\''

export LD_LIBRARY_PATH=/workspace/target/release
export LUA_PATH="/workspace/lua/?.lua;;"

"$RESTY" /workspace/.github/scripts/openresty-smoke.lua
"$BUSTED" --lua="$LUAJIT" /workspace/tests/lua \
    --lpath="/workspace/lua/?.lua"
'
