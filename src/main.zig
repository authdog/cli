//! Authdog CLI.

const std = @import("std");
const cli = @import("cli.zig");

pub fn main(init: std.process.Init) u8 {
    return cli.run(init) catch |err| {
        if (err != error.Reported) {
            var buf: [256]u8 = undefined;
            var stderr = std.Io.File.stderr().writer(init.io, &buf);
            stderr.interface.print("error: {t}\n", .{err}) catch {};
            stderr.interface.flush() catch {};
        }
        return 1;
    };
}
