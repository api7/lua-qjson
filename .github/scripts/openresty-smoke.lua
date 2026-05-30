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
