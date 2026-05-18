describe("qjson native loader", function()
    it("reports attempted libraries and the last loader error", function()
        local old_lib = package.loaded["qjson.lib"]
        local old_ffi = package.loaded.ffi
        local old_cpath = package.cpath

        package.loaded["qjson.lib"] = nil
        package.loaded.ffi = {
            load = function(name)
                error("mock load failure for " .. name)
            end,
        }
        package.cpath = "/tmp/qjson-test/?.so;/tmp/qjson-test/loadall.so"

        local ok, err = pcall(require, "qjson.lib")

        package.loaded["qjson.lib"] = old_lib
        package.loaded.ffi = old_ffi
        package.cpath = old_cpath

        assert.is_false(ok)
        assert.matches("qjson: failed to load native library qjson", err, 1, true)
        assert.matches("qjson, libqjson, /tmp/qjson-test/qjson.so, /tmp/qjson-test/libqjson.so", err, 1, true)
        assert.matches("mock load failure for /tmp/qjson-test/libqjson.so", err, 1, true)
        assert.is_nil(string.find(err, "loadall.so", 1, true))
    end)

    it("skips loadable libraries without qjson symbols", function()
        local old_lib = package.loaded["qjson.lib"]
        local old_ffi = package.loaded.ffi
        local old_cpath = package.cpath
        local valid_lib = { qjson_parse = true }

        package.loaded["qjson.lib"] = nil
        package.loaded.ffi = {
            load = function(name)
                if name == "qjson" then
                    return {}
                end
                if name == "libqjson" then
                    return valid_lib
                end
                error("unexpected load: " .. name)
            end,
        }
        package.cpath = ""

        local ok, lib = pcall(require, "qjson.lib")

        package.loaded["qjson.lib"] = old_lib
        package.loaded.ffi = old_ffi
        package.cpath = old_cpath

        assert.is_true(ok)
        assert.are.equal(valid_lib, lib)
    end)
end)
