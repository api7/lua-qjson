std             = "luajit"
max_line_length = false
unused_args     = false
redefined       = false

files["tests/lua/**/*.lua"] = {
    std = "luajit+busted",
}
