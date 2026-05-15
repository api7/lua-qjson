local qd    = require("quickdecode")
local cjson = require("cjson")

describe("quickdecode vs lua-cjson", function()
    it("agrees on simple string field", function()
        local s = '{"a":"x"}'
        assert.are.equal(cjson.decode(s).a, qd.parse(s):get_str("a"))
    end)

    it("agrees on integer field", function()
        local s = '{"a":42}'
        assert.are.equal(cjson.decode(s).a, qd.parse(s):get_i64("a"))
    end)

    it("agrees on float field", function()
        local s = '{"a":1.5}'
        assert.are.equal(cjson.decode(s).a, qd.parse(s):get_f64("a"))
    end)

    it("agrees on bool field", function()
        local s = '{"a":true}'
        assert.are.equal(cjson.decode(s).a, qd.parse(s):get_bool("a"))
    end)

    it("agrees on nested path", function()
        local s = '{"body":{"model":"gpt"}}'
        assert.are.equal(cjson.decode(s).body.model, qd.parse(s):get_str("body.model"))
    end)
end)
