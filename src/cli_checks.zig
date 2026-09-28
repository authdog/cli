//! Spawns the authdog binary and checks help, status redaction, and logout.

const std = @import("std");

pub fn main(init: std.process.Init) !void {
    const allocator = init.gpa;
    const io = init.io;

    var iterator = try std.process.Args.Iterator.initAllocator(init.minimal.args, allocator);
    _ = iterator.next();
    const exe_arg = iterator.next() orelse return error.MissingExe;
    const exe = try allocator.dupe(u8, exe_arg);
    defer allocator.free(exe);
    iterator.deinit();

    try checkHelp(allocator, io, exe);
    try checkNoArgs(allocator, io, exe);
    try checkStatusAndLogout(allocator, io, init.environ_map, exe);
}

fn checkHelp(allocator: std.mem.Allocator, io: std.Io, exe: []const u8) !void {
    const result = try run(allocator, io, null, exe, &.{"--help"});
    defer result.deinit(allocator);
    try expectExit(result.term, 0);
    try expectContains(result.stdout, "login");
    try expectContains(result.stdout, "logout");
    try expectContains(result.stdout, "status");
    try expectContains(result.stdout, "--output");
}

fn checkNoArgs(allocator: std.mem.Allocator, io: std.Io, exe: []const u8) !void {
    const result = try run(allocator, io, null, exe, &.{});
    defer result.deinit(allocator);
    try expectExit(result.term, 0);
    try expectContains(result.stdout, "Usage: authdog");
}

fn checkStatusAndLogout(
    allocator: std.mem.Allocator,
    io: std.Io,
    parent_env: *std.process.Environ.Map,
    exe: []const u8,
) !void {
    var random_bytes: [8]u8 = undefined;
    try io.randomSecure(&random_bytes);
    var path_buf: [96]u8 = undefined;
    const config_dir = try std.fmt.bufPrint(&path_buf, "/tmp/authdog-cli-{x}", .{&random_bytes});
    try std.Io.Dir.cwd().createDirPath(io, config_dir);
    defer std.Io.Dir.cwd().deleteTree(io, config_dir) catch {};

    const creds = try std.fmt.allocPrint(allocator, "{s}/credentials.json", .{config_dir});
    defer allocator.free(creds);
    try std.Io.Dir.cwd().writeFile(io, .{
        .sub_path = creds,
        .data =
        \\{
        \\  "access_token": "access-secret",
        \\  "refresh_token": "refresh-secret"
        \\}
        ,
    });

    try parent_env.put("AUTHDOG_CONFIG_DIR", config_dir);

    const status = try run(allocator, io, parent_env, exe, &.{ "status", "--json" });
    defer status.deinit(allocator);
    try expectExit(status.term, 0);
    if (std.mem.indexOf(u8, status.stdout, "access-secret") != null) return error.TokenLeaked;
    if (std.mem.indexOf(u8, status.stdout, "refresh-secret") != null) return error.TokenLeaked;

    const parsed = try std.json.parseFromSlice(StatusJson, allocator, status.stdout, .{});
    defer parsed.deinit();
    if (!parsed.value.logged_in) return error.ExpectedLoggedIn;

    const logout = try run(allocator, io, parent_env, exe, &.{"logout"});
    defer logout.deinit(allocator);
    try expectExit(logout.term, 0);
    _ = std.Io.Dir.cwd().statFile(io, creds, .{}) catch |err| switch (err) {
        error.FileNotFound => return,
        else => return err,
    };
    return error.CredentialsRemain;
}

const StatusJson = struct {
    logged_in: bool,
    credentials_path: []const u8,
};

const Output = struct {
    term: std.process.Child.Term,
    stdout: []u8,
    stderr: []u8,

    fn deinit(self: Output, allocator: std.mem.Allocator) void {
        allocator.free(self.stdout);
        allocator.free(self.stderr);
    }
};

fn run(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: ?*std.process.Environ.Map,
    exe: []const u8,
    args: []const []const u8,
) !Output {
    var argv: [8][]const u8 = undefined;
    argv[0] = exe;
    for (args, 0..) |arg, i| argv[i + 1] = arg;

    const result = try std.process.run(allocator, io, .{
        .argv = argv[0 .. args.len + 1],
        .environ_map = environ,
    });
    return .{
        .term = result.term,
        .stdout = result.stdout,
        .stderr = result.stderr,
    };
}

fn expectExit(term: std.process.Child.Term, code: u8) !void {
    switch (term) {
        .exited => |actual| if (actual != code) return error.UnexpectedExit,
        else => return error.UnexpectedExit,
    }
}

fn expectContains(haystack: []const u8, needle: []const u8) !void {
    if (std.mem.indexOf(u8, haystack, needle) == null) return error.MissingText;
}
