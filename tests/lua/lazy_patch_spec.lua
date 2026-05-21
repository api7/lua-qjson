local qjson = require("qjson")
local cjson = require("cjson")

local function collect_pairs(view)
    local out = {}
    local order = {}
    for k, v in qjson.pairs(view) do
        out[k] = v
        order[#order + 1] = k
    end
    return out, order
end

describe("LazyObject scalar patches on existing keys", function()
    it("replaces a string with a string", function()
        local v = qjson.decode('{"a":"x","b":"y"}')
        v.a = "z"
        assert.are.equal("z", v.a)
        assert.are.equal("y", v.b)
    end)

    it("replaces a number with a number", function()
        local v = qjson.decode('{"a":1,"b":2}')
        v.a = 999
        assert.are.equal(999, v.a)
        assert.are.equal(2, v.b)
    end)

    it("replaces a boolean with a boolean", function()
        local v = qjson.decode('{"a":true,"b":false}')
        v.a = false
        v.b = true
        assert.is_false(v.a)
        assert.is_true(v.b)
    end)

    it("replaces a value with qjson.null", function()
        local v = qjson.decode('{"a":1}')
        v.a = qjson.null
        assert.are.equal(qjson.null, v.a)
    end)

    it("replaces a scalar with a different scalar type (number -> string)", function()
        local v = qjson.decode('{"a":1}')
        v.a = "hello"
        assert.are.equal("hello", v.a)
        local out = qjson.encode(v)
        assert.are.equal("hello", cjson.decode(out).a)
    end)

    it("replaces a container with a scalar and drops cached child proxy", function()
        local v = qjson.decode('{"a":{"x":1},"b":2}')
        local child = v.a
        assert.is_table(child)
        v.a = "now-scalar"
        -- Subsequent read returns the patched scalar, not the cached proxy.
        assert.are.equal("now-scalar", v.a)
        -- pairs() also surfaces the scalar, in original key order.
        local map, order = collect_pairs(v)
        assert.are.equal("now-scalar", map.a)
        assert.are.equal(2, map.b)
        assert.are.equal("a", order[1])
        assert.are.equal("b", order[2])
        -- encode also reflects the replacement (via walking encoder, since
        -- the raw-slot bypass is not a patch entry).
        local parsed = cjson.decode(qjson.encode(v))
        assert.are.equal("now-scalar", parsed.a)
        assert.are.equal(2, parsed.b)
    end)

    it("repeated writes to the same key keep only one patch entry", function()
        local v = qjson.decode('{"a":1,"b":2}')
        v.a = "x"
        v.a = "y"
        local out = qjson.encode(v)
        -- Only one occurrence of "y", no "x" leftover, b unchanged.
        assert.is_truthy(out:find('"y"', 1, true))
        assert.is_nil(out:find('"x"', 1, true))
        -- Sanity: parse back and verify.
        local parsed = cjson.decode(out)
        assert.are.equal("y", parsed.a)
        assert.are.equal(2, parsed.b)
    end)

    it("read after write returns patched value", function()
        local v = qjson.decode('{"a":1}')
        v.a = 42
        assert.are.equal(42, v.a)
    end)

    it("pairs() yields patched value in original key order", function()
        local v = qjson.decode('{"a":1,"b":2,"c":3}')
        v.b = 999
        local seen = {}
        for k, val in qjson.pairs(v) do
            seen[#seen + 1] = {k, val}
        end
        assert.are.equal("a", seen[1][1]); assert.are.equal(1, seen[1][2])
        assert.are.equal("b", seen[2][1]); assert.are.equal(999, seen[2][2])
        assert.are.equal("c", seen[3][1]); assert.are.equal(3, seen[3][2])
    end)

    it("qjson.pairs yields patched value", function()
        local v = qjson.decode('{"a":1}')
        v.a = "patched"
        local map = {}
        for k, val in qjson.pairs(v) do map[k] = val end
        assert.are.equal("patched", map.a)
    end)

    it("#view is unchanged after scalar patch", function()
        local v = qjson.decode('{"a":1,"b":2,"c":3}')
        local before = qjson.len(v)
        v.a = 999
        v.b = "x"
        assert.are.equal(before, qjson.len(v))
    end)

    it("encode produces JSON with patched values spliced in", function()
        local v = qjson.decode('{"a":1,"b":"x","c":true}')
        v.a = 999
        v.b = "y"
        v.c = qjson.null
        assert.are.equal('{"a":999,"b":"y","c":null}', qjson.encode(v))
    end)

    it("encode round-trip matches expected logical value", function()
        local v = qjson.decode('{"a":1,"b":[1,2,3],"c":{"x":true}}')
        v.a = 42
        local out = qjson.encode(v)
        local parsed = cjson.decode(out)
        assert.are.equal(42, parsed.a)
        assert.are.same({1, 2, 3}, parsed.b)
        assert.is_true(parsed.c.x)
    end)

    it("encode preserves whitespace outside patched ranges", function()
        local v = qjson.decode('{ "a" : 1 , "b" : 2 }')
        v.a = 99
        local out = qjson.encode(v)
        -- The encoder splices only the value bytes; original spaces remain.
        assert.are.equal('{ "a" : 99 , "b" : 2 }', out)
    end)

    it("multiple patches on different keys are all applied", function()
        local v = qjson.decode('{"a":1,"b":2,"c":3,"d":4}')
        v.a = 10
        v.c = 30
        v.d = "z"
        local out = qjson.encode(v)
        local parsed = cjson.decode(out)
        assert.are.equal(10, parsed.a)
        assert.are.equal(2, parsed.b)
        assert.are.equal(30, parsed.c)
        assert.are.equal("z", parsed.d)
    end)

    it("tostring(view) reflects patches", function()
        local v = qjson.decode('{"a":1,"b":2}')
        v.a = 99
        local s = tostring(v)
        local parsed = cjson.decode(s)
        assert.are.equal(99, parsed.a)
        assert.are.equal(2, parsed.b)
    end)

    it("qjson.materialize reflects patches", function()
        local v = qjson.decode('{"a":1,"b":{"x":2}}')
        v.a = 999
        local m = qjson.materialize(v)
        assert.are.equal(999, m.a)
        assert.are.equal(2, m.b.x)
    end)
end)

describe("LazyObject fall through to materialization", function()
    it("nil value triggers materialization", function()
        local v = qjson.decode('{"a":1,"b":2}')
        v.a = nil
        -- View is now a plain table: no lazy metatable.
        assert.are_not.equal(qjson._LazyObject, getmetatable(v))
        assert.is_nil(v.a)
        assert.are.equal(2, v.b)
    end)

    it("table value triggers materialization", function()
        local v = qjson.decode('{"a":1}')
        v.a = {x = 10}
        assert.are_not.equal(qjson._LazyObject, getmetatable(v))
        assert.are.equal(10, v.a.x)
    end)

    it("new (absent) key triggers materialization", function()
        local v = qjson.decode('{"a":1}')
        v.new_key = "fresh"
        assert.are_not.equal(qjson._LazyObject, getmetatable(v))
        assert.are.equal(1, v.a)
        assert.are.equal("fresh", v.new_key)
    end)

    it("prior patches are preserved when fall through occurs", function()
        local v = qjson.decode('{"a":1,"b":2}')
        v.a = "p"
        -- Now fall through with a nil delete.
        v.b = nil
        assert.are_not.equal(qjson._LazyObject, getmetatable(v))
        assert.are.equal("p", v.a)
        assert.is_nil(v.b)
    end)

    it("prior patches preserved across fall-through-on-table-write", function()
        local v = qjson.decode('{"a":1,"b":2}')
        v.a = 99
        v.b = {nested = true}
        assert.are_not.equal(qjson._LazyObject, getmetatable(v))
        assert.are.equal(99, v.a)
        assert.is_true(v.b.nested)
    end)
end)

describe("Mixed patches and dirty children", function()
    it("dirty child plus a patch on a different key are both applied in encode", function()
        local v = qjson.decode('{"nested":{"x":1},"other":"orig"}')
        -- Read and mutate the nested child to make t dirty.
        v.nested.x = 99
        -- Patch a sibling scalar.
        v.other = "patched"
        local out = qjson.encode(v)
        local parsed = cjson.decode(out)
        assert.are.equal(99, parsed.nested.x)
        assert.are.equal("patched", parsed.other)
    end)

    it("walking encoder respects patches when invoked via dirty descendant", function()
        local v = qjson.decode('{"a":{"b":1},"c":2}')
        -- Make a deeper subtree dirty to force walking at the top.
        v.a.b = 99
        v.c = "patched"
        local out = qjson.encode(v)
        local parsed = cjson.decode(out)
        assert.are.equal(99, parsed.a.b)
        assert.are.equal("patched", parsed.c)
    end)
end)

describe("LazyArray unchanged by patch feature", function()
    it("array __newindex still materializes on first scalar write", function()
        local v = qjson.decode('[10, 20, 30]')
        v[1] = 99
        -- After write, no longer a LazyArray (materialized into empty_array_mt).
        assert.are_not.equal(qjson._LazyArray, getmetatable(v))
        assert.are.equal(99, v[1])
        assert.are.equal(20, v[2])
        assert.are.equal(30, v[3])
    end)
end)
