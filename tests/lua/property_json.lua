local M = {}

M.null = {}

local Rng = {}
Rng.__index = Rng

function M.rng(seed)
    seed = math.floor(tonumber(seed) or 1) % 2147483647
    if seed <= 0 then seed = seed + 2147483646 end
    return setmetatable({ state = seed }, Rng)
end

function Rng:next()
    self.state = (self.state * 48271) % 2147483647
    return self.state
end

function Rng:int(n)
    return (self:next() % n) + 1
end

function Rng:bool()
    return self:int(2) == 1
end

local function random_string(rng)
    local len = rng:int(9) - 1
    local out = {}
    for i = 1, len do
        local choice = rng:int(12)
        if choice <= 5 then
            out[i] = string.char(96 + rng:int(26))
        elseif choice == 6 then
            out[i] = tostring(rng:int(9) - 1)
        elseif choice == 7 then
            out[i] = '"'
        elseif choice == 8 then
            out[i] = "\\"
        elseif choice == 9 then
            out[i] = "\n"
        elseif choice == 10 then
            out[i] = "\t"
        elseif choice == 11 then
            out[i] = string.char(rng:int(31) - 1)
        else
            out[i] = " "
        end
    end
    return table.concat(out)
end

local function gen_scalar(rng)
    local choice = rng:int(6)
    if choice == 1 then
        return M.null
    elseif choice == 2 then
        return rng:bool()
    elseif choice == 3 then
        return rng:int(20001) - 10001
    elseif choice == 4 then
        return (rng:int(20001) - 10001) / 10
    end
    return random_string(rng)
end

local gen_value

local function gen_array(rng, depth, max_depth)
    local items = {}
    local n = rng:int(5) - 1
    for i = 1, n do
        items[i] = gen_value(rng, depth + 1, max_depth)
    end
    return { kind = "array", items = items }
end

local function gen_object(rng, depth, max_depth)
    local entries = {}
    local seen = {}
    local n = rng:int(5) - 1
    for i = 1, n do
        local key
        repeat
            key = string.format("k%d_%d_%d", depth, i, rng:int(9999))
        until not seen[key]
        seen[key] = true
        entries[i] = {
            key = key,
            value = gen_value(rng, depth + 1, max_depth),
        }
    end
    return { kind = "object", entries = entries }
end

gen_value = function(rng, depth, max_depth)
    if depth >= max_depth then
        return gen_scalar(rng)
    end

    local choice = rng:int(10)
    if choice <= 5 then
        return gen_scalar(rng)
    elseif choice <= 7 then
        return gen_array(rng, depth, max_depth)
    end
    return gen_object(rng, depth, max_depth)
end

function M.gen_container(rng, max_depth)
    max_depth = max_depth or 5
    if rng:bool() then
        return gen_object(rng, 0, max_depth)
    end
    return gen_array(rng, 0, max_depth)
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

function M.encode_json(v)
    if v == M.null then
        return "null"
    end

    local tv = type(v)
    if tv == "string" then
        return encode_string(v)
    elseif tv == "number" then
        return string.format("%.14g", v)
    elseif tv == "boolean" then
        return v and "true" or "false"
    elseif tv == "table" and v.kind == "array" then
        local parts = {}
        for i, item in ipairs(v.items) do
            parts[i] = M.encode_json(item)
        end
        return "[" .. table.concat(parts, ",") .. "]"
    elseif tv == "table" and v.kind == "object" then
        local parts = {}
        for i, entry in ipairs(v.entries) do
            parts[i] = encode_string(entry.key) .. ":" .. M.encode_json(entry.value)
        end
        return "{" .. table.concat(parts, ",") .. "}"
    end

    error("unsupported generated JSON node: " .. tv)
end

function M.deep_equal(a, b)
    if a == b then
        return true
    end
    if type(a) ~= type(b) then
        return false
    end
    if type(a) ~= "table" then
        return false
    end
    if getmetatable(a) ~= getmetatable(b) then
        return false
    end
    for k, v in pairs(a) do
        if not M.deep_equal(v, b[k]) then
            return false
        end
    end
    for k in pairs(b) do
        if a[k] == nil then
            return false
        end
    end
    return true
end

function M.gen_deep_chain(rng, depth)
    local root = {}
    local cur = root
    for _ = 2, depth do
        local child = {}
        if rng:bool() then
            cur[1] = child
        else
            cur.k = child
        end
        cur = child
    end
    return root
end

return M
