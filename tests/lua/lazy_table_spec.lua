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
