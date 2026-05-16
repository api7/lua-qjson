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
