-- ARM64 NEON benchmark: qjson vs lua-cjson (parse + access only)
-- Run from worktree root:
--   DYLD_LIBRARY_PATH=./target/release LUA_CPATH='./vendor/lua-cjson/?.so;./target/release/lib?.so' \
--     luajit arm_bench.lua

package.cpath = "./vendor/lua-cjson/?.so;./target/release/lib?.so;" .. package.cpath

local qjson  = require("qjson")
local cjson  = require("cjson")
local function make_b64_block()
    local b64_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    local rng = 12345
    local t = {}
    for i = 1, 64 * 1024 do
        rng = (rng * 48271) % 2147483647
        local idx = (rng % 64) + 1
        t[i] = b64_chars:sub(idx, idx)
    end
    return table.concat(t)
end

local function make_b64(size)
    if size <= B64_BLOCK_LEN then
        return B64_BLOCK:sub(1, size)
    end
    local reps = math.ceil(size / B64_BLOCK_LEN)
    return string.rep(B64_BLOCK, reps):sub(1, size)
end

local function make_payload(target_bytes)
    local message_count = math.max(1, math.ceil(target_bytes / (1024 * 1024)))
    local envelope = '{"model":"gpt-4-vision","temperature":0.7,"messages":[]}'
    local text = string.rep("Q", 256)
    local text_part = '{"type":"text","text":"' .. text .. '"}'
    local image_prefix = '{"type":"image_url","image_url":{"url":"data:image/jpeg;base64,'
    local image_suffix = '"}}'
    local message_overhead = #('{"role":"user","content":[,]}') + #text_part
        + #image_prefix + #image_suffix
    local remaining = target_bytes - #envelope - (message_count * message_overhead)
    local image_size = math.max(1024, math.floor(remaining / message_count))

    local messages = {}
    for i = 1, message_count do
        local role = i % 2 == 1 and "user" or "assistant"
        local b64 = make_b64(image_size)
        local image_part = image_prefix .. b64 .. image_suffix
        messages[i] = '{"role":"' .. role .. '","content":['
            .. text_part .. "," .. image_part .. ']}'
    end

    return '{"model":"gpt-4-vision","temperature":0.7,"messages":['
        .. table.concat(messages, ",") .. ']}'
end

local ROUNDS = 5

local function bench(name, iters, fn)
    local warmup = math.max(50, math.floor(iters / 5))
    for _ = 1, warmup do fn() end

    collectgarbage("collect")

    local ops = {}
    for r = 1, ROUNDS do
        local t0 = os.clock()
        for _ = 1, iters do fn() end
        local t1 = os.clock()
        ops[r] = iters / (t1 - t0)
    end

    table.sort(ops)
    return ops[math.ceil(ROUNDS / 2)]
end

local content_paths_cache = {}

local function content_paths(n)
    local paths = content_paths_cache[n]
    if paths then return paths end
    paths = {}
    for i = 0, n - 1 do
        paths[i + 1] = "messages[" .. i .. "].content"
    end
    content_paths_cache[n] = paths
    return paths
end

local scenarios = {
    {name = "small",   target = 2 * 1024,    iters = 5000},
    {name = "medium",  target = 60 * 1024,   iters = 500},
    {name = "100k",    target = 100 * 1024,  iters = 200},
    {name = "1m",      target = 1024 * 1024, iters = 50},
    {name = "10m",     target = 10 * 1024 * 1024, iters = 5},
}

B64_BLOCK = make_b64_block()
B64_BLOCK_LEN = #B64_BLOCK

io.write("Generating payloads...")
io.flush()
local payloads = {}
for _, s in ipairs(scenarios) do
    payloads[s.name] = make_payload(s.target)
    io.write(" " .. s.name)
    io.flush()
end
print(" done.")
print("")

local header_fmt = "%-10s %-10s %-12s %-12s %-10s"
print(string.format(header_fmt, "Scenario", "Size", "cjson", "qjson.parse", "speedup"))
print(string.rep("-", 58))

for _, s in ipairs(scenarios) do
    local payload = payloads[s.name]
    local size_kb = #payload / 1024
    local size_label
    if size_kb >= 1024 then
        size_label = string.format("%.1f MB", size_kb / 1024)
    else
        size_label = string.format("%.0f KB", size_kb)
    end

    local cjson_ops = bench("cjson " .. s.name, s.iters, function()
        local obj = cjson.decode(payload)
        local _ = obj.model
        local _ = obj.temperature
        if obj.messages then
            for _, msg in ipairs(obj.messages) do
                local _ = msg.content
            end
        end
    end)

    local qjson_ops = bench("qjson " .. s.name, s.iters, function()
        local doc = qjson.parse(payload)
        local _ = doc:get_str("model")
        local _ = doc:get_f64("temperature")
        local n = doc:len("messages") or 0
        local paths = content_paths(n)
        for i = 1, n do
            local _ = doc:typeof(paths[i])
        end
    end)

    local speedup = qjson_ops / cjson_ops
    print(string.format("%-10s %-10s %-12.0f %-12.0f %-10.1fx",
        s.name, size_label, cjson_ops, qjson_ops, speedup))
end
