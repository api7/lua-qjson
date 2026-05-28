local qjson = require("qjson")

describe("ordered encode", function()
    it("preserves key order on value modification", function()
        local t = qjson.decode('{"c":3,"a":1,"b":2}')
        t.a = 100
        assert.are.equal('{"c":3,"a":100,"b":2}', qjson.encode(t))
    end)

    it("preserves order when deleting a key", function()
        local t = qjson.decode('{"c":3,"a":1,"b":2}')
        t.a = nil
        assert.are.equal('{"c":3,"b":2}', qjson.encode(t))
    end)

    it("appends new keys to the end", function()
        local t = qjson.decode('{"c":3,"a":1}')
        t.b = 2
        assert.are.equal('{"c":3,"a":1,"b":2}', qjson.encode(t))
    end)

    it("deleted then re-added key appears at end", function()
        local t = qjson.decode('{"a":1,"b":2,"c":3}')
        t.b = nil
        t.b = 999
        assert.are.equal('{"a":1,"c":3,"b":999}', qjson.encode(t))
    end)

    it("handles nested object modification", function()
        local t = qjson.decode('{"x":1,"nested":{"a":1,"b":2},"y":2}')
        t.nested.a = 100
        local out = qjson.encode(t)
        assert.truthy(out:find('"x":1'))
        assert.truthy(out:find('"y":2'))
        assert.truthy(out:find('"a":100'))
    end)

    it("handles empty object with additions", function()
        local t = qjson.decode('{}')
        t.a = 1
        t.b = 2
        assert.are.equal('{"a":1,"b":2}', qjson.encode(t))
    end)

    it("handles delete all keys", function()
        local t = qjson.decode('{"a":1,"b":2}')
        t.a = nil
        t.b = nil
        assert.are.equal('{}', qjson.encode(t))
    end)

    it("read before modify works correctly", function()
        local t = qjson.decode('{"a":1,"b":2,"c":3}')
        local _ = t.b  -- read first
        t.b = 999
        assert.are.equal('{"a":1,"b":999,"c":3}', qjson.encode(t))
    end)

    it("pairs iterates in original order after modification", function()
        local t = qjson.decode('{"c":3,"a":1,"b":2}')
        t.a = 100
        local order = {}
        for k, _ in qjson.pairs(t) do
            order[#order + 1] = k
        end
        assert.are.same({"c", "a", "b"}, order)
    end)

    it("multiple modifications preserve order", function()
        local t = qjson.decode('{"a":1,"b":2,"c":3,"d":4}')
        t.b = 20
        t.d = 40
        t.a = 10
        assert.are.equal('{"a":10,"b":20,"c":3,"d":40}', qjson.encode(t))
    end)

    it("materialize preserves modified values", function()
        local t = qjson.decode('{"c":3,"a":1,"b":2}')
        t.a = 100
        local m = qjson.materialize(t)
        assert.are.equal(100, m.a)
        assert.are.equal(2, m.b)
        assert.are.equal(3, m.c)
    end)

    it("cached nested object mutations are preserved in encode", function()
        local t = qjson.decode('{"nested":{"x":1,"y":2}}')
        local nested = t.nested  -- cache the nested object
        t.extra = "added"        -- trigger parent materialization
        nested.x = 100           -- modify cached nested
        local out = qjson.encode(t)
        assert.truthy(out:find('"x":100'))
        assert.truthy(out:find('"extra":"added"'))
    end)

    it("replaces a cached child container after parent materialization", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        local old = t.a
        assert.are.equal(1, old.x)
        t.c = 3
        t.a = "replaced"
        assert.are.equal('{"a":"replaced","b":2,"c":3}', qjson.encode(t))
    end)

    it("deletes a cached child container after parent materialization", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        local old = t.a
        assert.are.equal(1, old.x)
        t.c = 3
        t.a = nil
        assert.are.equal('{"b":2,"c":3}', qjson.encode(t))
    end)

    it("replaces a cached child container as the first parent write", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        local old = t.a
        assert.are.equal(1, old.x)
        t.a = "replaced"
        assert.are.equal('{"a":"replaced","b":2}', qjson.encode(t))
    end)

    it("deletes a cached child container as the first parent write", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        local old = t.a
        assert.are.equal(1, old.x)
        t.a = nil
        assert.are.equal('{"b":2}', qjson.encode(t))
    end)

    it("handles _keys as user JSON field without collision", function()
        local t = qjson.decode('{"_keys":["user"],"a":1}')
        local user_keys = t._keys
        assert.are.equal("table", type(user_keys))
        t.b = 2
        assert.are.equal(1, t.a)
        assert.are.equal(2, t.b)
        local out = qjson.encode(t)
        assert.truthy(out:find('"_keys"'))
        assert.truthy(out:find('"a":1'))
        assert.truthy(out:find('"b":2'))
    end)

    it("handles _values as user JSON field without collision", function()
        local t = qjson.decode('{"_values":{"z":9},"a":1}')
        local user_values = t._values
        assert.are.equal(9, user_values.z)
        assert.are.equal(1, t.a)
        t.b = 2
        local out = qjson.encode(t)
        assert.truthy(out:find('"_values"'))
        assert.truthy(out:find('"a":1'))
        assert.truthy(out:find('"b":2'))
    end)

    it("pairs sees cached child mutations on unmodified parent", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        local a = t.a
        a.x = 99
        local seen = {}
        for k, v in qjson.pairs(t) do
            if k == "a" then seen.a_x = v.x end
            if k == "b" then seen.b = v end
        end
        assert.are.equal(99, seen.a_x)
        assert.are.equal(2, seen.b)
    end)

    it("rejects non-string key write with a clear error", function()
        local t = qjson.decode('{"a":1}')
        assert.has_error(function() t[1] = "x" end, "qjson: object key must be a string, got number")
        -- object must remain consistent after the failed write
        assert.are.equal(1, t.a)
    end)
end)
