//! Persist CLI credentials under the OS config dir (`~/.config/authdog-cli` on Linux).

const builtin = @import("builtin");
const std = @import("std");

pub const StoredSession = struct {
    access_token: []const u8,
    refresh_token: []const u8,
    api_origin: ?[]const u8 = null,
};

pub fn credentialsPath(allocator: std.mem.Allocator, io: std.Io, environ: *const std.process.Environ.Map) ![]u8 {
    const dir = try configDir(allocator, io, environ);
    defer allocator.free(dir);
    return std.fs.path.join(allocator, &.{ dir, "credentials.json" });
}

pub fn configDir(allocator: std.mem.Allocator, io: std.Io, environ: *const std.process.Environ.Map) ![]u8 {
    if (environ.get("AUTHDOG_CONFIG_DIR")) |override| {
        if (override.len != 0) {
            try std.Io.Dir.cwd().createDirPath(io, override);
            return allocator.dupe(u8, override);
        }
    }

    const path = try defaultConfigDir(allocator, environ);
    errdefer allocator.free(path);
    try std.Io.Dir.cwd().createDirPath(io, path);
    return path;
}

fn defaultConfigDir(allocator: std.mem.Allocator, environ: *const std.process.Environ.Map) ![]u8 {
    if (builtin.os.tag == .windows) {
        const appdata = environ.get("APPDATA") orelse return error.NoConfigDir;
        if (appdata.len == 0) return error.NoConfigDir;
        return std.fs.path.join(allocator, &.{ appdata, "Authdog", "authdog-cli" });
    }

    if (builtin.os.tag == .macos) {
        const home = environ.get("HOME") orelse return error.NoConfigDir;
        if (home.len == 0) return error.NoConfigDir;
        return std.fs.path.join(allocator, &.{
            home,
            "Library",
            "Application Support",
            "com.Authdog.authdog-cli",
        });
    }

    if (environ.get("XDG_CONFIG_HOME")) |xdg| {
        if (xdg.len != 0) return std.fs.path.join(allocator, &.{ xdg, "authdog-cli" });
    }
    const home = environ.get("HOME") orelse return error.NoConfigDir;
    if (home.len == 0) return error.NoConfigDir;
    return std.fs.path.join(allocator, &.{ home, ".config", "authdog-cli" });
}

pub fn loadSession(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
) !?StoredSession {
    const path = try credentialsPath(allocator, io, environ);
    defer allocator.free(path);
    return loadSessionFrom(allocator, io, path);
}

pub fn loadSessionFrom(allocator: std.mem.Allocator, io: std.Io, path: []const u8) !?StoredSession {
    const raw = std.Io.Dir.cwd().readFileAlloc(io, path, allocator, .limited(1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => return null,
        else => return err,
    };
    defer allocator.free(raw);

    const parsed = std.json.parseFromSlice(StoredSession, allocator, raw, .{
        .ignore_unknown_fields = true,
    }) catch return error.InvalidCredentials;
    defer parsed.deinit();

    const api_origin: ?[]const u8 = if (parsed.value.api_origin) |origin|
        try allocator.dupe(u8, origin)
    else
        null;
    return .{
        .access_token = try allocator.dupe(u8, parsed.value.access_token),
        .refresh_token = try allocator.dupe(u8, parsed.value.refresh_token),
        .api_origin = api_origin,
    };
}

pub fn saveSession(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    session: StoredSession,
) !void {
    const path = try credentialsPath(allocator, io, environ);
    defer allocator.free(path);
    return saveSessionTo(io, path, session);
}

pub fn saveSessionTo(io: std.Io, path: []const u8, session: StoredSession) !void {
    const parent = std.fs.path.dirname(path) orelse ".";
    const base = std.fs.path.basename(path);
    var parent_dir = try std.Io.Dir.cwd().createDirPathOpen(io, parent, .{});
    defer parent_dir.close(io);

    var atomic_file = try parent_dir.createFileAtomic(io, base, .{
        .permissions = privateFileMode(),
        .replace = true,
    });
    defer atomic_file.deinit(io);

    if (builtin.os.tag != .windows) {
        try atomic_file.file.setPermissions(io, privateFileMode());
    }

    var write_buf: [256]u8 = undefined;
    var file_writer = atomic_file.file.writer(io, &write_buf);
    try file_writer.interface.print("{f}\n", .{std.json.fmt(session, .{ .whitespace = .indent_2 })});
    try file_writer.interface.flush();
    try atomic_file.file.sync(io);
    try atomic_file.replace(io);
}

pub fn clearSession(allocator: std.mem.Allocator, io: std.Io, environ: *const std.process.Environ.Map) !void {
    const path = try credentialsPath(allocator, io, environ);
    defer allocator.free(path);
    return clearSessionAt(io, path);
}

pub fn clearSessionAt(io: std.Io, path: []const u8) !void {
    std.Io.Dir.cwd().deleteFile(io, path) catch |err| switch (err) {
        error.FileNotFound => return,
        else => return err,
    };
}

fn privateFileMode() std.Io.File.Permissions {
    if (builtin.os.tag == .windows) return .default_file;
    return .fromMode(0o600);
}

test "stored session json roundtrip" {
    const session = StoredSession{
        .access_token = "token-a",
        .refresh_token = "token-r",
    };
    var aw: std.Io.Writer.Allocating = .init(std.testing.allocator);
    defer aw.deinit();
    try aw.writer.print("{f}", .{std.json.fmt(session, .{})});

    const parsed = try std.json.parseFromSlice(StoredSession, std.testing.allocator, aw.written(), .{});
    defer parsed.deinit();
    try std.testing.expectEqualStrings("token-a", parsed.value.access_token);
    try std.testing.expectEqualStrings("token-r", parsed.value.refresh_token);
}

test "atomic store roundtrip and clear use injected path" {
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();

    const path = try std.fmt.allocPrint(
        std.testing.allocator,
        ".zig-cache/tmp/{s}/credentials.json",
        .{&tmp.sub_path},
    );
    defer std.testing.allocator.free(path);

    const session = StoredSession{
        .access_token = "access",
        .refresh_token = "refresh",
    };
    try saveSessionTo(io, path, session);

    const loaded = (try loadSessionFrom(std.testing.allocator, io, path)).?;
    defer {
        std.testing.allocator.free(loaded.access_token);
        std.testing.allocator.free(loaded.refresh_token);
    }
    try std.testing.expectEqualStrings("access", loaded.access_token);
    try std.testing.expectEqualStrings("refresh", loaded.refresh_token);

    if (builtin.os.tag != .windows) {
        const stat = try std.Io.Dir.cwd().statFile(io, path, .{});
        try std.testing.expectEqual(@as(std.posix.mode_t, 0o600), stat.permissions.toMode() & 0o777);
    }

    try clearSessionAt(io, path);
    try std.testing.expect((try loadSessionFrom(std.testing.allocator, io, path)) == null);
}
