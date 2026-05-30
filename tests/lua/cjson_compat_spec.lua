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

local function assert_materializes_like_lua_cjson(src)
    assert.is_true(deep_equal(qjson.materialize(qjson.decode(src)), cjson.decode(src)))
end

local function assert_encodes_like_lua_cjson(src)
    local out = qjson.encode(qjson.decode(src))
    assert.is_true(deep_equal(cjson.decode(out), cjson.decode(src)))
end

local function assert_equivalent_json(src)
    assert_materializes_like_lua_cjson(src)
    assert_encodes_like_lua_cjson(src)
end

local function assert_fixture_paths(paths)
    for _, path in ipairs(paths) do
        local p = path

        it("materializes like lua-cjson for fixture " .. p, function()
            assert_materializes_like_lua_cjson(read_file(p))
        end)

        it("encodes a lua-cjson-equivalent value for fixture " .. p, function()
            assert_encodes_like_lua_cjson(read_file(p))
        end)
    end
end

describe("qjson lua-cjson compatibility smoke", function()
    it("agrees on simple string field", function()
        local s = '{"a":"x"}'
        assert.are.equal(cjson.decode(s).a, qjson.parse(s):get_str("a"))
    end)

    it("agrees on integer field", function()
        local s = '{"a":42}'
        assert.are.equal(cjson.decode(s).a, tonumber(qjson.parse(s):get_i64("a")))
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
end)

describe("qjson cJSON upstream fixtures", function()
    assert_fixture_paths({
        "tests/vendor/cJSON/tests/inputs/test1",
        "tests/vendor/cJSON/tests/inputs/test2",
        "tests/vendor/cJSON/tests/inputs/test3",
        "tests/vendor/cJSON/tests/inputs/test4",
        "tests/vendor/cJSON/tests/inputs/test5",
        "tests/vendor/cJSON/tests/inputs/test7",
        "tests/vendor/cJSON/tests/inputs/test8",
        "tests/vendor/cJSON/tests/inputs/test9",
        "tests/vendor/cJSON/tests/inputs/test10",
        "tests/vendor/cJSON/tests/inputs/test11",
        "tests/vendor/cJSON/tests/json-patch-tests/cjson-utils-tests.json",
        "tests/vendor/cJSON/tests/json-patch-tests/package.json",
        "tests/vendor/cJSON/tests/json-patch-tests/spec_tests.json",
        "tests/vendor/cJSON/tests/json-patch-tests/tests.json",
    })
end)

describe("qjson simdjson upstream fixtures", function()
    assert_fixture_paths({
        "tests/vendor/simdjson/jsonexamples/citm_catalog.json",
        "tests/vendor/simdjson/jsonexamples/example_config.json",
        "tests/vendor/simdjson/jsonexamples/twitter.json",
    })

    it("materializes and encodes each simdjson NDJSON record like lua-cjson", function()
        local src = read_file("tests/vendor/simdjson/jsonexamples/amazon_cellphones.ndjson")
        local records = 0
        for line in src:gmatch("([^\r\n]+)") do
            records = records + 1
            assert_equivalent_json(line)
        end
        assert.is_true(records >= 793)
    end)
end)
