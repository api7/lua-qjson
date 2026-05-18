local ffi = require("ffi")

local tried = {}
local attempts = {}
local last_error
local required_symbols = {
    "qjson_strerror",
    "qjson_parse",
    "qjson_parse_ex",
    "qjson_free",
    "qjson_get_str",
    "qjson_get_i64",
    "qjson_get_f64",
    "qjson_get_bool",
    "qjson_is_null",
    "qjson_typeof",
    "qjson_len",
    "qjson_open",
    "qjson_cursor_open",
    "qjson_cursor_field",
    "qjson_cursor_index",
    "qjson_cursor_get_str",
    "qjson_cursor_get_i64",
    "qjson_cursor_get_f64",
    "qjson_cursor_get_bool",
    "qjson_cursor_typeof",
    "qjson_cursor_len",
    "qjson_cursor_bytes",
    "qjson_cursor_object_entry_at",
}

local function try_load(name)
    if tried[name] then
        return nil
    end
    tried[name] = true
    attempts[#attempts + 1] = name
    local ok, lib = pcall(ffi.load, name)
    if ok then
        for _, required_symbol in ipairs(required_symbols) do
            local has_symbol, symbol = pcall(function()
                return lib[required_symbol]
            end)
            if not has_symbol or symbol == nil then
                last_error = "loaded " .. name .. " but missing required symbol " .. required_symbol
                return nil
            end
        end
        return lib
    end
    last_error = lib
    return nil
end

local function load_from_cpath()
    local names = { "qjson", "libqjson" }
    for template in string.gmatch(package.cpath, "[^;]+") do
        if string.find(template, "?", 1, true) then
            for _, name in ipairs(names) do
                local path = string.gsub(template, "%?", name)
                local lib = try_load(path)
                if lib then
                    return lib
                end
            end
        end
    end
    return nil
end

local lib = try_load("qjson") or try_load("libqjson") or load_from_cpath()
if not lib then
    error("qjson: failed to load native library qjson; tried "
        .. table.concat(attempts, ", ")
        .. "; last error: " .. tostring(last_error))
end

return lib
