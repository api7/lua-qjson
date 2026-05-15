local qd = require("quickdecode")

describe("quickdecode decoder pooling", function()
    it("new_decoder returns a usable object", function()
        local d = qd.new_decoder()
        assert.is_not_nil(d)
        assert.are.equal("function", type(d.parse))
        assert.are.equal("function", type(d.reset))
        assert.are.equal("function", type(d.destroy))
    end)

    it("parse returns a Doc supporting the full accessor surface", function()
        local dec = qd.new_decoder()
        local doc = dec:parse('{"name":"alice","age":30,"active":true}')

        assert.are.equal("alice", doc:get_str("name"))
        assert.are.equal(30, doc:get_i64("age"))
        assert.is_true(doc:get_bool("active"))
        assert.are.equal(qd.T_OBJ, doc:typeof(""))
        assert.are.equal(3, doc:len(""))
    end)

    it("reuses the decoder across multiple parses", function()
        local dec = qd.new_decoder()
        for i = 1, 5 do
            local doc = dec:parse(string.format('{"i":%d}', i))
            assert.are.equal(i, doc:get_i64("i"))
        end
    end)

    it("second parse marks the first doc stale (returns nil)", function()
        local dec  = qd.new_decoder()
        local doc1 = dec:parse('{"a":1}')
        assert.are.equal(1, doc1:get_i64("a"))

        local doc2 = dec:parse('{"b":2}')
        assert.are.equal(2, doc2:get_i64("b"))

        -- doc1 is stale; FFI returns QJD_STALE_DOC which the wrapper turns
        -- into nil (same convention as path-not-found).
        assert.is_nil(doc1:get_i64("a"))
        assert.is_nil(doc1:get_str("a"))
    end)

    it("reset invalidates outstanding docs and keeps the decoder usable", function()
        local dec = qd.new_decoder()
        local doc = dec:parse('{"x":42}')
        assert.are.equal(42, doc:get_i64("x"))

        dec:reset()
        assert.is_nil(doc:get_i64("x"))

        local doc2 = dec:parse('{"y":7}')
        assert.are.equal(7, doc2:get_i64("y"))
    end)

    it("destroy makes the decoder reject further parses", function()
        local dec = qd.new_decoder()
        dec:destroy()
        assert.has_error(function() dec:parse('{}') end)
    end)

    it("destroy invalidates outstanding docs (raises rather than nil)", function()
        -- Per design: post-destroy doc operations get QJD_INVALID_ARG, not
        -- QJD_STALE_DOC. The wrapper raises on QJD_INVALID_ARG.
        local dec = qd.new_decoder()
        local doc = dec:parse('{"a":1}')
        dec:destroy()
        assert.has_error(function() doc:get_i64("a") end)
    end)

    it("legacy qd.parse is not affected by decoder activity", function()
        local dec = qd.new_decoder()
        local pooled_doc = dec:parse('{"x":1}')
        local oneshot    = qd.parse('{"y":2}')

        -- Reparse the decoder; the one-shot doc must keep working.
        dec:parse('{"z":3}')
        assert.are.equal(2, oneshot:get_i64("y"))
        assert.is_nil(pooled_doc:get_i64("x"))  -- pooled doc became stale
    end)

    it("cursors opened from a doc become stale after reparse", function()
        local dec = qd.new_decoder()
        local doc = dec:parse('{"arr":[10,20,30]}')
        local cur = doc:open("arr")
        assert.are.equal(3, cur:len())

        dec:parse('{"other":true}')
        assert.is_nil(cur:len())
        assert.is_nil(cur:get_i64("[0]"))
    end)

    it("parse error does not poison the decoder", function()
        local dec = qd.new_decoder()
        assert.has_error(function() dec:parse('{') end)

        local doc = dec:parse('{"ok":1}')
        assert.are.equal(1, doc:get_i64("ok"))
    end)

    it("repeated reset and destroy on the same decoder are safe", function()
        local dec = qd.new_decoder()
        dec:reset()
        dec:reset()
        dec:destroy()
        dec:destroy()
        -- After destroy: still safe to call, just raises on parse.
        assert.has_error(function() dec:parse('{}') end)
    end)
end)
