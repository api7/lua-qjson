local qjson = require("qjson")

describe("qjson encode depth guard", function()
    it("raises a circular reference error before reaching the max-depth guard", function()
        local t = {}
        t.self = t
        assert.has_error(function()
            qjson.encode(t)
        end, "qjson.encode: circular reference")
    end)

    it("allows shared table references that are not recursive cycles", function()
        local child = { x = 1 }
        local encoded = qjson.encode({
            a = child,
            b = child,
        })
        local decoded = qjson.decode(encoded)

        assert.are.equal(1, decoded.a.x)
        assert.are.equal(1, decoded.b.x)
    end)

    it("raises an error when nesting depth exceeds 1000", function()
        local root = {}
        local cur = root
        for _ = 1, 1001 do
            cur.x = {}
            cur = cur.x
        end
        assert.has_error(function()
            qjson.encode(root)
        end, "qjson.encode: max depth exceeded")
    end)

    it("succeeds when nesting depth is exactly 1000", function()
        local root = {}
        local cur = root
        for _ = 1, 999 do
            cur.x = {}
            cur = cur.x
        end
        local ok, result = pcall(qjson.encode, root)
        assert.is_true(ok, result)
        assert.is_string(result)
    end)
end)
