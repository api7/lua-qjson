local qjson    = require("qjson")
local cjson = require("cjson")

local function read_file(path)
    local f = assert(io.open(path, "rb"))
    local s = f:read("*a")
    f:close()
    return s
end

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

describe("qjson vs lua-cjson", function()
    it("agrees on simple string field", function()
        local s = '{"a":"x"}'
        assert.are.equal(cjson.decode(s).a, qjson.parse(s):get_str("a"))
    end)

    it("agrees on integer field", function()
        local s = '{"a":42}'
        assert.are.equal(cjson.decode(s).a, qjson.parse(s):get_i64("a"))
    end)

    it("agrees on float field", function()
        local s = '{"a":1.5}'
        assert.are.equal(cjson.decode(s).a, qjson.parse(s):get_f64("a"))
    end)

    it("agrees on bool field", function()
        local s = '{"a":true}'
        assert.are.equal(cjson.decode(s).a, qjson.parse(s):get_bool("a"))
    end)

    it("agrees on nested path", function()
        local s = '{"body":{"model":"gpt"}}'
        assert.are.equal(cjson.decode(s).body.model, qjson.parse(s):get_str("body.model"))
    end)

    local fixture_paths = {
        "tests/fixtures/third_party/cjson/test1.json",
        "tests/fixtures/third_party/cjson/test2.json",
        "tests/fixtures/third_party/cjson/test9.json",
        "tests/fixtures/third_party/cjson/test10.json",
        "tests/fixtures/third_party/cjson/test11.json",
        "tests/fixtures/third_party/simdjson/example_config.json",
    }

    for _, path in ipairs(fixture_paths) do
        it("materializes like lua-cjson for fixture " .. path, function()
            local src = read_file(path)
            assert.is_true(deep_equal(qjson.materialize(qjson.decode(src)), cjson.decode(src)))
        end)

        it("encodes a lua-cjson-equivalent value for fixture " .. path, function()
            local src = read_file(path)
            local out = qjson.encode(qjson.decode(src))
            assert.is_true(deep_equal(cjson.decode(out), cjson.decode(src)))
        end)
    end
end)
