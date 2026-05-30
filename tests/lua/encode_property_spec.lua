local qjson = require("qjson")
local prop = require("tests.lua.property_json")

local CASES = tonumber(os.getenv("QJSON_PROP_CASES")) or 200
local SEED  = tonumber(os.getenv("QJSON_PROP_SEED"))  or 760076

describe("qjson Lua encode property coverage", function()
    it("round-trips generated containers through materialize and encode", function()
        local rng = prop.rng(SEED)
        for i = 1, CASES do
            local src = prop.encode_json(prop.gen_container(rng))
            local materialized = qjson.materialize(qjson.decode(src))
            local encoded = qjson.encode(materialized)
            local reparsed = qjson.materialize(qjson.decode(encoded))

            assert.is_true(
                prop.deep_equal(materialized, reparsed),
                string.format("case=%d seed=%d src=%s encoded=%s", i, SEED, src, encoded)
            )
        end
    end)

    it("keeps the encoder max-depth boundary stable for generated chains", function()
        local rng = prop.rng(SEED + 1)
        for depth = 995, 1005 do
            local chain = prop.gen_deep_chain(rng, depth)
            local ok, err = pcall(qjson.encode, chain)
            if depth <= 1000 then
                assert.is_true(ok, string.format("depth=%d err=%s", depth, tostring(err)))
            else
                assert.is_false(ok, string.format("depth=%d unexpectedly encoded", depth))
                assert.matches("qjson.encode: max depth exceeded", tostring(err), 1, true)
            end
        end
    end)
end)
