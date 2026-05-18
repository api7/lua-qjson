-- Minimal probe for perf: hammers qjson.parse on a fixed 100K payload so perf
-- samples concentrate on the FFI entry + parse hot path. Not a benchmark —
-- there is no timing or memory accounting here, just sustained work.

package.path  = package.path  .. ";./lua/?.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"

local qjson = require("qjson")

-- Same payload generator as lua_bench.lua so probe output corresponds to
-- the same shape the bench measures. Park-Miller LCG keeps it deterministic.
local function make_payload(target_bytes)
    local rng_state = 42
    local function rng_range(lo, hi)
        rng_state = (rng_state * 48271) % 2147483647
        return lo + (rng_state % (hi - lo + 1))
    end

    local text = string.rep("Q", 1500)
    local text_part = '{"type":"text","text":"' .. text .. '"}'
    local parts = { text_part }
    local current = 200 + #text_part

    while current < target_bytes do
        local remaining = target_bytes - current
        local img_size
        if remaining < 50 * 1024 then
            img_size = math.max(1024, remaining)
        else
            local upper = math.min(500 * 1024, remaining)
            img_size = rng_range(50 * 1024, upper)
        end
        local b64 = string.rep("A", img_size)
        local img_part = '{"type":"image_url","image_url":{"url":"data:image/jpeg;base64,'
            .. b64 .. '"}}'
        parts[#parts + 1] = img_part
        current = current + #img_part + 1
    end

    return '{"model":"gpt-4-vision","temperature":0.7,"messages":'
        .. '[{"role":"user","content":[' .. table.concat(parts, ",") .. ']}]}'
end

local payload = make_payload(100 * 1024)
local iters = tonumber(arg[1]) or 500000

-- Warmup so JIT traces compile before perf starts sampling steady state.
for _ = 1, 1000 do
    local d = qjson.parse(payload)
    local _ = d:get_str("model")
end

io.stderr:write(string.format("probe: %d bytes payload, %d iters\n", #payload, iters))

for _ = 1, iters do
    local d = qjson.parse(payload)
    local _ = d:get_str("model")
    local _ = d:get_f64("temperature")
    local _ = d:get_str("messages[0].role")
end
