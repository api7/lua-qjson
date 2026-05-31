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

describe("qjson.encode_sparse_array lua-cjson compatible controls", function()
    local function reset_sparse_defaults()
        qjson.encode_sparse_array(false, 2, 10)
    end

    local function assert_sparse_array_arg_error(fn, expected)
        local ok, err = pcall(fn)
        assert.is_false(ok)
        assert.matches(expected, tostring(err), 1, true)
    end

    before_each(function()
        reset_sparse_defaults()
    end)

    after_each(function()
        reset_sparse_defaults()
    end)

    it("returns lua-cjson compatible defaults via getter", function()
        assert.same({false, 2, 10}, {qjson.encode_sparse_array()})
    end)

    it("setter always returns a triplet and getter reflects current values", function()
        assert.same({true, 2, 10}, {qjson.encode_sparse_array(true)})
        assert.same({true, 2, 3}, {qjson.encode_sparse_array(nil, nil, 3)})
        assert.same({true, 2, 3}, {qjson.encode_sparse_array()})
    end)

    it("only updates fields that are explicitly provided", function()
        assert.same({true, 5, 7}, {qjson.encode_sparse_array(true, 5, 7)})
        assert.same({true, 0, 7}, {qjson.encode_sparse_array(nil, 0, nil)})
        assert.same({false, 0, 7}, {qjson.encode_sparse_array(false, nil, nil)})
        assert.same({false, 0, 11}, {qjson.encode_sparse_array(nil, nil, 11)})
    end)

    it("treats any truthy convert input as true", function()
        assert.same({true, 2, 10}, {qjson.encode_sparse_array("on")})
        assert.same({true, 2, 10}, {qjson.encode_sparse_array()})
    end)

    it("defaults to rejecting excessively sparse arrays", function()
        assert_encode_error({[1] = "one", [1000] = "thousand"}, "excessively sparse array")
    end)

    it("forbids any sparse holes when ratio=1 and safe=0", function()
        qjson.encode_sparse_array(false, 1, 0)
        assert_encode_error({[1] = 1, [3] = 3}, "excessively sparse array")
        assert.are.equal("[1,2]", qjson.encode({[1] = 1, [2] = 2}))
    end)

    it("converts excessively sparse arrays to objects when convert=true", function()
        qjson.encode_sparse_array(true)
        assert_json_equal(qjson.encode({[1] = "one", [1000] = "thousand"}), '{"1":"one","1000":"thousand"}')
    end)

    it("disables excessive sparse checks when ratio=0 and fills null holes", function()
        qjson.encode_sparse_array(false, 0, 10)
        assert.are.equal("[1,null,null,null,null,6]", qjson.encode({[1] = 1, [6] = 6}))
        assert.are.equal('[null,null,"v"]', qjson.encode({[3] = "v"}))
    end)

    it("applies safe threshold before triggering excessive sparse handling", function()
        qjson.encode_sparse_array(false, 2, 7)
        assert.are.equal('["a",null,null,null,null,null,"g"]', qjson.encode({[1] = "a", [7] = "g"}))

        qjson.encode_sparse_array(true, 2, 5)
        assert_json_equal(qjson.encode({[1] = "a", [7] = "g"}), '{"1":"a","7":"g"}')
    end)

    it("rejects invalid ratio and safe values", function()
        assert_sparse_array_arg_error(function()
            qjson.encode_sparse_array(nil, -1)
        end, "bad argument #2 to qjson.encode_sparse_array (expected non-negative integer)")
        assert_sparse_array_arg_error(function()
            qjson.encode_sparse_array(nil, 1.5)
        end, "bad argument #2 to qjson.encode_sparse_array (expected non-negative integer)")
        assert_sparse_array_arg_error(function()
            qjson.encode_sparse_array(nil, math.huge)
        end, "bad argument #2 to qjson.encode_sparse_array (expected non-negative integer)")
        assert_sparse_array_arg_error(function()
            qjson.encode_sparse_array(nil, nil, -1)
        end, "bad argument #3 to qjson.encode_sparse_array (expected non-negative integer)")
        assert_sparse_array_arg_error(function()
            qjson.encode_sparse_array(nil, nil, 1.2)
        end, "bad argument #3 to qjson.encode_sparse_array (expected non-negative integer)")
        assert_sparse_array_arg_error(function()
            qjson.encode_sparse_array(nil, nil, math.huge)
        end, "bad argument #3 to qjson.encode_sparse_array (expected non-negative integer)")
    end)

    it("keeps sparse-array settings unchanged when setter validation fails", function()
        qjson.encode_sparse_array(false, 5, 7)

        assert_sparse_array_arg_error(function()
            qjson.encode_sparse_array(true, -1, 9)
        end, "bad argument #2 to qjson.encode_sparse_array (expected non-negative integer)")
        assert.same({false, 5, 7}, {qjson.encode_sparse_array()})

        assert_sparse_array_arg_error(function()
            qjson.encode_sparse_array(true, 6, math.huge)
        end, "bad argument #3 to qjson.encode_sparse_array (expected non-negative integer)")
        assert.same({false, 5, 7}, {qjson.encode_sparse_array()})
    end)

    it("rejects too many arguments", function()
        assert_sparse_array_arg_error(function()
            qjson.encode_sparse_array(false, 2, 10, true)
        end, "bad argument #4 to qjson.encode_sparse_array (found too many arguments)")
    end)

    it("keeps default sparse-array encoding behavior compatible with lua-cjson", function()
        local value = {[1] = "one", [5] = "five"}
        assert_json_equal(qjson.encode(value), cjson.encode(value))
    end)

    it("does not let strict sparse settings affect empty_array_mt or lazy-array paths", function()
        qjson.encode_sparse_array(false, 1, 0)

        assert.are.equal("[]", qjson.encode(setmetatable({}, qjson.empty_array_mt)))

        local lazy_clean = qjson.decode("[1,null,3]")
        assert.are.equal("[1,null,3]", qjson.encode(lazy_clean))

        local lazy_materialized = qjson.decode("[1,2]")
        lazy_materialized[4] = 4
        assert.are.equal("[1,2,null,4]", qjson.encode(lazy_materialized))
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
