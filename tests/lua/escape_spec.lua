local qjson = require("qjson")

describe("qjson strings", function()
    it("decodes simple escape", function()
        local d = qjson.parse('{"a":"he\\nlo"}')
        assert.are.equal("he\nlo", d:get_str("a"))
    end)

    it("decodes unicode escape", function()
        local d = qjson.parse('{"a":"\\u00e9"}')
        assert.are.equal("\xc3\xa9", d:get_str("a"))
    end)

    it("decodes surrogate pair", function()
        local d = qjson.parse('{"a":"\\uD83D\\uDE00"}')
        assert.are.equal("\xF0\x9F\x98\x80", d:get_str("a"))
    end)

    it("zero-copy for unescaped strings", function()
        local d = qjson.parse('{"a":"plain"}')
        assert.are.equal("plain", d:get_str("a"))
    end)
end)
