local qjson = require("qjson")
local cjson = require("cjson")
local LONG_ESC_A = "\\u0061\\u0062\\u0063\\u0064\\u0065"
local LONG_ESC_B = "\\u0066\\u0067\\u0068\\u0069\\u006A"

describe("qjson.materialize keep_origin", function()
    it("keeps default materialize semantics when keep_origin is not set", function()
        local src = '{"blob":"\\u0061"}'
        local t = qjson.materialize(qjson.decode(src))

        assert.is_nil(getmetatable(t))
        assert.are.equal("a", t.blob)
        assert.are.equal('{"blob":"a"}', qjson.encode(t))
    end)

    it("accepts keep_origin=true and validates options", function()
        local t = qjson.materialize(qjson.decode('{"a":1}'), { keep_origin = true })
        assert.is_nil(getmetatable(t))
        assert.are.equal(1, t.a)

        assert.has_error(function()
            qjson.materialize(qjson.decode('{"a":1}'), true)
        end, "qjson.materialize: opts must be a table")

        assert.has_error(function()
            qjson.materialize(qjson.decode('{"a":1}'), { keep_origin = 1 })
        end, "qjson.materialize: opts.keep_origin must be a boolean")
    end)

    it("does not guarantee reuse for short escaped strings when parent is changed", function()
        local t = qjson.materialize(qjson.decode('{"blob":"\\u0061","x":1}'), { keep_origin = true })
        t.x = 2

        assert.are.equal('{"blob":"a","x":2}', qjson.encode(t))
    end)

    it("reuses unchanged escaped string token when raw token is above threshold", function()
        local src = '{"blob":"' .. LONG_ESC_A .. '","x":1}'
        local t = qjson.materialize(qjson.decode(src), { keep_origin = true })
        t.x = 2

        assert.are.equal('{"blob":"' .. LONG_ESC_A .. '","x":2}', qjson.encode(t))
    end)

    it("falls back to normal escaping for changed string children", function()
        local t = qjson.materialize(qjson.decode('{"blob":"\\u0061","x":1}'), { keep_origin = true })
        t.blob = "line1\nline2"

        assert.are.equal('{"blob":"line1\\nline2","x":1}', qjson.encode(t))
    end)

    it("re-emits small-scalar containers field-by-field when unmodified", function()
        local src = '{ "n":1.0, "s":"\\u0061", "b":true, "u":null }'
        local t = qjson.materialize(qjson.decode(src), { keep_origin = true })
        local out = qjson.encode(t)

        assert.are.equal('{"n":1,"s":"a","b":true,"u":null}', out)
        assert.are_not.equal(src, out)
    end)

    it("returns original slice for unmodified containers with complete large children", function()
        local src = '{ "a":"' .. LONG_ESC_A .. '" , "b":"' .. LONG_ESC_B .. '" }'
        local t = qjson.materialize(qjson.decode(src), { keep_origin = true })

        assert.are.equal(src, qjson.encode(t))
    end)

    it("does not reintroduce duplicate keys after materialization", function()
        local t = qjson.materialize(qjson.decode('{"a":1,"a":2}'), { keep_origin = true })
        t.b = 3

        local out = qjson.encode(t)
        assert.are.equal('{"a":2,"b":3}', out)
        local _, count = out:gsub('"a":', "")
        assert.are.equal(1, count)
    end)

    it("does not splice standalone numeric tokens in changed parents", function()
        local t = qjson.materialize(qjson.decode('{"n":1.0,"e":1e3,"z":-0,"x":1}'), { keep_origin = true })
        t.x = 2

        assert.are.equal('{"n":1,"e":1000,"z":0,"x":2}', qjson.encode(t))
    end)

    it("partial origins do not hide nested table mutations behind a parent raw slice", function()
        local t = qjson.materialize(qjson.decode('{"a":{"x":1},"b":2}'), { keep_origin = true })
        t.a.x = 9

        assert.are.equal('{"a":{"x":9},"b":2}', qjson.encode(t))
    end)

    it("falls back to normal array/object classification for incomplete arrays", function()
        local src = '[ 1 , 2 , 3 ]'
        local t = qjson.materialize(qjson.decode(src), { keep_origin = true })

        assert.are.equal("[1,2,3]", qjson.encode(t))
    end)

    it("still reports circular references after materialization", function()
        local t = qjson.materialize(qjson.decode('{"a":1}'), { keep_origin = true })
        t.self = t

        assert.has_error(function()
            qjson.encode(t)
        end, "qjson.encode: circular reference")
    end)

    it("still reports circular references through origin child tables", function()
        local t = qjson.materialize(qjson.decode('{"a":{"x":1},"b":2}'), { keep_origin = true })
        t.a.self = t

        assert.has_error(function()
            qjson.encode(t)
        end, "qjson.encode: circular reference")
    end)

    it("still reports max-depth errors for unchanged origin trees", function()
        local parts = {}
        for i = 1, 1001 do
            parts[i] = '{"x":'
        end
        parts[#parts + 1] = '{}'
        for _ = 1, 1001 do
            parts[#parts + 1] = '}'
        end
        local t = qjson.materialize(qjson.decode(table.concat(parts)), { keep_origin = true })

        assert.has_error(function()
            qjson.encode(t)
        end, "qjson.encode: max depth exceeded")
    end)

    it("preserves lazy mutations made before keep_origin materialization", function()
        local lazy = qjson.decode('{"a":{"x":1},"b":2}')
        lazy.a.x = 9
        local t = qjson.materialize(lazy, { keep_origin = true })

        assert.are.equal(9, t.a.x)
        assert.are.equal(2, t.b)
    end)

    it("preserves lazy array child mutations made before keep_origin materialization", function()
        local lazy = qjson.decode('[{"x":1},{"y":2}]')
        lazy[1].x = 9
        local t = qjson.materialize(lazy, { keep_origin = true })

        assert.are.equal(9, t[1].x)
        assert.are.equal(2, t[2].y)
    end)

    it("keeps source bytes alive for provenance-backed reuse", function()
        local function materialized()
            local src = '{"blob":"' .. LONG_ESC_A .. '","x":1}'
            return qjson.materialize(qjson.decode(src), { keep_origin = true })
        end
        local t = materialized()
        collectgarbage("collect")
        t.x = 2

        assert.are.equal('{"blob":"' .. LONG_ESC_A .. '","x":2}', qjson.encode(t))
    end)

    it("reuses large complete child subtrees when parent is modified", function()
        local src = '{"x":0,"big": { "a":"' .. LONG_ESC_A .. '" , "b":"' .. LONG_ESC_B .. '" }}'
        local t = qjson.materialize(qjson.decode(src), { keep_origin = true })
        t.x = 9

        local out = qjson.encode(t)
        assert.are.equal(9, cjson.decode(out).x)
        assert.is_truthy(string.find(out, '"big":{ "a":"' .. LONG_ESC_A .. '" , "b":"' .. LONG_ESC_B .. '" }', 1, true))
    end)
end)
