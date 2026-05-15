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

-- Shape: a multimodal chat-completion request with one ~1.5K text question
-- and N base64-encoded image parts (each 50-500 KB) until the payload reaches
-- target_bytes. Mirrors the production case the bench is meant to reflect.
--
-- Image sizes are drawn from a deterministic Park-Miller LCG (not math.random,
-- which delegates to libc rand() and varies across machines) so the same
-- target_bytes produces byte-identical output on any LuaJIT 2.1 host. The
-- upper bound is capped at `remaining + slack` so the final overshoot vs
-- target stays inside ~10 KB.
local function make_payload(target_bytes)
    local rng_state = 42
    local function rng_range(lo, hi)
        -- Park-Miller minimal-standard LCG: a=48271, m=2^31-1. Multiplication
        -- fits in double precision (48271 * 2^31 < 2^53).
        rng_state = (rng_state * 48271) % 2147483647
        return lo + (rng_state % (hi - lo + 1))
    end

    local text = string.rep("Q", 1500)
    local text_part = '{"type":"text","text":"' .. text .. '"}'
    local parts = { text_part }
    local current = 200 + #text_part  -- approx outer envelope overhead

    while current < target_bytes do
        local remaining = target_bytes - current
        local img_size
        if remaining < 50 * 1024 then
            -- Final image: shrink below the 50 KB floor so the label matches
            -- the actual payload size. Bench iters all see the same payload
            -- regardless, so the smaller tail blob doesn't change what's
            -- being measured.
            img_size = math.max(1024, remaining)
        else
            local upper = math.min(500 * 1024, remaining)
            img_size = rng_range(50 * 1024, upper)
        end
        local b64 = string.rep("A", img_size)
        local img_part = '{"type":"image_url","image_url":{"url":"data:image/jpeg;base64,'
            .. b64 .. '"}}'
        parts[#parts + 1] = img_part
        current = current + #img_part + 1  -- +1 for comma
    end

    return '{"model":"gpt-4-vision","temperature":0.7,"messages":'
        .. '[{"role":"user","content":[' .. table.concat(parts, ",") .. ']}]}'
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
    {name = "2m",     iters = 20,   payload = make_payload(2 * 1024 * 1024)},
    {name = "5m",     iters = 20,   payload = make_payload(5 * 1024 * 1024)},
    {name = "10m",    iters = 20,   payload = make_payload(10 * 1024 * 1024)},
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
