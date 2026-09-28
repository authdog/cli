//! Conventional process-level CLI.

const std = @import("std");
const login = @import("login.zig");
const session = @import("session.zig");

pub const OutputFormat = enum { text, json };

pub const Command = enum { login, logout, status };

pub const Invocation = union(enum) {
    help,
    version,
    missing,
    bad: []const u8,
    run: struct {
        format: OutputFormat,
        command: Command,
    },
};

pub fn run(init: std.process.Init) !u8 {
    const allocator = init.arena.allocator();
    const io = init.io;

    var args_buf: [64][]const u8 = undefined;
    const args = try collectArgs(allocator, init.minimal.args, &args_buf);

    switch (try parse(allocator, args)) {
        .help => {
            try writeAll(io, std.Io.File.stdout(), help_text);
            return 0;
        },
        .version => {
            try print(io, std.Io.File.stdout(), "authdog {s}\n", .{@import("build_options").version});
            return 0;
        },
        .missing => {
            try writeAll(io, std.Io.File.stdout(), help_text);
            return 0;
        },
        .bad => |message| {
            try print(io, std.Io.File.stderr(), "error: {s}\n", .{message});
            return 2;
        },
        .run => |parsed| {
            try dispatch(allocator, io, init.environ_map, parsed.format, parsed.command);
            return 0;
        },
    }
}

fn dispatch(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *std.process.Environ.Map,
    format: OutputFormat,
    command: Command,
) !void {
    switch (command) {
        .login => {
            const cfg = try login.AuthConfig.fromEnv(allocator, environ);
            try login.runBrowserLogin(allocator, io, environ, cfg);
            try emit(allocator, io, format, "Logged in to Authdog.", .{ .logged_in = true });
        },
        .logout => {
            try session.clearSession(allocator, io, environ);
            try emit(allocator, io, format, "Logged out. Local credentials removed.", .{ .logged_in = false });
        },
        .status => {
            const loaded = try session.loadSession(allocator, io, environ);
            const path = try session.credentialsPath(allocator, io, environ);
            const logged_in = loaded != null;
            const text = try std.fmt.allocPrint(allocator, "Logged in: {s}\nCredentials: {s}", .{
                if (logged_in) "yes" else "no",
                path,
            });
            try emit(allocator, io, format, text, .{
                .logged_in = logged_in,
                .credentials_path = path,
            });
        },
    }
}

fn emit(
    allocator: std.mem.Allocator,
    io: std.Io,
    format: OutputFormat,
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

const help_text =
    \\Usage: authdog [OPTIONS] <COMMAND>
    \\
    \\Authdog CLI
    \\
    \\Commands:
    \\  login   Sign in through the hosted Authdog browser flow
    \\  logout  Delete locally stored credentials
    \\  status  Show local login status
    \\
    \\Options:
    \\  -o, --output <OUTPUT>  Output format for command data [possible values: text, json]
    \\      --json             Shorthand for --output json
    \\  -h, --help             Print help
    \\  -V, --version          Print version
    \\
;

pub fn parse(allocator: std.mem.Allocator, args: []const []const u8) !Invocation {
    var format: OutputFormat = .text;
    var format_set = false;
    var json = false;
    var command: ?Command = null;

    var index: usize = 0;
    while (index < args.len) : (index += 1) {
        const arg = args[index];
        if (std.mem.eql(u8, arg, "--help") or std.mem.eql(u8, arg, "-h")) return .help;
        if (std.mem.eql(u8, arg, "--version") or std.mem.eql(u8, arg, "-V")) return .version;
        if (std.mem.eql(u8, arg, "--json")) {
            json = true;
            continue;
        }
        if (std.mem.eql(u8, arg, "--output") or std.mem.eql(u8, arg, "-o")) {
            index += 1;
            if (index >= args.len) return .{ .bad = "missing value for --output" };
            format = parseFormat(args[index]) orelse {
                return .{ .bad = try std.fmt.allocPrint(allocator, "invalid output format '{s}'", .{args[index]}) };
            };
            format_set = true;
            continue;
        }
        if (std.mem.startsWith(u8, arg, "--output=")) {
            const value = arg["--output=".len..];
            format = parseFormat(value) orelse {
                return .{ .bad = try std.fmt.allocPrint(allocator, "invalid output format '{s}'", .{value}) };
            };
            format_set = true;
            continue;
        }
        if (std.mem.startsWith(u8, arg, "-o") and arg.len > 2) {
            const value = arg["-o".len..];
            format = parseFormat(value) orelse {
                return .{ .bad = try std.fmt.allocPrint(allocator, "invalid output format '{s}'", .{value}) };
            };
            format_set = true;
            continue;
        }
        if (std.mem.startsWith(u8, arg, "-")) {
            return .{ .bad = try std.fmt.allocPrint(allocator, "unexpected argument '{s}'", .{arg}) };
        }
        if (command != null) {
            return .{ .bad = try std.fmt.allocPrint(allocator, "unexpected argument '{s}'", .{arg}) };
        }
        command = std.meta.stringToEnum(Command, arg) orelse {
            return .{ .bad = try std.fmt.allocPrint(allocator, "unrecognized command '{s}'", .{arg}) };
        };
    }

    if (json and format_set) return .{ .bad = "--json conflicts with --output" };
    const selected = command orelse return .missing;
    return .{ .run = .{
        .format = if (json) .json else format,
        .command = selected,
    } };
}

fn parseFormat(value: []const u8) ?OutputFormat {
    if (std.mem.eql(u8, value, "text")) return .text;
    if (std.mem.eql(u8, value, "json")) return .json;
    return null;
}

fn collectArgs(allocator: std.mem.Allocator, args: std.process.Args, buf: [][]const u8) ![]const []const u8 {
    var iterator = try std.process.Args.Iterator.initAllocator(args, allocator);
    defer iterator.deinit();
    _ = iterator.next();
    var len: usize = 0;
    while (iterator.next()) |arg| {
        if (len >= buf.len) return error.TooManyArgs;
        buf[len] = try allocator.dupe(u8, arg);
        len += 1;
    }
    return buf[0..len];
}

fn print(io: std.Io, file: std.Io.File, comptime fmt: []const u8, args: anytype) !void {
    var buf: [512]u8 = undefined;
    var writer = file.writer(io, &buf);
    try writer.interface.print(fmt, args);
    try writer.interface.flush();
}

fn writeAll(io: std.Io, file: std.Io.File, bytes: []const u8) !void {
    var buf: [512]u8 = undefined;
    var writer = file.writer(io, &buf);
    try writer.interface.writeAll(bytes);
    try writer.interface.flush();
}

test {
    _ = login;
    _ = session;
}

test "parses login command" {
    const parsed = try parse(std.testing.allocator, &.{"login"});
    try std.testing.expectEqual(Command.login, parsed.run.command);
    try std.testing.expectEqual(OutputFormat.text, parsed.run.format);
}

test "parses status json flag" {
    const parsed = try parse(std.testing.allocator, &.{ "status", "--json" });
    try std.testing.expectEqual(Command.status, parsed.run.command);
    try std.testing.expectEqual(OutputFormat.json, parsed.run.format);
}

test "no args requests help" {
    const parsed = try parse(std.testing.allocator, &.{});
    try std.testing.expect(parsed == .missing);
}

test "json conflicts with explicit output" {
    const parsed = try parse(std.testing.allocator, &.{ "status", "--output", "text", "--json" });
    try std.testing.expectEqualStrings("--json conflicts with --output", parsed.bad);
}
