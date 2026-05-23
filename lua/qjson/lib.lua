local ffi = require("ffi")

ffi.cdef[[
typedef struct qjson_doc qjson_doc;
typedef struct {
    const qjson_doc* doc;
    uint32_t idx_start, idx_end, _reserved0, _reserved1;
} qjson_cursor;

typedef struct {
    uint32_t mode;
    uint32_t max_depth;
} qjson_options;

const char* qjson_strerror(int code);
qjson_doc* qjson_parse   (const uint8_t* buf, size_t len, int* err_out);
qjson_doc* qjson_parse_ex(const uint8_t* buf, size_t len,
                       const qjson_options* opts, int* err_out);
void     qjson_free    (qjson_doc* doc);

int qjson_get_str (qjson_doc*, const char* path, size_t path_len, const uint8_t** p, size_t* n);
int qjson_get_i64 (qjson_doc*, const char* path, size_t path_len, int64_t* out);
int qjson_get_f64 (qjson_doc*, const char* path, size_t path_len, double*  out);
int qjson_get_bool(qjson_doc*, const char* path, size_t path_len, int*     out);
int qjson_is_null (qjson_doc*, const char* path, size_t path_len, int*     out);
int qjson_typeof  (qjson_doc*, const char* path, size_t path_len, int*     out);
int qjson_len     (qjson_doc*, const char* path, size_t path_len, size_t*  out);

int qjson_open        (qjson_doc*, const char* path, size_t path_len, qjson_cursor* out);
int qjson_cursor_open (const qjson_cursor*, const char* path, size_t path_len, qjson_cursor* out);
int qjson_cursor_field(const qjson_cursor*, const char* key,  size_t key_len, qjson_cursor* out);
int qjson_cursor_index(const qjson_cursor*, size_t i, qjson_cursor* out);

int qjson_cursor_get_str (const qjson_cursor*, const char*, size_t, const uint8_t**, size_t*);
int qjson_cursor_get_i64 (const qjson_cursor*, const char*, size_t, int64_t*);
int qjson_cursor_get_f64 (const qjson_cursor*, const char*, size_t, double*);
int qjson_cursor_get_bool(const qjson_cursor*, const char*, size_t, int*);
int qjson_cursor_typeof  (const qjson_cursor*, const char*, size_t, int*);
int qjson_cursor_len     (const qjson_cursor*, const char*, size_t, size_t*);
int qjson_cursor_bytes(const qjson_cursor*, size_t* byte_start, size_t* byte_end);
int qjson_cursor_object_entry_at(const qjson_cursor*, size_t i,
                                const uint8_t** key_ptr, size_t* key_len,
                                qjson_cursor* value_out);
int qjson_cursor_get_value(const qjson_cursor*,
                           int* type_out,
                           const uint8_t** str_ptr, size_t* str_len,
                           double* f64_out, int* bool_out,
                           size_t* byte_start, size_t* byte_end);
]]

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
    "qjson_cursor_get_value",
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
