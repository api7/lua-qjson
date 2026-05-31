local qjson = require("qjson")

local doc = qjson.parse([[
{
  "body": {
    "model": "qjson-ci",
    "ok": true
  },
  "items": [10, 20, 30],
  "none": null
}
]])

assert(doc:get_str("body.model") == "qjson-ci")
assert(doc:get_bool("body.ok") == true)
assert(tonumber(doc:get_i64("items[1]")) == 20)
assert(doc:is_null("none") == true)

local decoded = qjson.decode([[{"field":"value","nested":{"n":42},"list":[true]}]])
assert(decoded.field == "value")
assert(decoded.nested.n == 42)
assert(decoded.list[1] == true)

local plain_pairs_keys = {}
local plain_pairs_values = {}
for k, v in pairs(decoded) do
    plain_pairs_keys[#plain_pairs_keys + 1] = k
    plain_pairs_values[k] = v
end
assert(#plain_pairs_keys == 3)
assert(plain_pairs_keys[1] == "field")
assert(plain_pairs_keys[2] == "nested")
assert(plain_pairs_keys[3] == "list")
assert(plain_pairs_values.field == "value")
assert(plain_pairs_values.nested.n == 42)
assert(plain_pairs_values.list[1] == true)

local plain_ipairs_values = {}
for i, v in ipairs(decoded.list) do
    plain_ipairs_values[i] = v
end
assert(#plain_ipairs_values == 1)
assert(plain_ipairs_values[1] == true)

local encoded = qjson.encode({
    field = "value",
    nested = { n = 42 },
    list = { true },
})
local roundtrip = qjson.decode(encoded)
assert(roundtrip.field == "value")
assert(roundtrip.nested.n == 42)
assert(roundtrip.list[1] == true)

doc = nil
decoded = nil
roundtrip = nil
collectgarbage("collect")
collectgarbage("collect")
