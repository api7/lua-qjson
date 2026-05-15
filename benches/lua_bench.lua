package.path  = package.path  .. ";./lua/?.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"

local qd    = require("quickdecode")
local cjson = require("cjson")

local function read_file(p)
    local f = assert(io.open(p, "rb"))
    local s = f:read("*a")
    f:close()
    return s
end

local function bench(name, iters, fn)
    collectgarbage("collect")
    local mem_before = collectgarbage("count")
    local t0 = os.clock()
    for _ = 1, iters do fn() end
    local t1 = os.clock()
    local mem_after = collectgarbage("count")
    print(string.format("%-44s  %7.2fms total   %6.2fus/op   %+8.1fKB",
        name, (t1 - t0) * 1000, (t1 - t0) * 1e6 / iters,
        mem_after - mem_before))
end

local fixtures = {
    small  = read_file("benches/fixtures/small_api.json"),
    medium = read_file("benches/fixtures/medium_resp.json"),
}

local iters_for = { small = 5000, medium = 500 }

for _, size in ipairs({"small", "medium"}) do
    local payload = fixtures[size]
    print(string.format("=== %s (%d bytes) ===", size, #payload))

    bench("cjson.decode + access 3 fields", iters_for[size], function()
        local obj = cjson.decode(payload)
        local _ = obj.model
        local _ = obj.temperature
        local _ = obj.messages and obj.messages[1] and obj.messages[1].role
    end)

    bench("quickdecode.parse + access 3 fields", iters_for[size], function()
        local d = qd.parse(payload)
        local _ = d:get_str("model")
        local _ = d:get_f64("temperature")
        local _ = d:get_str("messages[0].role")
    end)
end
