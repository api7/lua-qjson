local qjson = require("qjson")
local ffi = require("ffi")

local function assert_encode_error(value, message)
    assert.has_error(function()
        qjson.encode(value)
    end, message)
end

describe("qjson.encode error coverage", function()
    it("rejects non-finite numbers", function()
        assert_encode_error(math.huge, "qjson.encode: cannot encode non-finite number")
        assert_encode_error(-math.huge, "qjson.encode: cannot encode non-finite number")
        assert_encode_error(0 / 0, "qjson.encode: cannot encode non-finite number")
    end)

    it("rejects unsupported Lua value types", function()
        assert_encode_error(function() end, "qjson.encode: unsupported value type: function")
        assert_encode_error(coroutine.create(function() end), "qjson.encode: unsupported value type: thread")
        assert_encode_error(newproxy(false), "qjson.encode: unsupported value type: userdata")
        assert_encode_error(ffi.new("double", 1.25), "qjson.encode: unsupported value type: cdata")
        assert_encode_error(ffi.new("struct { int x; }"), "qjson.encode: unsupported value type: cdata")
        assert_encode_error(ffi.cast("void *", 1), "qjson.encode: unsupported value type: cdata")
    end)

    it("encodes int64 and uint64 cdata as decimal JSON integers", function()
        assert.are.equal("9007199254740993", qjson.encode(9007199254740993LL))
        assert.are.equal("18446744073709551615", qjson.encode(18446744073709551615ULL))
        assert.are.equal(1, select("#", qjson.encode(9007199254740993LL)))
        assert.are.equal(1, select("#", qjson.encode(18446744073709551615ULL)))
        assert.are.equal('{"i":9007199254740993}', qjson.encode({ i = 9007199254740993LL }))
        assert.are.equal('{"u":18446744073709551615}', qjson.encode({ u = 18446744073709551615ULL }))
    end)

    it("encodes 64-bit cdata boundary values without precision loss", function()
        assert.are.equal("9223372036854775807", qjson.encode(9223372036854775807LL))
        assert.are.equal("-9223372036854775808", qjson.encode(ffi.new("int64_t", -9223372036854775807LL - 1LL)))
        assert.are.equal("18446744073709551615", qjson.encode(18446744073709551615ULL))
        assert.are.equal("0", qjson.encode(0LL))
        assert.are.equal("0", qjson.encode(0ULL))
    end)

    it("encodes nested int64 and uint64 cdata values", function()
        assert.are.equal("[1,2]", qjson.encode({ 1LL, 2LL }))
        assert.are.equal('[{"x":9007199254740993},{"y":18446744073709551615}]', qjson.encode({
            { x = 9007199254740993LL },
            { y = 18446744073709551615ULL },
        }))
    end)

    it("round-trips decoded 64-bit integer cdata through encode", function()
        local doc = qjson.parse('{"i":9007199254740993,"u":18446744073709551615}')

        assert.are.equal('{"i":9007199254740993}', qjson.encode({ i = doc:get_i64("i") }))
        assert.are.equal('{"u":18446744073709551615}', qjson.encode({ u = doc:get_u64("u") }))
    end)

    it("intentionally differs from lua-cjson by accepting int64 and uint64 cdata", function()
        local cjson = require("cjson")

        assert.has_error(function()
            cjson.encode(9007199254740993LL)
        end)
        assert.has_error(function()
            cjson.encode(18446744073709551615ULL)
        end)
        assert.are.equal("9007199254740993", qjson.encode(9007199254740993LL))
        assert.are.equal("18446744073709551615", qjson.encode(18446744073709551615ULL))
    end)

    it("stringifies numeric object keys and rejects unsupported key types", function()
        assert.are.equal('{"0":"zero"}', qjson.encode({[0] = "zero"}))
        assert_encode_error({[true] = 1}, "qjson.encode: object key must be a string or number, got boolean")
        assert_encode_error({[{}] = 1}, "qjson.encode: object key must be a string or number, got table")
    end)

    it("encodes sparse arrays within the lua-cjson default sparse limit", function()
        assert.are.equal('["first",null,null,null,"fifth"]', qjson.encode({[1] = "first", [5] = "fifth"}))
    end)

    it("keeps empty table object and empty array encodings distinct", function()
        local empty_array = setmetatable({}, qjson.empty_array_mt)

        assert.are.equal("{}", qjson.encode({}))
        assert.are.equal("[]", qjson.encode(empty_array))

        local encoded = qjson.encode({
            empty_object = {},
            empty_array = empty_array,
        })
        local doc = qjson.parse(encoded)
        assert.are.equal(qjson.T_OBJ, doc:typeof("empty_object"))
        assert.are.equal(qjson.T_ARR, doc:typeof("empty_array"))
    end)

    it("encodes valid nested structures without false positives", function()
        local value = {
            ok = true,
            count = 3,
            name = "nested",
            items = {1, 2, 3},
            child = {
                ratio = 1.25,
                empty_array = setmetatable({}, qjson.empty_array_mt),
            },
        }

        local encoded = qjson.encode(value)
        local decoded = qjson.decode(encoded)

        assert.is_true(decoded.ok)
        assert.are.equal(3, decoded.count)
        assert.are.equal("nested", decoded.name)
        assert.are.equal(2, decoded.items[2])
        assert.are.equal(1.25, decoded.child.ratio)
        assert.are.equal(0, qjson.len(decoded.child.empty_array))
    end)
end)
