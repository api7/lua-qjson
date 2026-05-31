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
        local d = qjson.parse('{"user":{"age":1}}')
        assert.has_error(
            function() d:get_str("user.age") end,
            "qjson: type mismatch: expected string, got number at byte 15"
        )
    end)

    it("does not invent an expected null type when no expected type is provided", function()
        local d = qjson.parse('{"a":1}')
        local a = d:open("a")
        local ok, err = pcall(function() a:open("x") end)
        assert.is_false(ok)
        assert.is_truthy(string.find(tostring(err), "qjson: type mismatch", 1, true), tostring(err))
        assert.is_falsy(string.find(tostring(err), "expected null", 1, true), tostring(err))
    end)

    it("parse errors include byte offsets", function()
        local cases = {
            { json = "[}",                fragment = "parse error at byte 1: unexpected '}', expected value" },
            { json = '{"a":1,,',          fragment = "parse error at byte 7: unexpected ',', expected value" },
            { json = "[01]",              fragment = "invalid number '01' at byte 1" },
            { json = "{\"a\":\"\255\"}", fragment = "invalid UTF-8 in string at byte 5" },
            { json = "{}garbage",         fragment = "trailing content 'garbage' after root value at byte 2" },
            { json = string.rep("[", 1025), fragment = "nesting too deep at byte 1024 (max 1024)" },
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

    it("lazy string decode errors report the string content offset", function()
        local d = qjson.parse('{"s":"\\001"}', { lazy = true })
        assert.has_error(
            function() d:get_str("s") end,
            "qjson: invalid string content at byte 6"
        )
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
        assert.has_error(function() d:get_i64("u") end, "qjson: out of range at byte 5")
        assert.has_error(function() d:get_u64("neg") end, "qjson: out of range at byte 32")
        for _, path in ipairs({"f", "b", "s", "n"}) do
            local ok_i64, err_i64 = pcall(function() d:get_i64(path) end)
            local ok_u64, err_u64 = pcall(function() d:get_u64(path) end)
            assert.is_false(ok_i64)
            assert.is_false(ok_u64)
            assert.is_truthy(string.find(tostring(err_i64), "expected number", 1, true), tostring(err_i64))
            assert.is_truthy(string.find(tostring(err_u64), "expected number", 1, true), tostring(err_u64))
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

    it("len type mismatch reports an array/object expectation", function()
        local d = qjson.parse('{"n":1}')
        assert.has_error(
            function() d:len("n") end,
            "qjson: type mismatch: expected array/object, got number at byte 5"
        )
    end)
end)
