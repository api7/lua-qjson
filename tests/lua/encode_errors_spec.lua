local qjson = require("qjson")

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
    end)

    it("rejects non-string object keys", function()
        assert_encode_error({name = "value", [2] = "two"}, "qjson.encode: object key must be a string, got number")
        assert_encode_error({[true] = 1}, "qjson.encode: object key must be a string, got boolean")
        assert_encode_error({[{}] = 1}, "qjson.encode: object key must be a string, got table")
    end)

    it("rejects sparse arrays as objects with numeric keys", function()
        assert_encode_error({[1] = "first", [5] = "fifth"}, "qjson.encode: object key must be a string, got number")
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
