local qjson = require("qjson")

describe("Lazy Patch - basic patching", function()
    it("patches a single field and encodes correctly", function()
        local t = qjson.decode('{"a":1,"b":2}')
        t.a = 10
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(10, parsed.a)
        assert.are.equal(2, parsed.b)
    end)

    it("reads patched value back", function()
        local t = qjson.decode('{"a":1,"b":2}')
        t.a = 10
        assert.are.equal(10, t.a)
        assert.are.equal(2, t.b)
    end)

    it("patches multiple fields", function()
        local t = qjson.decode('{"a":1,"b":2,"c":3}')
        t.a = 10
        t.b = 20
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(10, parsed.a)
        assert.are.equal(20, parsed.b)
        assert.are.equal(3, parsed.c)
    end)

    it("patches the same field multiple times", function()
        local t = qjson.decode('{"a":1}')
        t.a = 10
        t.a = 20
        t.a = 30
        assert.are.equal(30, t.a)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(30, parsed.a)
    end)
end)

describe("Lazy Patch - new fields", function()
    it("adds a new field", function()
        local t = qjson.decode('{"a":1}')
        t.b = 2
        assert.are.equal(1, t.a)
        assert.are.equal(2, t.b)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(1, parsed.a)
        assert.are.equal(2, parsed.b)
    end)

    it("adds multiple new fields", function()
        local t = qjson.decode('{"a":1}')
        t.b = 2
        t.c = 3
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(1, parsed.a)
        assert.are.equal(2, parsed.b)
        assert.are.equal(3, parsed.c)
    end)
end)

describe("Lazy Patch - deletion", function()
    it("deletes a field", function()
        local t = qjson.decode('{"a":1,"b":2}')
        t.a = nil
        assert.is_nil(t.a)
        assert.are.equal(2, t.b)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.is_nil(parsed.a)
        assert.are.equal(2, parsed.b)
    end)

    it("deletes then re-adds a field", function()
        local t = qjson.decode('{"a":1,"b":2}')
        t.a = nil
        assert.is_nil(t.a)
        t.a = 100
        assert.are.equal(100, t.a)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(100, parsed.a)
        assert.are.equal(2, parsed.b)
    end)
end)

describe("Lazy Patch - type changes", function()
    it("changes number to string", function()
        local t = qjson.decode('{"a":1}')
        t.a = "hello"
        assert.are.equal("hello", t.a)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal("hello", parsed.a)
    end)

    it("changes string to number", function()
        local t = qjson.decode('{"a":"hello"}')
        t.a = 42
        assert.are.equal(42, t.a)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(42, parsed.a)
    end)

    it("changes scalar to object", function()
        local t = qjson.decode('{"a":1}')
        t.a = {x = 10}
        assert.are.equal(10, t.a.x)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(10, parsed.a.x)
    end)

    it("changes scalar to array", function()
        local t = qjson.decode('{"a":1}')
        t.a = {10, 20, 30}
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.same({10, 20, 30}, parsed.a)
    end)
end)

describe("Lazy Patch - subtrees remain lazy", function()
    it("patching root does not materialize children", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        t.b = 20
        -- Child should still be lazy
        assert.are.equal(qjson._LazyObject, getmetatable(t.a))
        assert.are.equal(1, t.a.x)
    end)

    it("patching root preserves lazy array children", function()
        local t = qjson.decode('{"a":[1,2,3],"b":2}')
        t.b = 20
        -- Child should still be lazy
        assert.are.equal(qjson._LazyArray, getmetatable(t.a))
        assert.are.equal(1, t.a[1])
    end)
end)

describe("Lazy Patch - iteration with patches", function()
    it("iterates with patched values", function()
        local t = qjson.decode('{"a":1,"b":2,"c":3}')
        t.a = 10
        local keys = {}
        local values = {}
        for k, v in qjson.pairs(t) do
            keys[#keys+1] = k
            values[k] = v
        end
        assert.are.equal(10, values.a)
        assert.are.equal(2, values.b)
        assert.are.equal(3, values.c)
    end)

    it("iterates with new fields", function()
        local t = qjson.decode('{"a":1}')
        t.b = 2
        local keys = {}
        local values = {}
        for k, v in qjson.pairs(t) do
            keys[#keys+1] = k
            values[k] = v
        end
        assert.are.equal(1, values.a)
        assert.are.equal(2, values.b)
    end)

    it("iterates skipping deleted fields", function()
        local t = qjson.decode('{"a":1,"b":2,"c":3}')
        t.b = nil
        local keys = {}
        local values = {}
        for k, v in qjson.pairs(t) do
            keys[#keys+1] = k
            values[k] = v
        end
        assert.are.equal(1, values.a)
        assert.is_nil(values.b)
        assert.are.equal(3, values.c)
    end)
end)

describe("Lazy Patch - nested modifications", function()
    it("nested object modification records a patch on the child (child stays lazy)", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        t.a.x = 10
        -- Per lazy-patch-spec edge case 1: parent and child both stay lazy;
        -- the child records its own patch instead of materializing.
        assert.are.equal(qjson._LazyObject, getmetatable(t.a))
        assert.are.equal(10, t.a.x)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(10, parsed.a.x)
        assert.are.equal(2, parsed.b)
    end)

    it("nested array modification triggers child materialization", function()
        local t = qjson.decode('{"a":[1,2,3],"b":2}')
        t.a[2] = 20
        -- Child is now materialized
        assert.are.equal(qjson.empty_array_mt, getmetatable(t.a))
        assert.are.equal(20, t.a[2])
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.same({1, 20, 3}, parsed.a)
        assert.are.equal(2, parsed.b)
    end)
end)

describe("Lazy Patch - edge cases", function()
    it("handles empty object", function()
        local t = qjson.decode('{}')
        t.a = 1
        assert.are.equal(1, t.a)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(1, parsed.a)
    end)

    it("handles special characters in keys", function()
        local t = qjson.decode('{"a.b":1}')
        t["a.b"] = 10
        assert.are.equal(10, t["a.b"])
    end)

    it("handles unicode in values", function()
        local t = qjson.decode('{"a":"hello"}')
        t.a = "hello world"
        assert.are.equal("hello world", t.a)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal("hello world", parsed.a)
    end)

    it("handles boolean values", function()
        local t = qjson.decode('{"a":true}')
        t.a = false
        assert.is_false(t.a)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.is_false(parsed.a)
    end)

    it("handles null values", function()
        local t = qjson.decode('{"a":1}')
        t.a = qjson.null
        assert.are.equal(qjson.null, t.a)
        local out = qjson.encode(t)
        local cjson = require("cjson")
        local parsed = cjson.decode(out)
        assert.are.equal(cjson.null, parsed.a)
    end)
end)

describe("Lazy Patch - metatable preservation", function()
    it("LazyObject metatable is preserved after patching", function()
        local t = qjson.decode('{"a":1,"b":2}')
        t.a = 10
        assert.are.equal(qjson._LazyObject, getmetatable(t))
    end)

    it("can still access original fields after patching", function()
        local t = qjson.decode('{"a":1,"b":2,"c":3}')
        t.a = 10
        assert.are.equal(10, t.a)
        assert.are.equal(2, t.b)
        assert.are.equal(3, t.c)
    end)
end)
