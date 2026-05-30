local qjson = require("qjson")

local function utf8_char(cp)
    if cp < 0x80 then
        return string.char(cp)
    elseif cp < 0x800 then
        return string.char(
            0xC0 + math.floor(cp / 0x40),
            0x80 + (cp % 0x40)
        )
    elseif cp < 0x10000 then
        return string.char(
            0xE0 + math.floor(cp / 0x1000),
            0x80 + (math.floor(cp / 0x40) % 0x40),
            0x80 + (cp % 0x40)
        )
    end
    return string.char(
        0xF0 + math.floor(cp / 0x40000),
        0x80 + (math.floor(cp / 0x1000) % 0x40),
        0x80 + (math.floor(cp / 0x40) % 0x40),
        0x80 + (cp % 0x40)
    )
end

describe("qjson exhaustive Unicode encode/decode", function()
    it("round-trips every Unicode scalar value through encode and parse", function()
        for cp = 0, 0x10FFFF do
            if cp < 0xD800 or cp > 0xDFFF then
                local s = utf8_char(cp)
                local encoded = qjson.encode(s)
                local doc = qjson.parse(encoded)
                assert.are.equal(s, doc:get_str(""), string.format("U+%04X encoded as %s", cp, encoded))
            end
        end
    end)
end)
