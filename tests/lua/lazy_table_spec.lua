local qjson = require("qjson")

describe("LazyObject __index — scalars", function()
    it("reads a string field", function()
        local t = qjson.decode('{"k":"hello"}')
        assert.are.equal("hello", t.k)
    end)

    it("reads a number field", function()
        local t = qjson.decode('{"n":42.5}')
        assert.are.equal(42.5, t.n)
    end)

    it("reads a boolean field", function()
        local t = qjson.decode('{"b":true,"c":false}')
        assert.is_true(t.b)
        assert.is_false(t.c)
    end)

    it("returns nil for missing key", function()
        local t = qjson.decode('{"a":1}')
        assert.is_nil(t.missing)
    end)
end)

describe("LazyObject __index — nested containers", function()
    it("returns a LazyObject for a nested object", function()
        local t = qjson.decode('{"a":{"b":"x"}}')
        local inner = t.a
        assert.is_table(inner)
        assert.are.equal("x", inner.b)
    end)

    it("returns a LazyArray for a nested array", function()
        local t = qjson.decode('{"xs":[10,20]}')
        local xs = t.xs
        assert.is_table(xs)
        -- LazyArray __index is added in a later task; just verify it's
        -- a table-typed value at this stage.
    end)
end)

describe("LazyArray __index", function()
    it("reads scalar elements by integer index (1-based)", function()
        local t = qjson.decode('[10,"x",true,null]')
        assert.are.equal(10, t[1])
        assert.are.equal("x", t[2])
        assert.is_true(t[3])
        assert.are.equal(qjson.null, t[4])
    end)

    it("returns nil for out-of-range index", function()
        local t = qjson.decode('[1,2,3]')
        assert.is_nil(t[0])
        assert.is_nil(t[4])
    end)

    it("returns nil for non-integer key", function()
        local t = qjson.decode('[1,2,3]')
        assert.is_nil(t.foo)
        assert.is_nil(t[1.5])
    end)

    it("returns a nested LazyObject", function()
        local t = qjson.decode('[{"a":1},{"a":2}]')
        assert.are.equal(1, t[1].a)
        assert.are.equal(2, t[2].a)
    end)
end)

-- LuaJIT 5.1 only invokes __len on userdata; it ignores the metamethod on
-- tables unless built with LUAJIT_ENABLE_LUA52COMPAT (OpenResty's default).
-- Probe once so the `#t` cases only run where they can pass; qjson.len(t) is
-- the supported path everywhere.
local LJ52_LEN = (#setmetatable({}, {__len = function() return 99 end}) == 99)

describe("qjson.len", function()
    it("counts object keys", function()
        local t = qjson.decode('{"a":1,"b":2,"c":3}')
        assert.are.equal(3, qjson.len(t))
    end)

    it("counts array elements", function()
        local t = qjson.decode('[10,20,30,40]')
        assert.are.equal(4, qjson.len(t))
    end)

    it("returns 0 for empty containers", function()
        assert.are.equal(0, qjson.len(qjson.decode('{}')))
        assert.are.equal(0, qjson.len(qjson.decode('[]')))
    end)

    it("falls back to # on a plain table", function()
        assert.are.equal(3, qjson.len({10, 20, 30}))
    end)
end)

describe("__len (LJ52 only)", function()
    it("counts object keys via #t", function()
        if not LJ52_LEN then return pending("LuaJIT built without LUAJIT_ENABLE_LUA52COMPAT") end
        local t = qjson.decode('{"a":1,"b":2,"c":3}')
        assert.are.equal(3, #t)
    end)

    it("counts array elements via #t", function()
        if not LJ52_LEN then return pending("LuaJIT built without LUAJIT_ENABLE_LUA52COMPAT") end
        local t = qjson.decode('[10,20,30,40]')
        assert.are.equal(4, #t)
    end)

    it("returns 0 for empty containers via #t", function()
        if not LJ52_LEN then return pending("LuaJIT built without LUAJIT_ENABLE_LUA52COMPAT") end
        assert.are.equal(0, #qjson.decode('{}'))
        assert.are.equal(0, #qjson.decode('[]'))
    end)
end)

describe("__pairs / qjson.pairs over LazyObject", function()
    it("iterates string keys in source order", function()
        local t = qjson.decode('{"a":1,"b":2,"c":3}')
        local keys = {}
        local values = {}
        for k, v in qjson.pairs(t) do
            keys[#keys+1] = k
            values[#values+1] = v
        end
        assert.are.same({"a","b","c"}, keys)
        assert.are.same({1, 2, 3}, values)
    end)

    it("returns nested containers as lazy proxies, not materialized", function()
        local t = qjson.decode('{"a":{"x":1}}')
        for _, v in qjson.pairs(t) do
            assert.is_table(v)
            assert.are.equal(1, v.x)
        end
    end)

    it("handles empty object", function()
        local count = 0
        for _ in qjson.pairs(qjson.decode('{}')) do count = count + 1 end
        assert.are.equal(0, count)
    end)
end)

describe("__ipairs / qjson.ipairs over LazyArray", function()
    it("iterates elements 1..n in order", function()
        local t = qjson.decode('[10,20,30]')
        local got = {}
        for i, v in qjson.ipairs(t) do got[i] = v end
        assert.are.same({10,20,30}, got)
    end)

    it("yields lazy proxies for nested containers", function()
        local t = qjson.decode('[{"a":1},{"a":2}]')
        local seen = {}
        for _, v in qjson.ipairs(t) do
            assert.is_table(v)
            seen[#seen+1] = v.a
        end
        assert.are.same({1, 2}, seen)
    end)

    it("handles empty array", function()
        local count = 0
        for _ in qjson.ipairs(qjson.decode('[]')) do count = count + 1 end
        assert.are.equal(0, count)
    end)
end)

describe("__newindex — first-write materialization", function()
    it("preserves LazyObject metatable after modification for ordered encoding", function()
        local t = qjson.decode('{"a":1,"b":2}')
        t.c = 3
        assert.are.equal(qjson._LazyObject, getmetatable(t))
        assert.are.equal(1, t.a)
        assert.are.equal(2, t.b)
        assert.are.equal(3, t.c)
    end)

    it("nested containers remain lazy after parent modification", function()
        local t = qjson.decode('{"inner":{"x":1}}')
        t.extra = "y"
        assert.are.equal(qjson._LazyObject, getmetatable(t))
        local inner = t.inner
        assert.are.equal(qjson._LazyObject, getmetatable(inner))
        assert.are.equal(1, inner.x)
    end)

    it("LazyArray materializes preserving empty_array_mt", function()
        local t = qjson.decode('[]')
        t[1] = "x"
        assert.are.equal(qjson.empty_array_mt, getmetatable(t))
        assert.are.equal("x", t[1])
    end)

    it("simple write leaves other keys intact", function()
        local t = qjson.decode('{"a":1}')
        t.b = 2
        assert.are.equal(1, t.a)
        assert.are.equal(2, t.b)
    end)
end)

describe("qjson.materialize", function()
    it("converts a LazyObject and its nested containers into real tables", function()
        local m = qjson.materialize(qjson.decode('{"a":1,"b":{"c":[10,20]}}'))
        assert.is_nil(getmetatable(m))
        assert.are.equal(1, m.a)
        assert.is_nil(getmetatable(m.b))
        assert.are.equal(10, m.b.c[1])
        assert.are.equal(20, m.b.c[2])
    end)

    it("tags empty arrays with empty_array_mt", function()
        local m = qjson.materialize(qjson.decode('[]'))
        assert.are.equal(qjson.empty_array_mt, getmetatable(m))
    end)

    it("preserves cjson.null", function()
        local m = qjson.materialize(qjson.decode('{"x":null}'))
        assert.are.equal(qjson.null, m.x)
    end)

    it("passes through scalars and plain tables unchanged", function()
        assert.are.equal(42, qjson.materialize(42))
        assert.are.equal("hi", qjson.materialize("hi"))
        local raw = {1, 2, 3}
        assert.are.equal(raw, qjson.materialize(raw))
    end)
end)

describe("qjson.encode — lazy proxy substring fast path", function()
    it("re-emits the original JSON for an unmodified LazyObject", function()
        local src = '{"a":1,"b":[2,3],"c":"x"}'
        local t = qjson.decode(src)
        assert.are.equal(src, qjson.encode(t))
    end)

    it("re-emits the original JSON for an unmodified LazyArray", function()
        local src = '[10,20,{"k":"v"}]'
        local t = qjson.decode(src)
        assert.are.equal(src, qjson.encode(t))
    end)

    it("trims leading/trailing whitespace at the boundary", function()
        local src = '  {"a":1}  '
        local t = qjson.decode(src)
        -- byte span is the value, not its outer whitespace.
        assert.are.equal('{"a":1}', qjson.encode(t))
    end)
end)

describe("qjson.encode — scalars", function()
    it("encodes strings with JSON escapes", function()
        assert.are.equal('"hello"', qjson.encode("hello"))
        assert.are.equal('"a\\nb"', qjson.encode("a\nb"))
        assert.are.equal('"a\\"b"', qjson.encode('a"b'))
        assert.are.equal('"a\\\\b"', qjson.encode("a\\b"))
    end)

    it("encodes booleans", function()
        assert.are.equal("true", qjson.encode(true))
        assert.are.equal("false", qjson.encode(false))
    end)

    it("encodes numbers", function()
        assert.are.equal("42", qjson.encode(42))
        assert.are.equal("-3.14", qjson.encode(-3.14))
    end)

    it("encodes qjson.null as JSON null", function()
        assert.are.equal("null", qjson.encode(qjson.null))
    end)

    it("errors on unsupported values", function()
        assert.has_error(function() qjson.encode(function() end) end)
    end)
end)

describe("qjson.encode — real and mixed tables", function()
    it("encodes a real Lua object", function()
        local cjson = require("cjson")
        local s = qjson.encode({a = 1, b = "x"})
        assert.are.same({a = 1, b = "x"}, cjson.decode(s))
    end)

    it("encodes a real Lua array", function()
        assert.are.equal("[1,2,3]", qjson.encode({1,2,3}))
    end)

    it("encodes a hand-built empty array with empty_array_mt", function()
        local arr = setmetatable({}, qjson.empty_array_mt)
        assert.are.equal("[]", qjson.encode(arr))
    end)

    it("encodes mixed lazy + materialized", function()
        local t = qjson.decode('{"keep":{"x":1},"changed":{"y":2}}')
        t.changed = "now a string"
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.same({x=1}, parsed.keep)
        assert.are.equal("now a string", parsed.changed)
    end)
end)

local cjson = require("cjson")

-- Deep-equal aware of cjson.null and empty_array_mt (which qjson aliases).
local function deep_equal(a, b)
    if a == b then return true end
    if type(a) ~= "table" or type(b) ~= "table" then return false end
    for k, v in pairs(a) do
        if not deep_equal(v, b[k]) then return false end
    end
    for k in pairs(b) do
        if a[k] == nil then return false end
    end
    return true
end

describe("cjson round-trip equivalence", function()
    local fixtures = {
        '{"a":1,"b":"x","c":null,"d":true,"e":false,"f":[1,2,3],"g":{"h":4.5}}',
        '[1,"x",true,null,{},[]]',
        '{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}',
        '{}',
        '[]',
        '{"escapes":"a\\nb\\tc\\\"d\\\\e"}',
    }
    for _, src in ipairs(fixtures) do
        it("materialize matches cjson.decode for: " .. src:sub(1, 40), function()
            local from_qjson = qjson.materialize(qjson.decode(src))
            local from_cj = cjson.decode(src)
            assert.is_true(deep_equal(from_qjson, from_cj))
        end)

        it("encode round-trips for: " .. src:sub(1, 40), function()
            local out = qjson.encode(qjson.decode(src))
            local back_qjson = cjson.decode(out)
            local back_cj = cjson.decode(src)
            assert.is_true(deep_equal(back_qjson, back_cj))
        end)
    end
end)

describe("sentinel handling", function()
    it("JSON null reads as qjson.null and encodes back", function()
        local t = qjson.decode('{"x":null}')
        assert.are.equal(qjson.null, t.x)
        assert.are.equal('{"x":null}', qjson.encode(t))
    end)

    it("empty array stays an array through materialize and encode", function()
        local t = qjson.decode('{"xs":[]}')
        local m = qjson.materialize(t)
        assert.are.equal(qjson.empty_array_mt, getmetatable(m.xs))
        assert.are.equal('{"xs":[]}', qjson.encode(t))
    end)
end)

describe("qjson.encode — nested mutations propagate", function()
    it("emits nested object mutation, not original bytes", function()
        local cjson = require("cjson")
        local t = qjson.decode('{"a":{"b":{"c":1}},"d":2}')
        t.a.b.c = 999
        local out = qjson.encode(t)
        local parsed = cjson.decode(out)
        assert.are.equal(999, parsed.a.b.c)
        assert.are.equal(2, parsed.d)
    end)

    it("emits nested array mutation", function()
        local cjson = require("cjson")
        local t = qjson.decode('{"xs":[10,20,30]}')
        t.xs[2] = 999
        local out = qjson.encode(t)
        local parsed = cjson.decode(out)
        assert.are.equal(10, parsed.xs[1])
        assert.are.equal(999, parsed.xs[2])
        assert.are.equal(30, parsed.xs[3])
    end)

    it("preserves cached proxy identity across parent materialization", function()
        local t = qjson.decode('{"a":{"x":1}}')
        local inner = t.a
        t.c = 3
        assert.are.equal(inner, t.a)
        inner.x = 99
        assert.are.equal(99, t.a.x)
    end)

    it("modifies top-level field and encodes correctly", function()
        local cjson = require("cjson")
        local t = qjson.decode('{"model":"gpt-4","temperature":0.7}')
        t.model = "gpt-5"
        local out = qjson.encode(t)
        local parsed = cjson.decode(out)
        assert.are.equal("gpt-5", parsed.model)
        assert.are.equal(0.7, parsed.temperature)
    end)

    it("adds new field and encodes correctly", function()
        local cjson = require("cjson")
        local t = qjson.decode('{"a":1}')
        t.b = true
        local out = qjson.encode(t)
        local parsed = cjson.decode(out)
        assert.are.equal(1, parsed.a)
        assert.are.equal(true, parsed.b)
    end)

    it("modifies nested field and encodes correctly", function()
        local cjson = require("cjson")
        local t = qjson.decode('{"messages":[{"role":"user","content":"hello"}]}')
        t.messages[1].content = "world"
        local out = qjson.encode(t)
        local parsed = cjson.decode(out)
        assert.are.equal("user", parsed.messages[1].role)
        assert.are.equal("world", parsed.messages[1].content)
    end)

    it("encodes unmodified proxy via fast path", function()
        local json = '{"a":1,"b":"text","c":true}'
        local t = qjson.decode(json)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(1, parsed.a)
        assert.are.equal("text", parsed.b)
        assert.are.equal(true, parsed.c)
    end)

    it("encodes string with escapes correctly", function()
        local t = qjson.decode('{"key":"value"}')
        t.key = 'line1\nline2\t"quoted"'
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal('line1\nline2\t"quoted"', parsed.key)
    end)
end)
