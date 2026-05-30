local qjson = require("qjson")

describe("qjson basic", function()
    it("parses an object and gets a string field", function()
        local d = qjson.parse('{"a":"hello"}')
        assert.are.equal("hello", d:get_str("a"))
    end)

    it("returns nil on missing path", function()
        local d = qjson.parse('{"a":1}')
        assert.is_nil(d:get_str("b"))
    end)

    it("errors on type mismatch", function()
        local d = qjson.parse('{"a":1}')
        assert.has_error(function() d:get_str("a") end)
    end)

    it("parse errors include byte offsets", function()
        local cases = {
            { json = "{",                 fragment = "JSON parse error at byte 1" },
            { json = "[}",                fragment = "JSON parse error at byte 1" },
            { json = "[01]",              fragment = "invalid number format (RFC 8259) at byte 1" },
            { json = "{\"a\":\"\255\"}", fragment = "invalid UTF-8 in string at byte 5" },
            { json = "{}garbage",         fragment = "trailing content after root value at byte 2" },
        }

        for _, case in ipairs(cases) do
            local ok, err = pcall(qjson.parse, case.json)
            assert.is_false(ok)
            assert.is_truthy(
                string.find(tostring(err), case.fragment, 1, true),
                tostring(err)
            )
        end
    end)

    it("supports nested paths", function()
        local d = qjson.parse('{"body":{"model":"gpt"}}')
        assert.are.equal("gpt", d:get_str("body.model"))
    end)

    it("supports array indexing", function()
        local d = qjson.parse('{"xs":[10,20,30]}')
        assert.are.equal(20, d:get_i64("xs[1]"))
    end)

    it("cursor reuses shared prefix", function()
        local d = qjson.parse('{"body":{"a":1,"b":"two"}}')
        local b = d:open("body")
        assert.are.equal(1, b:get_i64("a"))
        assert.are.equal("two", b:get_str("b"))
    end)

    it("typeof reports correct types", function()
        local d = qjson.parse('{"s":"x","n":1,"f":1.5,"b":true,"z":null,"a":[],"o":{}}')
        assert.are.equal(qjson.T_STR,  d:typeof("s"))
        assert.are.equal(qjson.T_NUM,  d:typeof("n"))
        assert.are.equal(qjson.T_NUM,  d:typeof("f"))
        assert.are.equal(qjson.T_BOOL, d:typeof("b"))
        assert.are.equal(qjson.T_NULL, d:typeof("z"))
        assert.are.equal(qjson.T_ARR,  d:typeof("a"))
        assert.are.equal(qjson.T_OBJ,  d:typeof("o"))
    end)

    it("len for objects and arrays", function()
        local d = qjson.parse('{"o":{"a":1,"b":2,"c":3},"a":[1,2,3,4]}')
        assert.are.equal(3, d:len("o"))
        assert.are.equal(4, d:len("a"))
    end)
end)
