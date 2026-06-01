package.path  = package.path  .. ";./lua/?.lua;./benches/?.lua"
package.cpath = package.cpath .. ";./target/release/lib?.so"

local qjson    = require("qjson")
local cjson = require("cjson")
local manifest_mod = require("manifest")
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

local ROUNDS = 5

local function bench(name, iters, fn)
    -- Warmup pass: lets JIT compile hot traces and any one-time pools fill
    -- before measurement starts. Excluded from timing and memory delta.
    -- Floor at 50: LuaJIT hotloop default is 56, so fewer iterations leave
    -- the bench measuring interpreter mode for the large-payload scenarios
    -- (e.g. 500k has iters=100, iters/5=20 → without floor, traces may not compile).
    local warmup = math.max(50, math.floor(iters / 5))
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

local function default_table_modify_top(t)
    t.model = "new-model"
    t.temperature = 0.0
end

local function default_table_modify_add(t)
    t.stream = true
end

local function default_table_modify_nested(t)
    if t.messages and qjson.len(t.messages) > 0 then
        t.messages[1].content = "modified"
    end
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

local function github_table_modify_top(t)
    t[1].title = "modified title"
end

local function github_table_modify_add(t)
    if t[1] then
        t[1].extra_field = true
    end
end

local function github_table_modify_nested(t)
    if t[1] and t[1].user then
        t[1].user.login = "modified-user"
    end
end

local function array_len(v)
    if not v then
        return 0
    end
    return qjson.len(v)
end

local function content_metric(v)
    if v == nil then
        return {kind = "nil"}
    end
    if type(v) == "string" then
        return {
            kind = "string",
            len = #v,
            prefix = v:sub(1, 32),
        }
    end
    if type(v) == "table" then
        local n = qjson.len(v)
        local out = {
            kind = "array",
            len = n,
        }
        local first = n >= 1 and v[1] or nil
        local second = n >= 2 and v[2] or nil
        if type(first) == "table" then
            out.first_type = first.type
            out.first_text = first.text
        end
        if type(second) == "table" then
            local image_url = second.image_url
            local url = type(image_url) == "table" and image_url.url or nil
            out.second_type = second.type
            out.second_url_prefix = type(url) == "string" and url:sub(1, 32) or url
            out.second_url_len = type(url) == "string" and #url or nil
        end
        return out
    end
    return {kind = type(v), value = v}
end

local function content_array_metric_from_doc(d, path)
    local n = d:len(path)
    local out = {
        kind = "array",
        len = n,
    }
    if n and n >= 1 then
        out.first_type = d:get_str(path .. "[0].type")
        out.first_text = d:get_str(path .. "[0].text")
    end
    if n and n >= 2 then
        local url_path = path .. "[1].image_url.url"
        out.second_type = d:get_str(path .. "[1].type")
        local url = d:get_str(url_path)
        out.second_url_prefix = type(url) == "string" and url:sub(1, 32) or url
        out.second_url_len = type(url) == "string" and #url or nil
    end
    return out
end

local function doc_content_metric(d, path)
    local ty = d:typeof(path)
    if ty == nil then
        return {kind = "nil"}
    end
    ty = tonumber(ty)
    if ty == qjson.T_STR then
        local v = d:get_str(path)
        return {
            kind = "string",
            len = #v,
            prefix = v:sub(1, 32),
        }
    end
    if ty == qjson.T_ARR then
        return content_array_metric_from_doc(d, path)
    end
    if ty == qjson.T_OBJ then
        return {
            kind = "object",
            len = d:len(path),
        }
    end
    return {kind = tostring(ty)}
end

local function default_table_summary(t)
    local messages = t.messages
    local n = array_len(messages)
    local contents = {}
    for i = 1, n do
        local msg = messages[i]
        contents[i] = content_metric(msg and msg.content)
    end
    return {
        model = t.model,
        temperature = t.temperature,
        message_count = n,
        first_role = (n > 0 and messages[1] and messages[1].role) or nil,
        last_role = (n > 0 and messages[n] and messages[n].role) or nil,
        contents = contents,
    }
end

local function default_doc_summary(d)
    local n = d:len("messages") or 0
    local contents = {}
    for i = 0, n - 1 do
        contents[i + 1] = doc_content_metric(d, "messages[" .. i .. "].content")
    end
    return {
        model = d:get_str("model"),
        temperature = d:get_f64("temperature"),
        message_count = n,
        first_role = (n > 0) and d:get_str("messages[0].role") or nil,
        last_role = (n > 0) and d:get_str("messages[" .. (n - 1) .. "].role") or nil,
        contents = contents,
    }
end

local function github_table_summary(t)
    local first = t[1]
    return {
        first_id = first and first.id or nil,
        first_title = first and first.title or nil,
        first_user_login = first and first.user and first.user.login or nil,
        first_label_count = first and array_len(first.labels) or 0,
    }
end

local function github_doc_summary(d)
    return {
        first_id = d:get_f64("[0].id"),
        first_title = d:get_str("[0].title"),
        first_user_login = d:get_str("[0].user.login"),
        first_label_count = d:len("[0].labels") or 0,
    }
end

local scenarios = {
    {name = "small",  iters = 5000, payload = read_file("benches/fixtures/small_api.json")},
    {name = "medium", iters = 500,  payload = read_file("benches/fixtures/medium_resp.json")},
    {name = "github-100k", iters = 100, payload = make_github_issues_payload(100 * 1024),
     cjson_access = github_cjson_access, qjson_access = github_qjson_access, table_access = github_table_access,
     modify_top = github_table_modify_top, modify_add = github_table_modify_add, modify_nested = github_table_modify_nested,
     table_summary = github_table_summary, doc_summary = github_doc_summary},
    {name = "100k",   iters = 100,  payload = make_payload(100 * 1024)},
    {name = "200k",   iters = 50,   payload = make_payload(200 * 1024)},
    {name = "500k",   iters = 100,  payload = make_payload(500 * 1024)},
    {name = "1m",     iters = 60,   payload = make_payload(1024 * 1024)},
    {name = "2m",     iters = 20,   payload = make_payload(2 * 1024 * 1024)},
    {name = "5m",     iters = 20,   payload = make_payload(5 * 1024 * 1024)},
    {name = "10m",    iters = 20,   payload = make_payload(10 * 1024 * 1024)},
}

-- The pooled API (qjson.new_decoder + :parse) only exists on commits that
-- landed the Decoder refactor. Probe so the bench still runs on older builds.
local has_pooled_api = type(qjson.new_decoder) == "function"
local pooled_decoder = has_pooled_api and qjson.new_decoder() or nil

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

local function parse_cli_args(argv)
    local smoke = false
    local positional = {}
    for i = 1, #argv do
        local a = argv[i]
        if a == "--smoke" then
            smoke = true
        else
            positional[#positional + 1] = a
        end
    end
    if #positional > 1 then
        error("usage: lua_bench.lua [--smoke] [scenario]")
    end
    return smoke, positional[1]
end

local smoke_mode, filter = parse_cli_args(arg)

local function path_child(path, key)
    if type(key) == "number" then
        return path .. "[" .. key .. "]"
    end
    if type(key) == "string" and key:match("^[%a_][%w_]*$") then
        return path .. "." .. key
    end
    return path .. "[" .. string.format("%q", tostring(key)) .. "]"
end

local function value_repr(v)
    if v == cjson.null then
        return "cjson.null"
    end
    local tv = type(v)
    if tv == "nil" or tv == "number" or tv == "boolean" then
        return tostring(v)
    end
    if tv == "string" then
        if #v > 120 then
            return string.format("%q...<%d bytes>", v:sub(1, 120), #v)
        end
        return string.format("%q", v)
    end
    local ok, enc = pcall(cjson.encode, v)
    if ok then
        if #enc > 512 then
            return enc:sub(1, 512) .. "...<" .. #enc .. " bytes>"
        end
        return enc
    end
    return tostring(v)
end

local function fail_smoke(scenario, workload, path, expected, actual)
    error(string.format(
        "[smoke][%s][%s] mismatch at %s: expected=%s actual=%s",
        scenario, workload, path, value_repr(expected), value_repr(actual)), 0)
end

local qjson_type_names = {
    [qjson.T_NULL] = "null",
    [qjson.T_BOOL] = "bool",
    [qjson.T_NUM]  = "number",
    [qjson.T_STR]  = "string",
    [qjson.T_ARR]  = "array",
    [qjson.T_OBJ]  = "object",
}

local function type_name(ty)
    if ty == nil then
        return "nil"
    end
    return qjson_type_names[tonumber(ty)] or tostring(ty)
end

local function semantic_equal(expected, actual, path)
    if expected == actual then
        return true
    end
    if expected == cjson.null or actual == cjson.null then
        return false, path, expected, actual
    end
    local te = type(expected)
    local ta = type(actual)
    if te ~= ta then
        return false, path, expected, actual
    end
    if te ~= "table" then
        return false, path, expected, actual
    end

    for k, ev in pairs(expected) do
        if actual[k] == nil then
            return false, path_child(path, k), ev, nil
        end
        local ok, bad_path, bad_expected, bad_actual =
            semantic_equal(ev, actual[k], path_child(path, k))
        if not ok then
            return false, bad_path, bad_expected, bad_actual
        end
    end

    for k, av in pairs(actual) do
        if expected[k] == nil then
            return false, path_child(path, k), nil, av
        end
    end
    return true
end

local function assert_semantic_equal(scenario, workload, expected, actual)
    local ok, bad_path, bad_expected, bad_actual = semantic_equal(expected, actual, "$")
    if not ok then
        fail_smoke(scenario, workload, bad_path, bad_expected, bad_actual)
    end
end

local function path_for_child(parent, key, array_parent)
    if array_parent then
        return parent .. "[" .. (key - 1) .. "]"
    end
    if type(key) ~= "string" or key:find("[%.%[%]]") then
        return nil
    end
    if parent == "" then
        return key
    end
    return parent .. "." .. key
end

local function collect_empty_container_shapes(v, path, out)
    local mt = getmetatable(v)
    if mt == qjson._LazyArray then
        local n = qjson.len(v)
        if n == 0 then
            out[#out + 1] = {path = path, kind = qjson.T_ARR}
            return
        end
        for i, child in qjson.ipairs(v) do
            collect_empty_container_shapes(child, path_for_child(path, i, true), out)
        end
    elseif mt == qjson._LazyObject then
        local n = qjson.len(v)
        if n == 0 then
            out[#out + 1] = {path = path, kind = qjson.T_OBJ}
            return
        end
        for k, child in qjson.pairs(v) do
            local child_path = path_for_child(path, k, false)
            if child_path then
                collect_empty_container_shapes(child, child_path, out)
            end
        end
    end
end

local function empty_container_shapes(json)
    local shapes = {}
    collect_empty_container_shapes(qjson.decode(json), "", shapes)
    return shapes
end

local function assert_empty_container_shapes(scenario, workload, expected_json, actual_json)
    local shapes = empty_container_shapes(expected_json)
    if #shapes == 0 then
        return
    end

    local actual_doc = qjson.parse(actual_json)
    for _, shape in ipairs(shapes) do
        local actual_type = actual_doc:typeof(shape.path)
        if tonumber(actual_type) ~= shape.kind then
            fail_smoke(scenario, workload, shape.path,
                type_name(shape.kind), type_name(actual_type))
        end
        local actual_len = actual_doc:len(shape.path)
        if actual_len ~= 0 then
            fail_smoke(scenario, workload, shape.path .. " length", 0, actual_len)
        end
    end
end

local function smoke_mutation_roundtrip(payload, mutator)
    local t = qjson.decode(payload)
    mutator(t)
    local encoded = qjson.encode(t)
    return cjson.decode(encoded), encoded
end

local function run_scenario_smoke(s)
    local cjson_access = s.cjson_access or default_cjson_access
    local qjson_access = s.qjson_access or default_qjson_access
    local table_access = s.table_access or default_table_access
    local modify_top = s.modify_top or default_table_modify_top
    local modify_add = s.modify_add or default_table_modify_add
    local modify_nested = s.modify_nested or default_table_modify_nested
    local table_summary = s.table_summary or default_table_summary
    local doc_summary = s.doc_summary or default_doc_summary

    local baseline = cjson.decode(s.payload)
    cjson_access(baseline)
    local expected_summary = table_summary(baseline)

    local parsed = qjson.parse(s.payload)
    qjson_access(parsed)
    assert_semantic_equal(s.name, "qjson.parse + access fields",
        expected_summary, doc_summary(parsed))

    if has_pooled_api then
        local pooled = pooled_decoder:parse(s.payload)
        qjson_access(pooled)
        assert_semantic_equal(s.name, "qjson pooled :parse + access fields",
            expected_summary, doc_summary(pooled))

        local dec = qjson.new_decoder()
        local one_shot = dec:parse(s.payload)
        qjson_access(one_shot)
        assert_semantic_equal(s.name, "qjson new_decoder()+parse (one-shot)",
            expected_summary, doc_summary(one_shot))
    end

    local lazy = qjson.decode(s.payload)
    table_access(lazy)
    assert_semantic_equal(s.name, "qjson.decode + access content",
        expected_summary, table_summary(lazy))

    if simdjson then
        local simd = simdjson:decode(s.payload)
        cjson_access(simd)
        assert_semantic_equal(s.name, "simdjson.decode + access fields",
            expected_summary, table_summary(simd))
    end

    local unmodified = qjson.encode(qjson.decode(s.payload))
    local unmodified_decoded = cjson.decode(unmodified)
    assert_semantic_equal(s.name, "qjson.decode + qjson.encode (unmodified)",
        baseline, unmodified_decoded)
    assert_empty_container_shapes(s.name, "qjson.decode + qjson.encode (unmodified)",
        s.payload, unmodified)

    local expect_top = cjson.decode(s.payload)
    modify_top(expect_top)
    local actual_top, encoded_top = smoke_mutation_roundtrip(s.payload, modify_top)
    local modify_top_workload = "qjson.decode + modify top + encode"
    assert_semantic_equal(s.name, modify_top_workload, expect_top, actual_top)
    assert_empty_container_shapes(s.name, modify_top_workload, s.payload, encoded_top)

    local expect_add = cjson.decode(s.payload)
    modify_add(expect_add)
    local actual_add, encoded_add = smoke_mutation_roundtrip(s.payload, modify_add)
    local modify_add_workload = "qjson.decode + add field + encode"
    assert_semantic_equal(s.name, modify_add_workload, expect_add, actual_add)
    assert_empty_container_shapes(s.name, modify_add_workload, s.payload, encoded_add)

    local expect_nested = cjson.decode(s.payload)
    modify_nested(expect_nested)
    local actual_nested, encoded_nested = smoke_mutation_roundtrip(s.payload, modify_nested)
    local modify_nested_workload = "qjson.decode + modify nested + encode"
    assert_semantic_equal(s.name, modify_nested_workload, expect_nested, actual_nested)
    assert_empty_container_shapes(s.name, modify_nested_workload, s.payload, encoded_nested)
end

local function run_interleaved_smoke()
    local next_p = make_cycler(interleaved)
    for i = 1, #interleaved do
        local workload = "interleaved cycle #" .. i
        local payload = next_p()
        local baseline = cjson.decode(payload)
        default_cjson_access(baseline)
        local expected_summary = default_table_summary(baseline)

        local parsed = qjson.parse(payload)
        default_qjson_access(parsed)
        assert_semantic_equal("interleaved", workload .. " qjson.parse + access fields",
            expected_summary, default_doc_summary(parsed))

        local lazy = qjson.decode(payload)
        default_table_access(lazy)
        assert_semantic_equal("interleaved", workload .. " qjson.decode + access content",
            expected_summary, default_table_summary(lazy))

        local unmodified = qjson.encode(qjson.decode(payload))
        local encode_workload = workload .. " qjson.decode + qjson.encode (unmodified)"
        assert_semantic_equal("interleaved", encode_workload, baseline, cjson.decode(unmodified))
        assert_empty_container_shapes("interleaved", encode_workload, payload, unmodified)

        if simdjson then
            local simd = simdjson:decode(payload)
            default_cjson_access(simd)
            assert_semantic_equal("interleaved", workload .. " simdjson.decode + access fields",
                expected_summary, default_table_summary(simd))
        end
    end
end

local smoke_default_scenarios = {"small", "medium", "github-100k", "100k"}

local function run_smoke()
    if filter == "interleaved" then
        run_interleaved_smoke()
        return
    end

    if filter then
        run_scenario_smoke(scenario_by_name(filter))
        return
    end

    for _, name in ipairs(smoke_default_scenarios) do
        run_scenario_smoke(scenario_by_name(name))
    end
    run_interleaved_smoke()
end

local function run_benchmarks()
    if not simdjson then
        print("lua-resty-simdjson unavailable; skipping simdjson rows: "
            .. tostring(simdjson_or_err))
    end

    for _, s in ipairs(scenarios) do
        if filter and s.name ~= filter then goto continue_scenario end
        print(string.format("=== %s (%d bytes) ===", s.name, #s.payload))

        local cjson_access = s.cjson_access or default_cjson_access
        local qjson_access = s.qjson_access or default_qjson_access
        local table_access = s.table_access or default_table_access
        local modify_top = s.modify_top or default_table_modify_top
        local modify_add = s.modify_add or default_table_modify_add
        local modify_nested = s.modify_nested or default_table_modify_nested

        bench("cjson.decode + access fields", s.iters, function()
            local obj = cjson.decode(s.payload)
            cjson_access(obj)
        end)

        -- cjson always fully materializes on decode, so modify+encode is the
        -- same cost as a full re-encode — useful as a realistic baseline for
        -- modify workloads.
        bench("cjson.decode + modify top + encode", s.iters, function()
            local obj = cjson.decode(s.payload)
            modify_top(obj)
            local _enc = cjson.encode(obj)
            if #_enc < 2 then error("cjson.encode produced too-short result") end
        end)

        bench("cjson.decode + add field + encode", s.iters, function()
            local obj = cjson.decode(s.payload)
            modify_add(obj)
            local _enc = cjson.encode(obj)
            if #_enc < 2 then error("cjson.encode produced too-short result") end
        end)

        bench("cjson.decode + modify nested + encode", s.iters, function()
            local obj = cjson.decode(s.payload)
            modify_nested(obj)
            local _enc = cjson.encode(obj)
            if #_enc < 2 then error("cjson.encode produced too-short result") end
        end)

        if simdjson then
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
            local _enc = qjson.encode(t)
            if #_enc < 2 then error("qjson.encode produced too-short result") end
        end)

        bench("qjson.decode + modify top + encode", s.iters, function()
            local t = qjson.decode(s.payload)
            modify_top(t)
            local _enc = qjson.encode(t)
            if #_enc < 2 then error("qjson.encode produced too-short result") end
        end)

        bench("qjson.decode + add field + encode", s.iters, function()
            local t = qjson.decode(s.payload)
            modify_add(t)
            local _enc = qjson.encode(t)
            if #_enc < 2 then error("qjson.encode produced too-short result") end
        end)

        bench("qjson.decode + modify nested + encode", s.iters, function()
            local t = qjson.decode(s.payload)
            modify_nested(t)
            local _enc = qjson.encode(t)
            if #_enc < 2 then error("qjson.encode produced too-short result") end
        end)
        ::continue_scenario::
    end

    if not filter or filter == "interleaved" then
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
                local _enc = qjson.encode(t)
                if #_enc < 2 then error("qjson.encode produced too-short result") end
            end)

            next_p = make_cycler(interleaved)
            bench("qjson.decode + modify top + encode", 400, function()
                local p = next_p()
                local t = qjson.decode(p)
                default_table_modify_top(t)
                local _enc = qjson.encode(t)
                if #_enc < 2 then error("qjson.encode produced too-short result") end
            end)

            next_p = make_cycler(interleaved)
            bench("qjson.decode + add field + encode", 400, function()
                local p = next_p()
                local t = qjson.decode(p)
                default_table_modify_add(t)
                local _enc = qjson.encode(t)
                if #_enc < 2 then error("qjson.encode produced too-short result") end
            end)

            next_p = make_cycler(interleaved)
            bench("qjson.decode + modify nested + encode", 400, function()
                local p = next_p()
                local t = qjson.decode(p)
                default_table_modify_nested(t)
                local _enc = qjson.encode(t)
                if #_enc < 2 then error("qjson.encode produced too-short result") end
            end)
        end
    end  -- filter == "interleaved"

    -- Manifest-driven scenarios: reuse the real-world fixture manifest shared
    -- with the Rust correctness gate (tests/fixtures/manifest.json). Fixture
    -- paths, per-fixture iteration counts and access paths all come from the
    -- manifest, so the benchmark and the correctness tests cover the same
    -- payloads and paths. The access closure is generated from each fixture's
    -- declared checks rather than being hand-written here. NDJSON fixtures are
    -- correctness-only (validated by the Rust gate) and skipped here.
    if not filter or filter == "manifest" then
        print("=== manifest fixtures ===")

        local manifest = manifest_mod.load()
        for _, f in ipairs(manifest.fixtures) do
            if manifest_mod.has_ci(f, "bench") and f.format == "json" then
                local payload = read_file(f.path)
                local access = manifest_mod.qjson_access(f.checks)
                local iters = f.bench_iters or 500
                print(string.format("--- %s [%s] (%d bytes) ---",
                    f.id, f.payload_type, #payload))

                bench("qjson.parse + manifest access", iters, function()
                    local d = qjson.parse(payload)
                    access(d)
                end)

                if has_pooled_api then
                    bench("qjson pooled :parse + manifest access", iters, function()
                        local d = pooled_decoder:parse(payload)
                        access(d)
                    end)
                end
            end
        end
    end  -- filter == "manifest"
end

if smoke_mode then
    run_smoke()
else
    run_benchmarks()
end
