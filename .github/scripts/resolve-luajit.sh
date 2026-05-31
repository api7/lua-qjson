#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -eq 0 ]; then
    set -- luajit luajit-2.1.0-beta3 lua
fi

for candidate in "$@"; do
    if command -v "$candidate" >/dev/null 2>&1; then
        lua_bin="$(command -v "$candidate")"
        if "$lua_bin" -e 'assert(jit, "LuaJIT required")' >/dev/null 2>&1; then
            if [ -n "${GITHUB_OUTPUT:-}" ]; then
                echo "path=$lua_bin" >> "$GITHUB_OUTPUT"
            fi
            "$lua_bin" -e 'print(jit.version)'
            exit 0
        fi
    fi
done

echo "LuaJIT executable not found" >&2
exit 1
