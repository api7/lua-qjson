local qjson = require "qjson"

describe("parse with options", function()
    it("accepts no second arg (default eager)", function()
        assert.is_not_nil(qjson.parse('{"a":1}'))
    end)

    it("accepts an empty opts table", function()
        assert.is_not_nil(qjson.parse('{"a":1}', {}))
    end)

    it("accepts lazy=true and tolerates trailing content", function()
        -- Trailing content is eager-only; lazy must parse OK.
        assert.is_not_nil(qjson.parse('{}garbage', { lazy = true }))
    end)

    it("accepts max_depth", function()
        assert.is_not_nil(qjson.parse('[[[1]]]', { max_depth = 1024 }))
    end)

    it("rejects invalid mode key value", function()
        assert.has_error(function()
            qjson.parse('{}', { lazy = "yes please" })
        end)
    end)

    it("accepts lazy=true and max_depth combined", function()
        assert.is_not_nil(qjson.parse('[[1]]', { lazy = true, max_depth = 256 }))
    end)

    it("rejects fractional max_depth", function()
        assert.has_error(function()
            qjson.parse('{}', { max_depth = 1.5 })
        end)
    end)
end)
