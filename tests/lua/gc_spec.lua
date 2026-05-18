local qjson = require("qjson")

describe("qjson GC", function()
    it("collects Doc without crashing and frees underlying qjson_doc", function()
        -- Create and drop many Docs to exercise the ffi.gc finalizer path.
        -- A leak or double-free would surface as either crash, memory growth,
        -- or use-after-free under valgrind. Here we just confirm the loop
        -- completes and that values remain correct mid-loop.
        for i = 1, 200 do
            local d = qjson.parse(string.format('{"i":%d}', i))
            assert.are.equal(i, d:get_i64("i"))
            d = nil  -- drop reference
        end
        collectgarbage("collect")
        collectgarbage("collect")
    end)

    it("Doc finalizer runs after collectgarbage", function()
        -- Use a weak table to confirm the Doc is reachable for collection.
        local refs = setmetatable({}, { __mode = "v" })
        do
            local d = qjson.parse('{"a":1}')
            refs[1] = d
            assert.are.equal(1, d:get_i64("a"))
        end
        collectgarbage("collect")
        collectgarbage("collect")
        assert.is_nil(refs[1])
    end)
end)
