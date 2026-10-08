//! `.authdog/project.json` link manifest.

const std = @import("std");
const globals = @import("globals.zig");
const login = @import("login.zig");

pub const LinkedProject = struct {
    schemaVersion: u32 = 1,
    tenantId: []const u8,
    applicationId: []const u8,
    environmentId: []const u8,
    apiOrigin: []const u8,
    identityOrigin: []const u8,
    linkedAt: []const u8,

    pub fn deinit(self: LinkedProject, allocator: std.mem.Allocator) void {
        allocator.free(self.tenantId);
        allocator.free(self.applicationId);
        allocator.free(self.environmentId);
        allocator.free(self.apiOrigin);
        allocator.free(self.identityOrigin);
        allocator.free(self.linkedAt);
    }
};

pub fn load(
    allocator: std.mem.Allocator,
    io: std.Io,
    path: []const u8,
) !?LinkedProject {
    const raw = std.Io.Dir.cwd().readFileAlloc(io, path, allocator, .limited(1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => return null,
        else => return err,
    };
    defer allocator.free(raw);

    const parsed = try std.json.parseFromSlice(LinkedProject, allocator, raw, .{
        .ignore_unknown_fields = true,
    });
    defer parsed.deinit();

    return .{
        .tenantId = try allocator.dupe(u8, parsed.value.tenantId),
        .applicationId = try allocator.dupe(u8, parsed.value.applicationId),
        .environmentId = try allocator.dupe(u8, parsed.value.environmentId),
        .apiOrigin = try allocator.dupe(u8, parsed.value.apiOrigin),
        .identityOrigin = try allocator.dupe(u8, parsed.value.identityOrigin),
        .linkedAt = try allocator.dupe(u8, parsed.value.linkedAt),
    };
}

pub fn save(io: std.Io, path: []const u8, project: LinkedProject) !void {
    const parent = std.fs.path.dirname(path) orelse ".";
    const base = std.fs.path.basename(path);
    try std.Io.Dir.cwd().createDirPath(io, parent);
    var parent_dir = try std.Io.Dir.cwd().openDir(io, parent, .{});
    defer parent_dir.close(io);

    var atomic = try parent_dir.createFileAtomic(io, base, .{
        .permissions = .fromMode(0o644),
        .replace = true,
    });
    defer atomic.deinit(io);

    var write_buf: [512]u8 = undefined;
    var file_writer = atomic.file.writer(io, &write_buf);
    try file_writer.interface.print("{f}\n", .{std.json.fmt(project, .{ .whitespace = .indent_2 })});
    try file_writer.interface.flush();
    try atomic.file.sync(io);
    try atomic.replace(io);
}

pub fn isoTimestamp(allocator: std.mem.Allocator, io: std.Io) ![]const u8 {
    const ts = std.Io.Clock.Timestamp.now(io, std.Io.Clock.real);
    const secs = ts.raw.toSeconds();
    return std.fmt.allocPrint(allocator, "{d}", .{secs});
}

pub fn newLinked(
    allocator: std.mem.Allocator,
    io: std.Io,
    tenant_id: []const u8,
    application_id: []const u8,
    environment_id: []const u8,
    api_origin: []const u8,
    identity_origin: []const u8,
) !LinkedProject {
    return .{
        .tenantId = try allocator.dupe(u8, tenant_id),
        .applicationId = try allocator.dupe(u8, application_id),
        .environmentId = try allocator.dupe(u8, environment_id),
        .apiOrigin = try allocator.dupe(u8, api_origin),
        .identityOrigin = try allocator.dupe(u8, identity_origin),
        .linkedAt = try isoTimestamp(allocator, io),
    };
}

pub fn resolvePath(allocator: std.mem.Allocator, globals_opts: globals.GlobalOptions) ![]const u8 {
    return allocator.dupe(u8, globals_opts.project_file);
}

test "linked project json roundtrip fields" {
    const p = LinkedProject{
        .tenantId = "t",
        .applicationId = "a",
        .environmentId = "e",
        .apiOrigin = globals.default_api_origin,
        .identityOrigin = login.default_identity_origin,
        .linkedAt = "2026-01-01T00:00:00Z",
    };
    var aw: std.Io.Writer.Allocating = .init(std.testing.allocator);
    defer aw.deinit();
    try aw.writer.print("{f}", .{std.json.fmt(p, .{})});
    const parsed = try std.json.parseFromSlice(LinkedProject, std.testing.allocator, aw.written(), .{
        .ignore_unknown_fields = true,
    });
    defer parsed.deinit();
    try std.testing.expectEqual(@as(u32, 1), parsed.value.schemaVersion);
}
