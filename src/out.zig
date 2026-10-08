//! stdout/stderr helpers and structured output.

const std = @import("std");
const globals = @import("globals.zig");

pub fn print(io: std.Io, file: std.Io.File, comptime fmt: []const u8, args: anytype) !void {
    var buf: [512]u8 = undefined;
    var writer = file.writer(io, &buf);
    try writer.interface.print(fmt, args);
    try writer.interface.flush();
}

pub fn writeAll(io: std.Io, file: std.Io.File, bytes: []const u8) !void {
    var buf: [512]u8 = undefined;
    var writer = file.writer(io, &buf);
    try writer.interface.writeAll(bytes);
    try writer.interface.flush();
}

pub fn emit(
    allocator: std.mem.Allocator,
    io: std.Io,
    format: globals.OutputFormat,
    text: []const u8,
    value: anytype,
) !void {
    switch (format) {
        .text => {
            if (text.len != 0) try print(io, std.Io.File.stdout(), "{s}\n", .{text});
        },
        .json => {
            var json: std.Io.Writer.Allocating = .init(allocator);
            defer json.deinit();
            try json.writer.print("{f}\n", .{std.json.fmt(value, .{ .whitespace = .indent_2 })});
            try writeAll(io, std.Io.File.stdout(), json.written());
        },
    }
}

pub fn errPrint(io: std.Io, allocator: std.mem.Allocator, comptime fmt: []const u8, args: anytype) !void {
    var buf: [512]u8 = undefined;
    var writer = std.Io.File.stderr().writer(io, &buf);
    const message = try std.fmt.allocPrint(allocator, fmt, args);
    defer allocator.free(message);
    try writer.interface.print("error: {s}\n", .{message});
    try writer.interface.flush();
}

pub fn fail(
    allocator: std.mem.Allocator,
    io: std.Io,
    format: globals.OutputFormat,
    message: []const u8,
) error{Reported}!noreturn {
    switch (format) {
        .text => errPrint(io, allocator, "{s}", .{message}) catch {},
        .json => emit(allocator, io, .json, "", .{
            .schemaVersion = 1,
            .ok = false,
            .@"error" = message,
        }) catch {},
    }
    return error.Reported;
}
