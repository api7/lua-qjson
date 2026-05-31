local qjson = require("qjson")
local cjson = require("cjson")
local prop = require("tests.lua.property_json")

local function positive_int_env(name, default)
    local value = os.getenv(name)
    if value == nil then
        return default
    end
    if not value:match("^[1-9]%d*$") then
        error(name .. " must be a positive integer, got " .. string.format("%q", value))
    end
    return tonumber(value)
end

local CASES = positive_int_env("QJSON_MUT_PROP_CASES", 40)
local SEED  = positive_int_env("QJSON_MUT_PROP_SEED", 104104)
local STEPS = positive_int_env("QJSON_MUT_PROP_STEPS", 24)

-- Supported lazy proxy API under test:
--   t.k reads/writes/deletes, t[i] reads/writes/appends, qjson.pairs,
--   qjson.ipairs, qjson.len, qjson.encode, and qjson.materialize.
--
-- Explicitly out of scope because they bypass the proxy contract:
--   rawset, rawget, next, and direct metatable replacement/mutation.

local ABSENT = {}
local DELETE = {}

local function cjson_decode_with_array_mt(json)
    if not cjson.decode_array_with_array_mt then
        return cjson.decode(json)
    end

    local old = cjson.decode_array_with_array_mt()
    cjson.decode_array_with_array_mt(true)
    local ok, value = pcall(cjson.decode, json)
    cjson.decode_array_with_array_mt(old)
    if not ok then
        error(value)
    end
    return value
end

local function model_null()
    return { kind = "null" }
end

local function model_object(entries)
    return { kind = "object", entries = entries or {} }
end

local function model_array(items)
    return { kind = "array", items = items or {} }
end

local function entry(key, value)
    return { key = key, value = value }
end

local function is_model_node(v, kind)
    return type(v) == "table" and v.kind == kind
end

local function clone_model(v, seen)
    if type(v) ~= "table" or not v.kind then
        return v
    end
    seen = seen or {}
    if seen[v] then
        return seen[v]
    end
    if v.kind == "null" then
        return model_null()
    elseif v.kind == "array" then
        local out = model_array({})
        seen[v] = out
        for i, item in ipairs(v.items) do
            out.items[i] = clone_model(item, seen)
        end
        return out
    elseif v.kind == "object" then
        local out = model_object({})
        seen[v] = out
        for i, item in ipairs(v.entries) do
            out.entries[i] = entry(item.key, clone_model(item.value, seen))
        end
        return out
    end
    error("unknown model kind: " .. tostring(v.kind))
end

local function find_entry_index(obj, key)
    for i, item in ipairs(obj.entries) do
        if item.key == key then
            return i
        end
    end
    return nil
end

local function model_get(obj, key)
    local idx = find_entry_index(obj, key)
    if not idx then
        return ABSENT
    end
    return obj.entries[idx].value
end

local function model_set(obj, key, value)
    local idx = find_entry_index(obj, key)
    if value == DELETE then
        if idx then
            table.remove(obj.entries, idx)
        end
        return
    end
    if idx then
        obj.entries[idx].value = value
    else
        obj.entries[#obj.entries + 1] = entry(key, value)
    end
end

local function encode_string(s)
    local out = { '"' }
    for i = 1, #s do
        local b = string.byte(s, i)
        if b == 0x22 then
            out[#out + 1] = '\\"'
        elseif b == 0x5C then
            out[#out + 1] = "\\\\"
        elseif b == 0x0A then
            out[#out + 1] = "\\n"
        elseif b == 0x0D then
            out[#out + 1] = "\\r"
        elseif b == 0x09 then
            out[#out + 1] = "\\t"
        elseif b == 0x08 then
            out[#out + 1] = "\\b"
        elseif b == 0x0C then
            out[#out + 1] = "\\f"
        elseif b < 0x20 then
            out[#out + 1] = string.format("\\u%04x", b)
        else
            out[#out + 1] = string.char(b)
        end
    end
    out[#out + 1] = '"'
    return table.concat(out)
end

local function encode_number(n)
    if n == math.floor(n) and math.abs(n) < 1e15 then
        return string.format("%d", n)
    end
    return string.format("%.14g", n)
end

local function model_encode(v, seen)
    if type(v) ~= "table" or not v.kind then
        local tv = type(v)
        if tv == "string" then
            return encode_string(v)
        elseif tv == "number" then
            return encode_number(v)
        elseif tv == "boolean" then
            return v and "true" or "false"
        end
        error("unsupported scalar model value: " .. tv)
    end

    if v.kind == "null" then
        return "null"
    end

    seen = seen or {}
    if seen[v] then
        error("model_encode: cycle in oracle model")
    end
    seen[v] = true

    if v.kind == "array" then
        local parts = {}
        for i, item in ipairs(v.items) do
            parts[i] = model_encode(item, seen)
        end
        seen[v] = nil
        return "[" .. table.concat(parts, ",") .. "]"
    end

    if v.kind == "object" then
        local parts = {}
        for i, item in ipairs(v.entries) do
            parts[i] = encode_string(item.key) .. ":" .. model_encode(item.value, seen)
        end
        seen[v] = nil
        return "{" .. table.concat(parts, ",") .. "}"
    end

    error("unknown model kind: " .. tostring(v.kind))
end

local function model_to_lua(v, seen)
    if type(v) ~= "table" or not v.kind then
        return v
    end
    if v.kind == "null" then
        return qjson.null
    end

    seen = seen or {}
    if seen[v] then
        return seen[v]
    end

    if v.kind == "array" then
        local out = {}
        seen[v] = out
        for i, item in ipairs(v.items) do
            out[i] = model_to_lua(item, seen)
        end
        if #out == 0 then
            setmetatable(out, qjson.empty_array_mt)
        end
        return out
    end

    if v.kind == "object" then
        local out = {}
        seen[v] = out
        for _, item in ipairs(v.entries) do
            out[item.key] = model_to_lua(item.value, seen)
        end
        return out
    end

    error("unknown model kind: " .. tostring(v.kind))
end

local function model_to_assignable(v)
    if is_model_node(v, "object") or is_model_node(v, "array") then
        return qjson.decode(model_encode(v))
    end
    return model_to_lua(v)
end

local function is_json_null(v)
    return rawequal(v, qjson.null) or rawequal(v, cjson.null)
end

local function numbers_equal(a, b)
    if a == b then
        return true
    end
    local scale = math.max(1, math.abs(a), math.abs(b))
    return math.abs(a - b) <= scale * 1e-12
end

-- semantic_equal(model, value) is intentionally stricter than a plain table
-- deep-equal but less strict than byte-equality:
--   * qjson.null and cjson.null are equivalent JSON null sentinels.
--   * Empty arrays must carry an array metatable, so [] and {} stay distinct.
--   * Numbers compare by numeric value with a small floating tolerance.
--   * Object key order is part of the ordered model and is asserted through
--     model_encode(model) == qjson.encode(lazy). Plain Lua materialized objects
--     cannot preserve order, so this function checks their structure only.
local function semantic_equal(model, value, seen)
    if is_model_node(model, "null") then
        return is_json_null(value)
    end

    local mt = type(model) == "table" and model.kind or nil
    if mt == "array" then
        if type(value) ~= "table" then
            return false
        end
        if #model.items == 0 and getmetatable(value) == nil then
            return false
        end
        if #value ~= #model.items then
            return false
        end
        seen = seen or {}
        if seen[model] == value then
            return true
        end
        seen[model] = value
        for i, item in ipairs(model.items) do
            if not semantic_equal(item, value[i], seen) then
                return false
            end
        end
        return true
    elseif mt == "object" then
        if type(value) ~= "table" then
            return false
        end
        seen = seen or {}
        if seen[model] == value then
            return true
        end
        seen[model] = value

        local expected = {}
        for _, item in ipairs(model.entries) do
            expected[item.key] = item.value
        end

        local count = 0
        for k in pairs(value) do
            count = count + 1
            if expected[k] == nil then
                return false
            end
        end
        if count ~= #model.entries then
            return false
        end

        for _, item in ipairs(model.entries) do
            if not semantic_equal(item.value, value[item.key], seen) then
                return false
            end
        end
        return true
    end

    if type(model) == "number" and type(value) == "number" then
        return numbers_equal(model, value)
    end
    return model == value
end

local function trace_text(trace, upto)
    local parts = {}
    upto = upto or #trace
    for i = 1, upto do
        parts[#parts + 1] = string.format("%02d %s", i, trace[i])
    end
    return table.concat(parts, "\n")
end

local function fail_message(ctx, checkpoint, trace, extra)
    return table.concat({
        "checkpoint=" .. checkpoint,
        "case=" .. ctx.case_no,
        "seed=" .. ctx.seed,
        "source=" .. ctx.source,
        "trace:\n" .. trace_text(trace),
        extra or "",
    }, "\n")
end

local function model_debug_json(value)
    local ok, encoded = pcall(model_encode, value)
    if ok then
        return encoded
    end
    return "<model_encode_error=" .. tostring(encoded) .. ">"
end

local function actual_debug_json(value)
    if value == nil then
        return "nil"
    end

    local debug_value = value
    local note = nil
    if type(value) == "table" then
        local mt = getmetatable(value)
        if mt == qjson._LazyObject or mt == qjson._LazyArray then
            local ok, materialized = pcall(qjson.materialize, value)
            if ok then
                debug_value = materialized
            else
                note = "materialize_error=" .. tostring(materialized)
            end
        end
    end

    local ok, encoded = pcall(qjson.encode, debug_value)
    if ok then
        if note then
            return encoded .. "\n" .. note
        end
        return encoded
    end

    if is_json_null(debug_value) then
        return "null"
    elseif type(debug_value) == "string" then
        return encode_string(debug_value)
    elseif type(debug_value) == "number" then
        return encode_number(debug_value)
    elseif type(debug_value) == "boolean" then
        return debug_value and "true" or "false"
    end
    return tostring(debug_value) .. " (encode_error=" .. tostring(encoded) .. ")"
end

local function assert_model_value(ctx, label, expected, actual, trace)
    local value = actual
    if type(actual) == "table" then
        local mt = getmetatable(actual)
        if mt == qjson._LazyObject or mt == qjson._LazyArray then
            value = qjson.materialize(actual)
        end
    end
    assert.is_true(
        semantic_equal(expected, value),
        fail_message(
            ctx,
            label,
            trace,
            "expected=" .. model_debug_json(expected) .. "\nactual=" .. actual_debug_json(actual)
        )
    )
end

local function assert_checkpoint(ctx, name, lazy, model, trace)
    local materialized = qjson.materialize(lazy)
    local expected_json = model_encode(model)
    assert.is_true(
        semantic_equal(model, materialized),
        fail_message(
            ctx,
            name,
            trace,
            "expected=" .. expected_json .. "\nactual_materialized=" .. actual_debug_json(materialized)
        )
    )

    local encoded = qjson.encode(lazy)
    assert.is_true(
        encoded == expected_json,
        fail_message(ctx, name, trace, "expected=" .. expected_json .. "\nactual_encoded=" .. encoded)
    )

    local cjson_value = cjson_decode_with_array_mt(encoded)
    assert.is_true(
        semantic_equal(model, cjson_value),
        fail_message(
            ctx,
            name,
            trace,
            "expected=" .. expected_json .. "\nactual_cjson_roundtrip=" .. actual_debug_json(cjson_value)
        )
    )

    local reparsed = qjson.materialize(qjson.decode(encoded))
    assert.is_true(
        semantic_equal(model, reparsed),
        fail_message(
            ctx,
            name,
            trace,
            "expected=" .. expected_json .. "\nactual_qjson_roundtrip=" .. actual_debug_json(reparsed)
        )
    )
end

local function random_string(rng)
    local choices = {
        "alpha",
        "line\nbreak",
        "quote\"value",
        "slash\\value",
        "tab\tvalue",
        "_keys",
        "_values",
    }
    return choices[rng:int(#choices)] .. tostring(rng:int(9))
end

local function random_scalar(rng)
    local choice = rng:int(6)
    if choice == 1 then
        return model_null()
    elseif choice == 2 then
        return rng:bool()
    elseif choice == 3 then
        return rng:int(200) - 100
    elseif choice == 4 then
        return (rng:int(200) - 100) / 4
    end
    return random_string(rng)
end

local function random_small_object(rng)
    return model_object({
        entry("v", random_scalar(rng)),
    })
end

local function random_small_array(rng)
    local n = rng:int(3) - 1
    local items = {}
    for i = 1, n do
        items[i] = random_scalar(rng)
    end
    return model_array(items)
end

local function initial_model(rng)
    return model_object({
        entry("a", model_object({
            entry("x", rng:int(20)),
            entry("y", random_string(rng)),
        })),
        entry("arr", model_array({
            rng:int(10),
            rng:int(10),
            rng:int(10),
        })),
        entry("flag", rng:bool()),
        entry("txt", random_string(rng)),
    })
end

local function append_trace(trace, text)
    trace[#trace + 1] = text
end

local function op_read_field(ctx)
    local expected = model_get(ctx.model, "txt")
    if expected == ABSENT then
        append_trace(ctx.trace, "read txt (missing)")
        assert.is_nil(ctx.lazy.txt)
        return
    end
    append_trace(ctx.trace, "read txt")
    assert_model_value(ctx, "read txt", expected, ctx.lazy.txt, ctx.trace)
end

local function op_read_nested(ctx)
    local a = model_get(ctx.model, "a")
    if not is_model_node(a, "object") then
        append_trace(ctx.trace, "read a.x skipped")
        return
    end
    append_trace(ctx.trace, "read a.x")
    local expected = model_get(a, "x")
    if expected == ABSENT then
        assert.is_nil(ctx.lazy.a.x)
        return
    end
    assert_model_value(ctx, "read a.x", expected, ctx.lazy.a.x, ctx.trace)
end

local function op_traverse_object(ctx)
    append_trace(ctx.trace, "traverse root with qjson.pairs")
    local i = 0
    for k, v in qjson.pairs(ctx.lazy) do
        i = i + 1
        local expected = ctx.model.entries[i]
        assert.is_truthy(expected, fail_message(ctx, "pairs root", ctx.trace, "too many keys"))
        assert.are.equal(expected.key, k)
        assert_model_value(ctx, "pairs root value", expected.value, v, ctx.trace)
    end
    assert.are.equal(#ctx.model.entries, i)
end

local function op_traverse_array(ctx)
    local arr = model_get(ctx.model, "arr")
    if not is_model_node(arr, "array") then
        append_trace(ctx.trace, "traverse arr skipped")
        return
    end
    append_trace(ctx.trace, "traverse arr with qjson.ipairs")
    local lazy_arr = ctx.lazy.arr
    local i = 0
    for idx, value in qjson.ipairs(lazy_arr) do
        i = i + 1
        assert.are.equal(i, idx)
        assert_model_value(ctx, "ipairs arr value", arr.items[i], value, ctx.trace)
    end
    assert.are.equal(#arr.items, i)
end

local function op_set_top_scalar(ctx)
    local key = "p" .. tostring(ctx.rng:int(4))
    local value = random_scalar(ctx.rng)
    append_trace(ctx.trace, "set " .. key .. " = " .. model_encode(value))
    ctx.lazy[key] = model_to_assignable(value)
    model_set(ctx.model, key, value)
end

local function op_mutate_nested(ctx)
    local a = model_get(ctx.model, "a")
    if not is_model_node(a, "object") then
        append_trace(ctx.trace, "mutate a.x skipped")
        return
    end
    local value = ctx.rng:int(2000) - 1000
    append_trace(ctx.trace, "mutate a.x = " .. tostring(value))
    ctx.lazy.a.x = value
    model_set(a, "x", value)
end

local function op_delete_and_reinsert(ctx)
    local key = (ctx.rng:bool() and "flag") or "txt"
    local value = random_scalar(ctx.rng)
    append_trace(ctx.trace, "delete and reinsert " .. key)
    ctx.lazy[key] = nil
    model_set(ctx.model, key, DELETE)
    ctx.lazy[key] = model_to_assignable(value)
    model_set(ctx.model, key, value)
end

local function op_delete_key(ctx)
    local key = (ctx.rng:bool() and "flag") or "txt"
    append_trace(ctx.trace, "delete " .. key)
    ctx.lazy[key] = nil
    model_set(ctx.model, key, DELETE)
end

local function op_replace_container(ctx)
    local value = random_scalar(ctx.rng)
    append_trace(ctx.trace, "replace a = " .. model_encode(value))
    ctx.lazy.a = model_to_assignable(value)
    model_set(ctx.model, "a", value)
end

local function op_restore_container(ctx)
    local value = random_small_object(ctx.rng)
    append_trace(ctx.trace, "restore a = " .. model_encode(value))
    ctx.lazy.a = model_to_assignable(value)
    model_set(ctx.model, "a", value)
end

local function op_replace_scalar_with_object(ctx)
    local value = random_small_object(ctx.rng)
    append_trace(ctx.trace, "replace txt = " .. model_encode(value))
    ctx.lazy.txt = model_to_assignable(value)
    model_set(ctx.model, "txt", value)
end

local function op_append_array(ctx)
    local arr = model_get(ctx.model, "arr")
    if not is_model_node(arr, "array") then
        append_trace(ctx.trace, "append arr skipped")
        return
    end
    local value = random_scalar(ctx.rng)
    append_trace(ctx.trace, "append arr[] = " .. model_encode(value))
    local lazy_arr = ctx.lazy.arr
    lazy_arr[qjson.len(lazy_arr) + 1] = model_to_assignable(value)
    arr.items[#arr.items + 1] = value
end

local function op_mutate_array(ctx)
    local arr = model_get(ctx.model, "arr")
    if not is_model_node(arr, "array") or #arr.items == 0 then
        append_trace(ctx.trace, "mutate arr[1] skipped")
        return
    end
    local value = random_scalar(ctx.rng)
    append_trace(ctx.trace, "mutate arr[1] = " .. model_encode(value))
    ctx.lazy.arr[1] = model_to_assignable(value)
    arr.items[1] = value
end

local function op_replace_array(ctx)
    local value = random_small_array(ctx.rng)
    append_trace(ctx.trace, "replace arr = " .. model_encode(value))
    ctx.lazy.arr = model_to_assignable(value)
    model_set(ctx.model, "arr", value)
end

local function op_self_assign(ctx)
    local a = model_get(ctx.model, "a")
    if a == ABSENT then
        append_trace(ctx.trace, "self-assign a skipped")
        return
    end
    append_trace(ctx.trace, "self-assign a = a")
    ctx.lazy.a = ctx.lazy.a
end

local function op_alias_child(ctx)
    local a = model_get(ctx.model, "a")
    if not is_model_node(a, "object") then
        append_trace(ctx.trace, "alias a skipped")
        return
    end
    local value = ctx.rng:int(5000)
    append_trace(ctx.trace, "alias alias = a; alias.x = " .. tostring(value))
    ctx.lazy.alias = ctx.lazy.a
    model_set(ctx.model, "alias", a)
    ctx.lazy.alias.x = value
    model_set(a, "x", value)
end

local function op_encode_midsequence(ctx)
    append_trace(ctx.trace, "encode mid-sequence")
    assert_checkpoint(ctx, "mid-sequence encode", ctx.lazy, ctx.model, ctx.trace)
end

local function op_materialize_midsequence(ctx)
    append_trace(ctx.trace, "materialize mid-sequence")
    local materialized = qjson.materialize(ctx.lazy)
    assert.is_true(
        semantic_equal(ctx.model, materialized),
        fail_message(ctx, "mid-sequence materialize", ctx.trace, "materialized value mismatch")
    )
end

local OPS = {
    op_read_field,
    op_read_nested,
    op_traverse_object,
    op_traverse_array,
    op_set_top_scalar,
    op_mutate_nested,
    op_delete_and_reinsert,
    op_delete_key,
    op_replace_container,
    op_restore_container,
    op_replace_scalar_with_object,
    op_append_array,
    op_mutate_array,
    op_replace_array,
    op_self_assign,
    op_alias_child,
    op_encode_midsequence,
    op_materialize_midsequence,
}

local function execute_case(seed, case_no, steps)
    local rng = prop.rng(seed + case_no * 7919)
    local model = initial_model(rng)
    local source = model_encode(model)
    local ctx = {
        seed = seed,
        case_no = case_no,
        source = source,
        rng = rng,
        model = clone_model(model),
        lazy = qjson.decode(source),
        trace = {},
    }

    assert_checkpoint(ctx, "initial", ctx.lazy, ctx.model, ctx.trace)
    for _ = 1, steps do
        OPS[rng:int(#OPS)](ctx)
        assert_checkpoint(ctx, "after step", ctx.lazy, ctx.model, ctx.trace)
    end
end

local function shortest_failing_prefix(seed, case_no, steps)
    for n = 1, steps do
        local ok, err = pcall(execute_case, seed, case_no, n)
        if not ok then
            return n, err
        end
    end
    return nil
end

describe("qjson lazy mutation stateful property coverage", function()
    it("keeps encode/materialize semantics aligned with an ordered model", function()
        for case_no = 1, CASES do
            local ok, err = pcall(execute_case, SEED, case_no, STEPS)
            if not ok then
                local prefix, prefix_err = shortest_failing_prefix(SEED, case_no, STEPS)
                local shrink = prefix and (
                    "shortest_failing_prefix=" .. prefix .. "\nminimized_error:\n" .. tostring(prefix_err)
                )
                    or "shortest_failing_prefix=not prefix-reproducible"
                error(tostring(err) .. "\n" .. shrink, 0)
            end
        end
    end)
end)

describe("qjson lazy mutation deterministic regressions", function()
    it("re-emits unmodified proxy bytes through the fast path", function()
        local src = '{"z":3,"a":{"x":1},"a":{"x":2}}'
        local t = qjson.decode(src)
        assert.are.equal(src, qjson.encode(t))
    end)

    it("mutates an old child proxy after parent materialization", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        local old = t.a
        t.c = 3
        old.x = 9
        assert.are.equal('{"a":{"x":9},"b":2,"c":3}', qjson.encode(t))
    end)

    it("marks ancestors dirty when a nested child changes", function()
        local t = qjson.decode('{"a":{"b":{"c":1}},"d":2}')
        t.a.b.c = 7
        assert.are.equal('{"a":{"b":{"c":7}},"d":2}', qjson.encode(t))
    end)

    it("handles duplicate keys with fast-path preservation and mutated last-wins collapse", function()
        local src = '{"a":1,"a":2}'
        assert.are.equal(src, qjson.encode(qjson.decode(src)))

        local t = qjson.decode(src)
        t.b = 3
        assert.are.equal('{"a":2,"b":3}', qjson.encode(t))
    end)

    it("ignores earlier duplicate container mutations under last-wins materialization", function()
        local t = qjson.decode('{"a":{"x":1},"a":{"y":2}}')
        for _, v in qjson.pairs(t) do
            if v.x then
                v.x = 99
            end
        end
        assert.are.equal('{"a":{"y":2}}', qjson.encode(t))
    end)

    it("handles escaped object keys and escaped string values after mutation", function()
        local t = qjson.decode('{"a\\nb":"x\\t\\"y","holder":{"k":"v"}}')
        t.extra = true
        assert.are.equal('{"a\\nb":"x\\t\\"y","holder":{"k":"v"},"extra":true}', qjson.encode(t))
    end)

    it("preserves nulls, empty arrays, and internal-name-like user keys", function()
        local t = qjson.decode('{"_keys":[],"_values":{"x":null},"n":null}')
        t.added = qjson.null
        local m = qjson.materialize(t)
        assert.are.equal(qjson.empty_array_mt, getmetatable(m._keys))
        assert.are.equal(qjson.null, m._values.x)
        assert.are.equal(qjson.null, m.n)
        assert.are.equal(qjson.null, m.added)
        assert.are.equal('{"_keys":[],"_values":{"x":null},"n":null,"added":null}', qjson.encode(t))
    end)

    it("defines aliasing as shared lazy proxy identity", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        local a = t.a
        t.alias = a
        t.alias.x = 9
        assert.are.equal(t.a, t.alias)
        assert.are.equal('{"a":{"x":9},"b":2,"alias":{"x":9}}', qjson.encode(t))
    end)

    it("returns a bounded encode error for mutation-created cycles", function()
        local t = qjson.decode('{"a":{}}')
        t.a.self = t
        assert.has_error(function()
            qjson.encode(t)
        end, "qjson.encode: circular reference")
    end)

    it("documents sparse array hole semantics after lazy array mutation", function()
        local t = qjson.decode('[1,2,3]')
        t[2] = nil

        local seen = {}
        for i, v in qjson.ipairs(t) do
            seen[#seen + 1] = i .. "=" .. tostring(v)
        end

        assert.are.equal(1, qjson.len(t))
        assert.are.same({"1=1"}, seen)
        assert.are.equal("[1]", qjson.encode(t))
    end)

    it("supports mutation during object iteration with final ordered state", function()
        local t = qjson.decode('{"a":1,"b":2}')
        local seen = {}
        for k, v in qjson.pairs(t) do
            seen[#seen + 1] = k .. "=" .. tostring(v)
            if k == "a" then
                t.b = 20
                t.c = 3
            end
        end

        assert.are.same({"a=1", "b=2"}, seen)
        assert.are.equal('{"a":1,"b":20,"c":3}', qjson.encode(t))
    end)

    it("rejects mutation during lazy array iteration with a clean error", function()
        local t = qjson.decode('[1,2,3]')
        assert.has_error(function()
            for i in qjson.ipairs(t) do
                if i == 1 then
                    t[2] = 20
                end
            end
        end, "qjson: invalid argument")
    end)

    it("forces number reserialization after a read without hiding normalization", function()
        local t = qjson.decode('{"big":9007199254740993,"negzero":-0,"sci":1.25e3,"float":12.5}')
        local _ = t.big
        t.extra = 1
        assert.are.equal(
            '{"big":9.007199254741e+15,"negzero":0,"sci":1250,"float":12.5,"extra":1}',
            qjson.encode(t)
        )
    end)

    it("keeps cached child proxies alive across GC", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        local a = t.a
        collectgarbage()
        collectgarbage()
        a.x = 7
        assert.are.equal('{"a":{"x":7},"b":2}', qjson.encode(t))
    end)

    it("encodes lazy proxies idempotently", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        t.a.x = 5
        local once = qjson.encode(t)
        local twice = qjson.encode(t)
        assert.are.equal(once, twice)
    end)

    it("does not pollute a source proxy when materializing a child", function()
        local t = qjson.decode('{"a":{"x":1},"b":2}')
        local child = t.a
        local materialized = qjson.materialize(child)
        materialized.x = 9

        assert.are.equal(1, child.x)
        assert.are.equal('{"a":{"x":1},"b":2}', qjson.encode(t))
    end)
end)
