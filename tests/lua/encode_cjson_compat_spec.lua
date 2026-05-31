local qjson = require("qjson")
local cjson = require("cjson")
local ffi = require("ffi")

local function deep_equal(a, b)
    if a == b then
        return true
    end
    if type(a) ~= type(b) then
        return false
    end
    if type(a) ~= "table" then
        return false
    end
    for k, v in pairs(a) do
        if not deep_equal(v, b[k]) then
            return false
        end
    end
    for k in pairs(b) do
        if a[k] == nil then
            return false
        end
    end
    return true
end

local function assert_json_equal(actual, expected)
    assert.is_true(
        deep_equal(cjson.decode(actual), cjson.decode(expected)),
        "expected " .. actual .. " to equal " .. expected
    )
end

local function assert_encodes_like_cjson(value)
    assert_json_equal(qjson.encode(value), cjson.encode(value))
end

local function assert_encode_error(value, message)
    local ok, err = pcall(qjson.encode, value)
    assert.is_false(ok)
    if message then
        assert.matches(message, tostring(err), 1, true)
    end
end

local function assert_cjson_does_not_preserve_proxy(proxy, expected_json)
    local ok, encoded = pcall(cjson.encode, proxy)
    if ok then
        assert.is_false(deep_equal(cjson.decode(encoded), cjson.decode(expected_json)))
    else
        assert.is_string(encoded)
    end
end

describe("qjson.encode lua-cjson compatible Lua inputs", function()
    it("encodes nil as JSON null", function()
        assert.are.equal("null", qjson.encode(nil))
        assert_encodes_like_cjson(nil)
    end)

    it("encodes positive integer keyed tables as arrays with null holes", function()
        assert.are.equal('["a",null,null,null,"e"]', qjson.encode({[1] = "a", [5] = "e"}))
        assert.are.equal("[1,null,3]", qjson.encode({[1] = 1, [3] = 3}))
        assert.are.equal('[null,"b"]', qjson.encode({[2] = "b"}))

        assert_encodes_like_cjson({[1] = "a", [5] = "e"})
        assert_encodes_like_cjson({[1] = 1, [3] = 3})
        assert_encodes_like_cjson({[2] = "b"})
    end)

    it("rejects excessively sparse arrays", function()
        assert_encode_error({[1] = 1, [1000] = 2}, "excessively sparse array")
    end)

    it("encodes mixed or non-positive numeric keys as object string keys", function()
        assert_json_equal(qjson.encode({[1] = 1, a = 2}), '{"1":1,"a":2}')
        assert_json_equal(qjson.encode({[1] = "a", [2] = "b", x = "c"}), '{"1":"a","2":"b","x":"c"}')
        assert_json_equal(qjson.encode({name = "value", [2] = "two"}), '{"2":"two","name":"value"}')
        assert_json_equal(qjson.encode({[0] = "zero"}), '{"0":"zero"}')
        assert_json_equal(qjson.encode({[-1] = "neg"}), '{"-1":"neg"}')
        assert_json_equal(qjson.encode({[1.5] = "half"}), '{"1.5":"half"}')

        assert_encodes_like_cjson({[1] = 1, a = 2})
        assert_encodes_like_cjson({[1] = "a", [2] = "b", x = "c"})
        assert_encodes_like_cjson({name = "value", [2] = "two"})
        assert_encodes_like_cjson({[0] = "zero"})
        assert_encodes_like_cjson({[-1] = "neg"})
        assert_encodes_like_cjson({[1.5] = "half"})
    end)

    it("keeps empty tables as objects unless empty_array_mt is used", function()
        assert.are.equal("{}", qjson.encode({}))
        assert.are.equal("[]", qjson.encode(setmetatable({}, qjson.empty_array_mt)))
    end)

    it("keeps qjson-specific circular reference wording for cycles", function()
        local obj = {}
        obj.self = obj
        assert.has_error(function()
            qjson.encode(obj)
        end, "qjson.encode: circular reference")

        local arr = {}
        arr[1] = arr
        assert.has_error(function()
            qjson.encode(arr)
        end, "qjson.encode: circular reference")

        assert.has_error(function()
            cjson.encode(obj)
        end)
        assert.has_error(function()
            cjson.encode(arr)
        end)
    end)
end)

describe("qjson.encode qjson lazy proxy extensions", function()
    it("encodes 64-bit integer cdata as a qjson extension", function()
        assert.are.equal("9007199254740993", qjson.encode(9007199254740993LL))
        assert.are.equal("18446744073709551615", qjson.encode(18446744073709551615ULL))
        assert.are.equal('{"i":9007199254740993}', qjson.encode({i = ffi.new("int64_t", 9007199254740993LL)}))

        assert.has_error(function()
            cjson.encode(9007199254740993LL)
        end)
        assert.has_error(function()
            cjson.encode(18446744073709551615ULL)
        end)
    end)

    it("passes clean lazy objects through as original JSON bytes", function()
        local src = '{"a":1,"big":900719925474099312345}'
        local lazy = qjson.decode(src)

        assert.are.equal(src, qjson.encode(lazy))

        assert_cjson_does_not_preserve_proxy(lazy, src)
    end)

    it("encodes dirty lazy objects in first-seen key order", function()
        local lazy = qjson.decode('{"a":1,"b":2}')
        lazy.a = 10
        lazy.c = 3

        assert.are.equal('{"a":10,"b":2,"c":3}', qjson.encode(lazy))

        assert_cjson_does_not_preserve_proxy(lazy, '{"a":10,"b":2,"c":3}')
    end)
end)

describe("qjson.encode shared unsupported inputs", function()
    it("rejects unsupported top-level value types", function()
        assert_encode_error(function() end, "unsupported value type: function")
        assert_encode_error(coroutine.create(function() end), "unsupported value type: thread")
        assert_encode_error(newproxy(false), "unsupported value type: userdata")
    end)

    it("rejects non-64-bit-integer cdata shapes", function()
        assert_encode_error(ffi.new("double", 1.25), "unsupported value type: cdata")
        assert_encode_error(ffi.new("struct { int x; }", { x = 1 }), "unsupported value type: cdata")
        assert_encode_error(ffi.cast("void *", nil), "unsupported value type: cdata")
    end)

    it("rejects unsupported table values", function()
        assert_encode_error({fn = function() end}, "unsupported value type: function")
        assert_encode_error({co = coroutine.create(function() end)}, "unsupported value type: thread")
        assert_encode_error({ud = newproxy(false)}, "unsupported value type: userdata")
        assert_encode_error({d = ffi.new("double", 1.25)}, "unsupported value type: cdata")
    end)

    it("rejects keys that are neither string nor number", function()
        assert_encode_error({[true] = 1}, "object key must be a string or number, got boolean")
        assert_encode_error({[{}] = 1}, "object key must be a string or number, got table")
        assert_encode_error({[function() end] = 1}, "object key must be a string or number, got function")
        assert_encode_error({[coroutine.create(function() end)] = 1}, "object key must be a string or number, got thread")
        assert_encode_error({[newproxy(false)] = 1}, "object key must be a string or number, got userdata")
        assert_encode_error({[ffi.new("int64_t", 1)] = 1}, "object key must be a string or number, got cdata")
    end)
end)
