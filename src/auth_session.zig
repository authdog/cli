//! Resolve bearer tokens and API origin from stored credentials.

const std = @import("std");
const globals = @import("globals.zig");
const session = @import("session.zig");

pub const AuthContext = struct {
    access_token: []const u8,
    api_origin: []const u8,
};

pub fn requireAuth(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    global_api_origin: []const u8,
) !AuthContext {
    const loaded = try session.loadSession(allocator, io, environ);
    const stored = loaded orelse return error.AuthRequired;
    const api_origin = if (stored.api_origin) |origin|
        try allocator.dupe(u8, origin)
    else
        try allocator.dupe(u8, global_api_origin);
    return .{
        .access_token = try allocator.dupe(u8, stored.access_token),
        .api_origin = api_origin,
    };
}

pub fn deinitAuth(allocator: std.mem.Allocator, ctx: AuthContext) void {
    allocator.free(ctx.access_token);
    allocator.free(ctx.api_origin);
}

test {
    _ = globals;
}
