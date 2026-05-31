#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y lua5.1 liblua5.1-0-dev luarocks

# Ubuntu LuaRocks targets Lua 5.1 by default; LuaJIT is ABI-compatible
# with 5.1 so rocks built for 5.1 load fine under each runtime.
sudo /usr/bin/luarocks install busted
sudo /usr/bin/luarocks install lua-cjson
