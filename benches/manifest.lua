-- Shared loader for the real-world fixture manifest (issue #139).
--
-- The manifest at tests/fixtures/manifest.json is the single source of truth
-- for fixture paths, payload classes, access paths and expected values. The
-- Rust correctness gate (tests/manifest_fixtures.rs) and this Lua benchmark
-- helper both read it, so paths and scenarios live in exactly one place.

local cjson = require("cjson")

local M = {}

local function read_file(p)
    local f = assert(io.open(p, "rb"))
    local s = f:read("*a")
    f:close()
    return s
end

-- Decode the manifest. `path` is repo-root relative; the bench is launched from
-- the repository root (see the Makefile `bench` target).
function M.load(path)
    return cjson.decode(read_file(path or "tests/fixtures/manifest.json"))
end

-- Index fixtures by id for direct lookup.
function M.by_id(manifest)
    local m = {}
    for _, f in ipairs(manifest.fixtures) do
        m[f.id] = f
    end
    return m
end

-- True if `fixture.ci` contains `tag` (pr | scheduled | bench).
function M.has_ci(fixture, tag)
    for _, c in ipairs(fixture.ci or {}) do
        if c == tag then return true end
    end
    return false
end

-- Build an access function over a qjson Doc that touches every check path with
-- the getter matching its declared type. This is the manifest-driven
-- replacement for hand-written per-fixture access closures.
function M.qjson_access(checks)
    return function(d)
        for i = 1, #checks do
            local c = checks[i]
            local t = c.type
            if t == "string" then
                local _ = d:get_str(c.path)
            elseif t == "number" then
                local _ = d:get_f64(c.path)
            elseif t == "bool" then
                local _ = d:get_bool(c.path)
            elseif t == "null" then
                local _ = d:is_null(c.path)
            else -- object | array
                local _ = d:typeof(c.path)
            end
            if c.len ~= nil then
                local _ = d:len(c.path)
            end
        end
    end
end

return M
