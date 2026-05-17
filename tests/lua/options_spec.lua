local qd = require "quickdecode"

describe("parse with options", function()
    it("accepts no second arg (default eager)", function()
        assert.is_not_nil(qd.parse('{"a":1}'))
    end)

    it("accepts an empty opts table", function()
        assert.is_not_nil(qd.parse('{"a":1}', {}))
    end)

    it("accepts lazy=true and tolerates trailing content", function()
        -- Trailing content is eager-only; lazy must parse OK.
        assert.is_not_nil(qd.parse('{}garbage', { lazy = true }))
    end)

    it("accepts max_depth", function()
        assert.is_not_nil(qd.parse('[[[1]]]', { max_depth = 1024 }))
    end)

    it("rejects invalid mode key value", function()
        assert.has_error(function()
            qd.parse('{}', { lazy = "yes please" })
        end)
    end)
end)
