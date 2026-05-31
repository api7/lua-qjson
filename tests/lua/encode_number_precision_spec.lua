local qjson = require("qjson")

local function assert_invalid_precision(value)
    local ok, err = pcall(qjson.encode_number_precision, value)
    assert.is_false(ok)
    assert.matches("expected integer between 1 and 14$", tostring(err))
end

local function with_precision(precision, fn)
    local old = qjson.encode_number_precision(precision)
    local ok, err = pcall(fn)
    qjson.encode_number_precision(old)
    if not ok then
        error(err, 0)
    end
end

describe("qjson.encode_number_precision", function()
    it("defaults to precision 14", function()
        assert.are.equal(14, qjson.encode_number_precision())
    end)

    it("sets precision and returns the previous value", function()
        with_precision(14, function()
            local old = qjson.encode_number_precision(3)
            assert.are.equal(14, old)
            assert.are.equal(3, qjson.encode_number_precision())

            local old_again = qjson.encode_number_precision(14)
            assert.are.equal(3, old_again)
            assert.are.equal(14, qjson.encode_number_precision())
        end)
    end)

    it("accepts boundary values 1 and 14", function()
        with_precision(14, function()
            assert.are.equal(14, qjson.encode_number_precision(1))
            assert.are.equal(1, qjson.encode_number_precision())
            assert.are.equal(1, qjson.encode_number_precision(14))
            assert.are.equal(14, qjson.encode_number_precision())
        end)
    end)

    it("rejects invalid precision values with the expected message", function()
        with_precision(14, function()
            assert_invalid_precision(-1)
            assert_invalid_precision(0)
            assert_invalid_precision(3.5)
            assert_invalid_precision(15)
            assert_invalid_precision("3")
            assert_invalid_precision(true)
            assert_invalid_precision({})
            assert.are.equal(14, qjson.encode_number_precision())
        end)
    end)

    it("uses precision for floating-point encoding", function()
        local n = 1 / 3
        with_precision(5, function()
            assert.are.equal(string.format("%.5g", n), qjson.encode(n))
        end)
        with_precision(12, function()
            assert.are.equal(string.format("%.12g", n), qjson.encode(n))
        end)
    end)

    it("keeps integer encoding unchanged", function()
        local n = 12345678901234
        with_precision(1, function()
            assert.are.equal(string.format("%d", n), qjson.encode(n))
        end)
        with_precision(14, function()
            assert.are.equal(string.format("%d", n), qjson.encode(n))
        end)
    end)

    it("uses configured precision when encoding dirty lazy proxies", function()
        with_precision(4, function()
            local lazy = qjson.decode('{"n":3.141592653589793,"k":1}')
            lazy.k = 2
            assert.are.equal('{"n":3.142,"k":2}', qjson.encode(lazy))
        end)
    end)

    it("uses configured precision for dirty lazy proxy tostring", function()
        with_precision(2, function()
            local lazy = qjson.decode('{"n":3.141592653589793}')
            lazy.n = 3.141592653589793
            assert.are.equal('{"n":3.1}', tostring(lazy))
        end)
    end)

    it("keeps clean lazy proxy fast path as original bytes", function()
        with_precision(2, function()
            local src = '{"n":3.141592653589793,"arr":[0.12345678901234567]}'
            local lazy = qjson.decode(src)
            assert.are.equal(src, qjson.encode(lazy))
            assert.are.equal(src, tostring(lazy))
        end)
    end)
end)
