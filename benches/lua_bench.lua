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
-- target_bytes produces byte-identical output on any LuaJIT 2.1 host.
--
-- Size accuracy: the normal-branch upper is `min(500K, remaining)` so the
-- loop cannot overshoot during steady state. When fewer than 50 KB remain
-- the final image falls through to `math.max(1024, remaining)` — undershoot
-- is at most a few hundred bytes; worst-case overshoot is ~1 KB (only when
-- `remaining < 1024`, which the seed=42 walk does not hit for our ladder).
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

local ROUNDS = 5

local function bench(name, iters, fn)
    -- Warmup pass: lets JIT compile hot traces and any one-time pools fill
    -- before measurement starts. Excluded from timing and memory delta.
    local warmup = math.max(3, math.floor(iters / 5))
    for _ = 1, warmup do fn() end

    collectgarbage("collect")
    local mem_before = collectgarbage("count")

    local ops = {}
    for r = 1, ROUNDS do
        local t0 = os.clock()
        for _ = 1, iters do fn() end
        local t1 = os.clock()
        ops[r] = iters / (t1 - t0)
    end
    local mem_after = collectgarbage("count")

    table.sort(ops)
    local median = ops[math.ceil(ROUNDS / 2)]
    local lo, hi = ops[1], ops[ROUNDS]
    local sum = 0
    for i = 1, ROUNDS do sum = sum + ops[i] end
    local mean = sum / ROUNDS

    print(string.format(
        "%-44s  median %9.0f ops/s   mean %9.0f   range %7.0f..%-9.0f   %+8.1fKB",
        name, median, mean, lo, hi, mem_after - mem_before))
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

-- The pooled API (qd.new_decoder + :parse) only exists on commits that
-- landed the Decoder refactor. Probe so the bench still runs on older builds.
local has_pooled_api = type(qd.new_decoder) == "function"
local pooled_decoder = has_pooled_api and qd.new_decoder() or nil

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

    if has_pooled_api then
        bench("quickdecode pooled :parse + access 3 fields", s.iters, function()
            local d = pooled_decoder:parse(s.payload)
            local _ = d:get_str("model")
            local _ = d:get_f64("temperature")
            local _ = d:get_str("messages[0].role")
        end)

        -- One-shot-per-request pattern: each iter creates a fresh decoder,
        -- parses once, and lets both decoder and doc fall to GC. No reuse.
        -- This is the typical "user does not cache the decoder" path.
        bench("quickdecode new_decoder()+parse (one-shot)", s.iters, function()
            local dec = qd.new_decoder()
            local d = dec:parse(s.payload)
            local _ = d:get_str("model")
            local _ = d:get_f64("temperature")
            local _ = d:get_str("messages[0].role")
        end)
    end

    bench("qd.decode + t.field x3", s.iters, function()
        local t = qd.decode(s.payload)
        local _ = t.model
        local _ = t.temperature
        local _ = t.messages and t.messages[1] and t.messages[1].role
    end)

    bench("qd.decode + qd.encode (unmodified)", s.iters, function()
        local t = qd.decode(s.payload)
        local _ = qd.encode(t)
    end)
end

-- Interleaved scenario: cycle through several payloads of different sizes
-- back-to-back, mirroring a server processing variable-size requests. The
-- single-payload loops above hand the allocator the same block over and over
-- and have no allocation to amortize away — they cannot exercise the doc
-- pool. This scenario can.
local function scenario_by_name(n)
    for _, s in ipairs(scenarios) do
        if s.name == n then return s end
    end
    error("no scenario " .. n)
end

local interleaved_names = {"100k", "200k", "500k", "1m"}
local interleaved = {}
for _, n in ipairs(interleaved_names) do
    interleaved[#interleaved + 1] = scenario_by_name(n).payload
end

local function make_cycler(items)
    local i = 0
    local n = #items
    return function()
        i = i + 1
        return items[((i - 1) % n) + 1]
    end
end

print(string.format("=== interleaved %s ===", table.concat(interleaved_names, ",")))

do
    local next_p = make_cycler(interleaved)
    bench("cjson.decode + access 3 fields", 400, function()
        local p = next_p()
        local obj = cjson.decode(p)
        local _ = obj.model
        local _ = obj.temperature
        local _ = obj.messages and obj.messages[1] and obj.messages[1].role
    end)

    next_p = make_cycler(interleaved)
    bench("quickdecode.parse + access 3 fields", 400, function()
        local p = next_p()
        local d = qd.parse(p)
        local _ = d:get_str("model")
        local _ = d:get_f64("temperature")
        local _ = d:get_str("messages[0].role")
    end)

    if has_pooled_api then
        next_p = make_cycler(interleaved)
        bench("quickdecode pooled :parse + access 3 fields", 400, function()
            local p = next_p()
            local d = pooled_decoder:parse(p)
            local _ = d:get_str("model")
            local _ = d:get_f64("temperature")
            local _ = d:get_str("messages[0].role")
        end)
    end

    next_p = make_cycler(interleaved)
    bench("qd.decode + t.field x3", 400, function()
        local p = next_p()
        local t = qd.decode(p)
        local _ = t.model
        local _ = t.temperature
        local _ = t.messages and t.messages[1] and t.messages[1].role
    end)

    next_p = make_cycler(interleaved)
    bench("qd.decode + qd.encode (unmodified)", 400, function()
        local p = next_p()
        local t = qd.decode(p)
        local _ = qd.encode(t)
    end)
end
