local qjson = require("qjson")

describe("qjson encode depth guard", function()
    it("raises an error on circular reference instead of crashing", function()
        local t = {}
        t.self = t
        assert.has_error(function()
            qjson.encode(t)
        end, "qjson.encode: max depth exceeded")
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
