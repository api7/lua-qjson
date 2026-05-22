package.path  = package.path  .. ";./lua/?.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"

local qjson    = require("qjson")
local cjson = require("cjson")
local simdjson_ok, simdjson_or_err = pcall(function()
    return require("resty.simdjson").new()
end)
local simdjson = simdjson_ok and simdjson_or_err or nil

local function read_file(p)
    local f = assert(io.open(p, "rb"))
    local s = f:read("*a")
    f:close()
    return s
end

-- Shape: a multimodal chat-completion request with one or more historical
-- messages. Each message contains one small text part and one base64-encoded
-- image part. The number of messages scales with payload size: a 10 MB request
-- has roughly ten 1 MB image-bearing messages.
--
-- Size accuracy: payload sizing is approximate. Message separators, role
-- strings, and the 1 KB minimum image size can add small drift from
-- `target_bytes` on tiny scenarios; larger scenarios stay close to target.
-- GitHub-style payload: simulates /repos/{owner}/{repo}/issues response.
-- Each issue has ~20 fields including nested user object, labels array,
-- and realistic string lengths (URLs, timestamps, markdown body).
-- Structural density ~3-5%, matching real GitHub API responses.
local function make_github_issues_payload(target_bytes)
    local issues = {}
    local current = 2  -- outer envelope: [...]

    local issue_num = 1
    while current < target_bytes do
        local labels = {}
        local label_count = (issue_num % 4)  -- 0-3 labels per issue
        for i = 1, label_count do
            labels[#labels + 1] = string.format(
                '{"id":%d,"name":"label-%d","color":"%06x","description":"Label description for categorization"}',
                10000 + issue_num * 10 + i, i, (issue_num * 12345 + i) % 0xFFFFFF)
        end

        local body_len = 200 + (issue_num % 5) * 100  -- 200-600 chars
        local body = string.rep("Lorem ipsum dolor sit amet. ", math.ceil(body_len / 29)):sub(1, body_len)

        local issue = string.format([[{
"id":%d,
"number":%d,
"title":"Issue title describing the problem or feature request #%d",
"body":"%s",
"state":"%s",
"locked":%s,
"comments":%d,
"user":{"login":"user%d","id":%d,"avatar_url":"https://avatars.githubusercontent.com/u/%d?v=4","type":"User","site_admin":false},
"labels":[%s],
"assignees":[],
"milestone":null,
"created_at":"2024-%02d-%02dT%02d:%02d:%02dZ",
"updated_at":"2024-%02d-%02dT%02d:%02d:%02dZ",
"closed_at":null,
"author_association":"CONTRIBUTOR",
"html_url":"https://github.com/example/repo/issues/%d",
"url":"https://api.github.com/repos/example/repo/issues/%d",
"repository_url":"https://api.github.com/repos/example/repo",
"labels_url":"https://api.github.com/repos/example/repo/issues/%d/labels{/name}",
"comments_url":"https://api.github.com/repos/example/repo/issues/%d/comments",
"events_url":"https://api.github.com/repos/example/repo/issues/%d/events"
}]],
            1000000 + issue_num,
            issue_num,
            issue_num,
            body,
            issue_num % 3 == 0 and "closed" or "open",
            issue_num % 7 == 0 and "true" or "false",
            issue_num % 50,
            issue_num % 100, 100000 + issue_num, 100000 + issue_num,
            table.concat(labels, ","),
            (issue_num % 12) + 1, (issue_num % 28) + 1, issue_num % 24, issue_num % 60, issue_num % 60,
            (issue_num % 12) + 1, (issue_num % 28) + 1, (issue_num + 1) % 24, (issue_num + 5) % 60, (issue_num + 10) % 60,
            issue_num, issue_num, issue_num, issue_num, issue_num)

        -- Remove newlines for compact JSON
        issue = issue:gsub("\n", "")
        issues[#issues + 1] = issue
        current = current + #issue + 1
        issue_num = issue_num + 1
    end

    return "[" .. table.concat(issues, ",") .. "]"
end

-- Pre-generate a 64 KB block of pseudo-random base64 characters.
-- Reused via repetition for larger image payloads to avoid O(n) generation.
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

local B64_BLOCK = make_b64_block()
local B64_BLOCK_LEN = #B64_BLOCK

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

local ROUNDS = 10

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

-- Default accessors for multimodal payloads
local function default_cjson_access(obj)
    local _ = obj.model
    local _ = obj.temperature
    if obj.messages then
        for _, msg in ipairs(obj.messages) do
            local _ = msg.content
        end
    end
end

local content_paths_by_message_count = {}

local function content_paths(n)
    local paths = content_paths_by_message_count[n]
    if paths then
        return paths
    end

    paths = {}
    for i = 0, n - 1 do
        paths[i + 1] = "messages[" .. i .. "].content"
    end
    content_paths_by_message_count[n] = paths
    return paths
end

local function default_qjson_access(d)
    local _ = d:get_str("model")
    local _ = d:get_f64("temperature")
    local n = d:len("messages") or 0
    local paths = content_paths(n)
    for i = 1, n do
        local _ = d:typeof(paths[i])
    end
end

local function default_table_access(t)
    local _ = t.model
    local _ = t.temperature
    if t.messages then
        for i = 1, qjson.len(t.messages) do
            local msg = t.messages[i]
            local _ = msg.content
        end
    end
end

-- Safe UTF-8 truncation: backs up past incomplete multi-byte sequences.
local function safe_sub(s, len)
    if #s <= len then return s end
    local pos = len
    while pos > 0 and s:byte(pos) >= 0x80 and s:byte(pos) < 0xC0 do pos = pos - 1 end
    if pos > 0 then
        local lead = s:byte(pos)
        local need = 0
        if lead >= 0xF0 then need = 3
        elseif lead >= 0xE0 then need = 2
        elseif lead >= 0xC2 then need = 1
        end
        if len - pos < need then pos = pos - 1 end
        while pos > 0 and s:byte(pos) >= 0x80 and s:byte(pos) < 0xC0 do pos = pos - 1 end
    end
    return s:sub(1, pos)
end

-- CJK GitHub-issues payload: same 20-field structure as github-100k but
-- with Chinese text and emoji in body/title/labels. Directly comparable
-- to github-100k — isolates the UTF-8 / high-bit byte impact.
local function make_cjk_payload(target_bytes)
    local issues = {}
    local current = 2
    local n = 1
    local cjk_body = "这是一段用于模拟GitHub Issues中文描述的测试文本包含常见的开发术语问题报告功能请求以及Bug修复记录"
        .. "😀🎉💡✨🚀🌟🔥🎊💯👍❤️🌍📱🎵🏆🍕🎮📚💻🔑🎁"
    local cjk_title = "修复用户登录页面在移动端的显示问题并优化响应式布局"
    while current < target_bytes do
        local labels = {}
        local label_count = (n % 4)
        local label_names = { "缺陷bug", "功能增强", "文档优化", "性能改进" }
        for i = 1, label_count do
            labels[#labels + 1] = string.format(
                [[{"id":%d,"name":"%s","color":"%06x","description":"标签分类描述"}]],
                10000 + n * 10 + i, label_names[i], (n * 12345 + i) % 0xFFFFFF)
        end
        -- Use whole multiples of cjk_body to avoid UTF-8 truncation
        local reps = 1 + (n % 3)
        local body = string.rep(cjk_body, reps)
        local issue = string.format([[{
"id":%d,
"number":%d,
"title":"%s #%d",
"body":"%s",
"state":"%s",
"locked":%s,
"comments":%d,
"user":{"login":"用户%d","id":%d,"avatar_url":"https://avatars.githubusercontent.com/u/%d?v=4","type":"用户","site_admin":false},
"labels":[%s],
"assignees":[],
"milestone":null,
"created_at":"2024-%02d-%02dT%02d:%02d:%02dZ",
"updated_at":"2024-%02d-%02dT%02d:%02d:%02dZ",
"closed_at":null,
"author_association":"贡献者",
"html_url":"https://github.com/example/中文仓库/issues/%d",
"url":"https://api.github.com/repos/example/中文仓库/issues/%d",
"repository_url":"https://api.github.com/repos/example/中文仓库",
"labels_url":"https://api.github.com/repos/example/中文仓库/issues/%d/labels{/名称}",
"comments_url":"https://api.github.com/repos/example/中文仓库/issues/%d/评论",
"events_url":"https://api.github.com/repos/example/中文仓库/issues/%d/事件"
}]],
            1000000 + n, n, cjk_title, n, body,
            n % 3 == 0 and "已关闭" or "进行中",
            n % 7 == 0 and "true" or "false",
            n % 50, n % 100, 100000 + n, 100000 + n,
            table.concat(labels, ","),
            (n % 12) + 1, (n % 28) + 1, n % 24, n % 60, n % 60,
            (n % 12) + 1, (n % 28) + 1, (n + 1) % 24, (n + 5) % 60, (n + 10) % 60,
            n, n, n, n, n)
        issue = issue:gsub("\n", "")
        if current + #issue + 3 > target_bytes then break end
        issues[#issues + 1] = issue
        current = current + #issue + 1
        n = n + 1
    end
    return "[" .. table.concat(issues, ",") .. "]"
end

local function cjk_qjson_access(d)
    if not d then return end
    local _ = d:get_i64("[0].id")
    local _ = d:get_str("[0].title")
    local _ = d:get_str("[0].user.login")
end

local function cjk_table_access(t)
    local _ = t[1] and t[1].id
    local _ = t[1] and t[1].title
    local _ = t[1] and t[1].user and t[1].user.login
end

local function cjk_cjson_access(obj)
    local _ = obj[1] and obj[1].id
    local _ = obj[1] and obj[1].title
    local _ = obj[1] and obj[1].user and obj[1].user.login
end

-- GitHub issues accessors: array of issues, access first issue's fields
local function github_cjson_access(obj)
    local _ = obj[1] and obj[1].id
    local _ = obj[1] and obj[1].title
    local _ = obj[1] and obj[1].user and obj[1].user.login
end

local function github_qjson_access(d)
    local _ = d:get_i64("[0].id")
    local _ = d:get_str("[0].title")
    local _ = d:get_str("[0].user.login")
end

local function github_table_access(t)
    local _ = t[1] and t[1].id
    local _ = t[1] and t[1].title
    local _ = t[1] and t[1].user and t[1].user.login
end

local scenarios = {
    {name = "small",  iters = 5000, payload = read_file("benches/fixtures/small_api.json")},
    {name = "medium", iters = 500,  payload = read_file("benches/fixtures/medium_resp.json")},
    {name = "github-100k",  iters = 100, payload = make_github_issues_payload(100 * 1024),
     cjson_access = github_cjson_access, qjson_access = github_qjson_access, table_access = github_table_access},
    {name = "cjk-100k",     iters = 100, payload = make_cjk_payload(100 * 1024),
     cjson_access = cjk_cjson_access, qjson_access = cjk_qjson_access, table_access = cjk_table_access,
     no_simdjson = true},
    {name = "100k",   iters = 100,  payload = make_payload(100 * 1024)},
    {name = "200k",   iters = 50,   payload = make_payload(200 * 1024)},
    {name = "500k",   iters = 20,   payload = make_payload(500 * 1024)},
    {name = "1m",     iters = 15,   payload = make_payload(1024 * 1024)},
    {name = "2m",     iters = 20,   payload = make_payload(2 * 1024 * 1024)},
    {name = "5m",     iters = 20,   payload = make_payload(5 * 1024 * 1024)},
    {name = "10m",    iters = 20,   payload = make_payload(10 * 1024 * 1024)},
}

-- The pooled API (qjson.new_decoder + :parse) only exists on commits that
-- landed the Decoder refactor. Probe so the bench still runs on older builds.
local has_pooled_api = type(qjson.new_decoder) == "function"
local pooled_decoder = has_pooled_api and qjson.new_decoder() or nil

if not simdjson then
    print("lua-resty-simdjson unavailable; skipping simdjson rows: "
        .. tostring(simdjson_or_err))
end

for _, s in ipairs(scenarios) do
    print(string.format("=== %s (%d bytes) ===", s.name, #s.payload))

    local cjson_access = s.cjson_access or default_cjson_access
    local qjson_access = s.qjson_access or default_qjson_access
    local table_access = s.table_access or default_table_access

    bench("cjson.decode + access fields", s.iters, function()
        local obj = cjson.decode(s.payload)
        cjson_access(obj)
    end)

    if simdjson and not s.no_simdjson then
        bench("simdjson.decode + access fields", s.iters, function()
            local obj = simdjson:decode(s.payload)
            cjson_access(obj)
        end)
    end

    bench("qjson.parse + access fields", s.iters, function()
        local d = qjson.parse(s.payload)
        qjson_access(d)
    end)

    if has_pooled_api then
        bench("qjson pooled :parse + access fields", s.iters, function()
            local d = pooled_decoder:parse(s.payload)
            qjson_access(d)
        end)

        bench("qjson new_decoder()+parse (one-shot)", s.iters, function()
            local dec = qjson.new_decoder()
            local d = dec:parse(s.payload)
            qjson_access(d)
        end)
    end

    bench("qjson.decode + access content", s.iters, function()
        local t = qjson.decode(s.payload)
        table_access(t)
    end)

    bench("qjson.decode + qjson.encode (unmodified)", s.iters, function()
        local t = qjson.decode(s.payload)
        local _ = qjson.encode(t)
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
    bench("cjson.decode + access fields", 400, function()
        local p = next_p()
        local obj = cjson.decode(p)
        default_cjson_access(obj)
    end)

    if simdjson then
        next_p = make_cycler(interleaved)
        bench("simdjson.decode + access fields", 400, function()
            local p = next_p()
            local obj = simdjson:decode(p)
            default_cjson_access(obj)
        end)
    end

    next_p = make_cycler(interleaved)
    bench("qjson.parse + access fields", 400, function()
        local p = next_p()
        local d = qjson.parse(p)
        default_qjson_access(d)
    end)

    if has_pooled_api then
        next_p = make_cycler(interleaved)
        bench("qjson pooled :parse + access fields", 400, function()
            local p = next_p()
            local d = pooled_decoder:parse(p)
            default_qjson_access(d)
        end)
    end

    next_p = make_cycler(interleaved)
    bench("qjson.decode + access content", 400, function()
        local p = next_p()
        local t = qjson.decode(p)
        default_table_access(t)
    end)

    next_p = make_cycler(interleaved)
    bench("qjson.decode + qjson.encode (unmodified)", 400, function()
        local p = next_p()
        local t = qjson.decode(p)
        local _ = qjson.encode(t)
    end)
end
