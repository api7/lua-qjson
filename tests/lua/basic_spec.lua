local qjson = require("qjson")
local ffi = require("ffi")

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

    it("returns lossless int64 cdata for integers above 2^53", function()
        local d = qjson.parse('{"id":9007199254740993}')
        local id = d:get_i64("id")
        assert.are.equal("cdata", type(id))
        assert.is_true(ffi.istype("int64_t", id))
        assert.are.equal("9007199254740993LL", tostring(id))
        assert.are.equal("number", type(d:get_f64("id")))
    end)

    it("returns lossless uint64 cdata up to uint64 max", function()
        local d = qjson.parse('{"id":18446744073709551615}')
        local id = d:get_u64("id")
        assert.are.equal("cdata", type(id))
        assert.is_true(ffi.istype("uint64_t", id))
        assert.are.equal("18446744073709551615ULL", tostring(id))
    end)

    it("cursor reuses shared prefix", function()
        local d = qjson.parse('{"body":{"a":1,"b":"two"}}')
        local b = d:open("body")
        assert.are.equal(1, b:get_i64("a"))
        assert.are.equal("two", b:get_str("b"))
    end)

    it("cursor returns lossless int64 and uint64 cdata", function()
        local d = qjson.parse('{"body":{"i":9007199254740993,"u":18446744073709551615}}')
        local body = d:open("body")
        local i = body:get_i64("i")
        local u = body:get_u64("u")
        assert.is_true(ffi.istype("int64_t", i))
        assert.is_true(ffi.istype("uint64_t", u))
        assert.are.equal("9007199254740993LL", tostring(i))
        assert.are.equal("18446744073709551615ULL", tostring(u))
    end)

    it("reports integer range and type errors consistently", function()
        local d = qjson.parse('{"u":18446744073709551615,"neg":-1,"f":1.5,"b":true,"s":"1","n":null}')
        assert.has_error(function() d:get_i64("u") end, "qjson: numeric out of range")
        assert.has_error(function() d:get_u64("neg") end, "qjson: numeric out of range")
        for _, path in ipairs({"f", "b", "s", "n"}) do
            assert.has_error(function() d:get_i64(path) end, "qjson: type mismatch at path")
            assert.has_error(function() d:get_u64(path) end, "qjson: type mismatch at path")
        end
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
