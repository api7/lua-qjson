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

local function make_payload(target_bytes)
    local header = '{"model":"gpt-4","temperature":0.7,"messages":['
    local footer = ']}'
    local msg = '{"role":"user","content":"' .. string.rep("x", 64) .. '"}'
    local parts = {}
    local current = #header + #footer
    while current + #msg + 1 <= target_bytes do
        parts[#parts + 1] = msg
        current = current + #msg + 1
    end
    return header .. table.concat(parts, ",") .. footer
end

local function bench(name, iters, fn)
    collectgarbage("collect")
    local mem_before = collectgarbage("count")
    local t0 = os.clock()
    for _ = 1, iters do fn() end
    local t1 = os.clock()
    local mem_after = collectgarbage("count")
    local elapsed = t1 - t0
    print(string.format("%-44s  %7.2fms total   %10.0f ops/s   %+8.1fKB",
        name, elapsed * 1000, iters / elapsed,
        mem_after - mem_before))
end

local scenarios = {
    {name = "small",  iters = 5000, payload = read_file("benches/fixtures/small_api.json")},
    {name = "medium", iters = 500,  payload = read_file("benches/fixtures/medium_resp.json")},
    {name = "100k",   iters = 100,  payload = make_payload(100 * 1024)},
    {name = "200k",   iters = 50,   payload = make_payload(200 * 1024)},
    {name = "500k",   iters = 20,   payload = make_payload(500 * 1024)},
    {name = "1m",     iters = 15,   payload = make_payload(1024 * 1024)},
    {name = "2m",     iters = 10,   payload = make_payload(2 * 1024 * 1024)},
    {name = "5m",     iters = 10,   payload = make_payload(5 * 1024 * 1024)},
    {name = "10m",    iters = 10,   payload = make_payload(10 * 1024 * 1024)},
}

for _, s in ipairs(scenarios) do
    print(string.format("=== %s (%d bytes) ===", s.name, #s.payload))

    bench("cjson.decode + access 3 fields", s.iters, function()
        local obj = cjson.decode(s.payload)
        local _ = obj.model
        local _ = obj.temperature
        local _ = obj.messages and obj.messages[1] and obj.messages[1].role
    end)

    bench("quickdecode.parse + access 3 fields", s.iters, function()
        local d = qd.parse(s.payload)
        local _ = d:get_str("model")
        local _ = d:get_f64("temperature")
        local _ = d:get_str("messages[0].role")
    end)
end
