local qd = require("quickdecode")
local qt = require("quickdecode.table")

describe("LazyObject __index — scalars", function()
    it("reads a string field", function()
        local t = qt.decode('{"k":"hello"}')
        assert.are.equal("hello", t.k)
    end)

    it("reads a number field", function()
        local t = qt.decode('{"n":42.5}')
        assert.are.equal(42.5, t.n)
    end)

    it("reads a boolean field", function()
        local t = qt.decode('{"b":true,"c":false}')
        assert.is_true(t.b)
        assert.is_false(t.c)
    end)

    it("returns nil for missing key", function()
        local t = qt.decode('{"a":1}')
        assert.is_nil(t.missing)
    end)
end)

describe("LazyObject __index — nested containers", function()
    it("returns a LazyObject for a nested object", function()
        local t = qt.decode('{"a":{"b":"x"}}')
        local inner = t.a
        assert.is_table(inner)
        assert.are.equal("x", inner.b)
    end)

    it("returns a LazyArray for a nested array", function()
        local t = qt.decode('{"xs":[10,20]}')
        local xs = t.xs
        assert.is_table(xs)
        -- LazyArray __index is added in a later task; just verify it's
        -- a table-typed value at this stage.
    end)
end)

describe("LazyArray __index", function()
    it("reads scalar elements by integer index (1-based)", function()
        local t = qt.decode('[10,"x",true,null]')
        assert.are.equal(10, t[1])
        assert.are.equal("x", t[2])
        assert.is_true(t[3])
        assert.are.equal(qt.null, t[4])
    end)

    it("returns nil for out-of-range index", function()
        local t = qt.decode('[1,2,3]')
        assert.is_nil(t[0])
        assert.is_nil(t[4])
    end)

    it("returns nil for non-integer key", function()
        local t = qt.decode('[1,2,3]')
        assert.is_nil(t.foo)
        assert.is_nil(t[1.5])
    end)

    it("returns a nested LazyObject", function()
        local t = qt.decode('[{"a":1},{"a":2}]')
        assert.are.equal(1, t[1].a)
        assert.are.equal(2, t[2].a)
    end)
end)

describe("__len", function()
    it("counts object keys", function()
        local t = qt.decode('{"a":1,"b":2,"c":3}')
        assert.are.equal(3, #t)
    end)

    it("counts array elements", function()
        local t = qt.decode('[10,20,30,40]')
        assert.are.equal(4, #t)
    end)

    it("returns 0 for empty containers", function()
        assert.are.equal(0, #qt.decode('{}'))
        assert.are.equal(0, #qt.decode('[]'))
    end)
end)

describe("__pairs / qd.pairs over LazyObject", function()
    it("iterates string keys in source order", function()
        local t = qt.decode('{"a":1,"b":2,"c":3}')
        local keys = {}
        local values = {}
        for k, v in qt.pairs(t) do
            keys[#keys+1] = k
            values[#values+1] = v
        end
        assert.are.same({"a","b","c"}, keys)
        assert.are.same({1, 2, 3}, values)
    end)

    it("returns nested containers as lazy proxies, not materialized", function()
        local t = qt.decode('{"a":{"x":1}}')
        for _, v in qt.pairs(t) do
            assert.is_table(v)
            assert.are.equal(1, v.x)
        end
    end)

    it("handles empty object", function()
        local count = 0
        for _ in qt.pairs(qt.decode('{}')) do count = count + 1 end
        assert.are.equal(0, count)
    end)
end)

describe("__ipairs / qd.ipairs over LazyArray", function()
    it("iterates elements 1..n in order", function()
        local t = qt.decode('[10,20,30]')
        local got = {}
        for i, v in qt.ipairs(t) do got[i] = v end
        assert.are.same({10,20,30}, got)
    end)

    it("yields lazy proxies for nested containers", function()
        local t = qt.decode('[{"a":1},{"a":2}]')
        local seen = {}
        for _, v in qt.ipairs(t) do
            assert.is_table(v)
            seen[#seen+1] = v.a
        end
        assert.are.same({1, 2}, seen)
    end)

    it("handles empty array", function()
        local count = 0
        for _ in qt.ipairs(qt.decode('[]')) do count = count + 1 end
        assert.are.equal(0, count)
    end)
end)

describe("__newindex — first-write materialization", function()
    it("converts LazyObject into a plain table preserving existing keys", function()
        local t = qt.decode('{"a":1,"b":2}')
        t.c = 3
        assert.is_nil(getmetatable(t))
        assert.are.equal(1, t.a)
        assert.are.equal(2, t.b)
        assert.are.equal(3, t.c)
    end)

    it("nested containers remain lazy after parent materialization", function()
        local t = qt.decode('{"inner":{"x":1}}')
        t.extra = "y"
        assert.is_nil(getmetatable(t))
        local inner = t.inner
        assert.are.equal(qt._LazyObject, getmetatable(inner))
        assert.are.equal(1, inner.x)
    end)

    it("LazyArray materializes preserving empty_array_mt", function()
        local t = qt.decode('[]')
        t[1] = "x"
        assert.are.equal(qt.empty_array_mt, getmetatable(t))
        assert.are.equal("x", t[1])
    end)

    it("simple write leaves other keys intact", function()
        local t = qt.decode('{"a":1}')
        t.b = 2
        assert.are.equal(1, t.a)
        assert.are.equal(2, t.b)
    end)
end)

describe("qt.materialize", function()
    it("converts a LazyObject and its nested containers into real tables", function()
        local m = qt.materialize(qt.decode('{"a":1,"b":{"c":[10,20]}}'))
        assert.is_nil(getmetatable(m))
        assert.are.equal(1, m.a)
        assert.is_nil(getmetatable(m.b))
        assert.are.equal(10, m.b.c[1])
        assert.are.equal(20, m.b.c[2])
    end)

    it("tags empty arrays with empty_array_mt", function()
        local m = qt.materialize(qt.decode('[]'))
        assert.are.equal(qt.empty_array_mt, getmetatable(m))
    end)

    it("preserves cjson.null", function()
        local m = qt.materialize(qt.decode('{"x":null}'))
        assert.are.equal(qt.null, m.x)
    end)

    it("passes through scalars and plain tables unchanged", function()
        assert.are.equal(42, qt.materialize(42))
        assert.are.equal("hi", qt.materialize("hi"))
        local raw = {1, 2, 3}
        assert.are.equal(raw, qt.materialize(raw))
    end)
end)

describe("qd.encode — lazy proxy substring fast path", function()
    it("re-emits the original JSON for an unmodified LazyObject", function()
        local src = '{"a":1,"b":[2,3],"c":"x"}'
        local t = qt.decode(src)
        assert.are.equal(src, qt.encode(t))
    end)

    it("re-emits the original JSON for an unmodified LazyArray", function()
        local src = '[10,20,{"k":"v"}]'
        local t = qt.decode(src)
        assert.are.equal(src, qt.encode(t))
    end)

    it("trims leading/trailing whitespace at the boundary", function()
        local src = '  {"a":1}  '
        local t = qt.decode(src)
        -- byte span is the value, not its outer whitespace.
        assert.are.equal('{"a":1}', qt.encode(t))
    end)
end)

describe("qd.encode — scalars", function()
    it("encodes strings with JSON escapes", function()
        assert.are.equal('"hello"', qt.encode("hello"))
        assert.are.equal('"a\\nb"', qt.encode("a\nb"))
        assert.are.equal('"a\\"b"', qt.encode('a"b'))
        assert.are.equal('"a\\\\b"', qt.encode("a\\b"))
    end)

    it("encodes booleans", function()
        assert.are.equal("true", qt.encode(true))
        assert.are.equal("false", qt.encode(false))
    end)

    it("encodes numbers", function()
        assert.are.equal("42", qt.encode(42))
        assert.are.equal("-3.14", qt.encode(-3.14))
    end)

    it("encodes qt.null as JSON null", function()
        assert.are.equal("null", qt.encode(qt.null))
    end)

    it("errors on unsupported values", function()
        assert.has_error(function() qt.encode(function() end) end)
    end)
end)

describe("qd.encode — real and mixed tables", function()
    it("encodes a real Lua object", function()
        local cjson = require("cjson")
        local s = qt.encode({a = 1, b = "x"})
        assert.are.same({a = 1, b = "x"}, cjson.decode(s))
    end)

    it("encodes a real Lua array", function()
        assert.are.equal("[1,2,3]", qt.encode({1,2,3}))
    end)

    it("encodes a hand-built empty array with empty_array_mt", function()
        local arr = setmetatable({}, qt.empty_array_mt)
        assert.are.equal("[]", qt.encode(arr))
    end)

    it("encodes mixed lazy + materialized", function()
        local t = qt.decode('{"keep":{"x":1},"changed":{"y":2}}')
        t.changed = "now a string"
        local out = qt.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.same({x=1}, parsed.keep)
        assert.are.equal("now a string", parsed.changed)
    end)
end)
