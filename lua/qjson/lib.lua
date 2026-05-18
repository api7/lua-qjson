local ffi = require("ffi")

local tried = {}

local function try_load(name)
    if tried[name] then
        return nil
    end
    tried[name] = true
    local ok, lib = pcall(ffi.load, name)
    if ok then
        return lib
    end
    return nil
end

local function load_from_cpath()
    local names = { "qjson", "libqjson" }
    for template in string.gmatch(package.cpath, "[^;]+") do
        for _, name in ipairs(names) do
            local path = string.gsub(template, "%?", name)
            local lib = try_load(path)
            if lib then
                return lib
            end
        end
    end
    return nil
end

local lib = try_load("qjson") or try_load("libqjson") or load_from_cpath()
if not lib then
    error("qjson: failed to load native library qjson")
end

return lib
