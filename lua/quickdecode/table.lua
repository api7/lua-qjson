-- Lazy table view + cjson-compatible encoder for quickdecode.
--
-- This module relies on the FFI cdef set up by `lua/quickdecode.lua`, so
-- callers must `require("quickdecode")` (transitively or directly) before
-- they require this module.

local ffi = require("ffi")
local C   = ffi.load("quickdecode")

-- Optional cjson bridge: reuse its sentinels when available so callers'
-- `v == cjson.null` comparisons keep working unchanged.
local has_cjson, cjson = pcall(require, "cjson")

local _M = {}

if has_cjson then
    _M.null            = cjson.null
    _M.empty_array_mt  = cjson.empty_array_mt
else
    _M.null            = setmetatable({}, { __tostring = function() return "null" end })
    _M.empty_array_mt  = { __jsontype = "array" }
end

return _M
