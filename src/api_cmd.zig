//! `authdog api` calls one published `/v1` path with the saved session.
//! Secret fields are removed before anything is printed.

const std = @import("std");
const api = @import("api/client.zig");
const auth_session = @import("auth_session.zig");
const globals = @import("globals.zig");
const out = @import("out.zig");

pub const Request = struct {
    method: []const u8,
    path: []const u8,
    data: ?[]const u8 = null,
};

pub fn parseMethod(name: []const u8) ?std.http.Method {
    if (eqlIgnoreCase(name, "GET")) return .GET;
    if (eqlIgnoreCase(name, "POST")) return .POST;
    if (eqlIgnoreCase(name, "PUT")) return .PUT;
    if (eqlIgnoreCase(name, "PATCH")) return .PATCH;
    if (eqlIgnoreCase(name, "DELETE")) return .DELETE;
    return null;
}

pub fn allowsBody(method: std.http.Method) bool {
    return switch (method) {
        .POST, .PUT, .PATCH => true,
        else => false,
    };
}

pub fn validatePath(path: []const u8) bool {
    if (!std.mem.startsWith(u8, path, "/v1")) return false;
    if (path.len == 3) return true;
    if (path[3] != '/' and path[3] != '?') return false;
    if (std.mem.startsWith(u8, path, "//")) return false;
    if (std.mem.indexOf(u8, path, "://") != null) return false;
    if (std.mem.indexOf(u8, path, "..") != null) return false;
    if (std.mem.indexOfScalar(u8, path, '\\') != null) return false;
    if (std.mem.indexOfAny(u8, path, " \t\r\n") != null) return false;
    return true;
}

pub fn redactJson(allocator: std.mem.Allocator, bytes: []const u8) ![]u8 {
    var parsed = std.json.parseFromSlice(std.json.Value, allocator, bytes, .{}) catch return error.NotJson;
    defer parsed.deinit();
    redactValue(&parsed.value);
    var doc: std.Io.Writer.Allocating = .init(allocator);
    errdefer doc.deinit();
    try doc.writer.print("{f}", .{std.json.fmt(parsed.value, .{ .whitespace = .indent_2 })});
    return doc.toOwnedSlice();
}

pub fn run(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    request: Request,
) !void {
    const method = parseMethod(request.method) orelse try out.fail(allocator, io, opts.format, "method must be GET, POST, PUT, PATCH, or DELETE");
    if (!validatePath(request.path)) try out.fail(allocator, io, opts.format, "path must be a /v1 route on the API origin");
    if (request.data != null and !allowsBody(method)) try out.fail(allocator, io, opts.format, "only POST, PUT, and PATCH take --data");

    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const url = try api.joinUrl(allocator, auth.api_origin, request.path);
    defer allocator.free(url);
    const response = try api.fetch(allocator, io, method, url, auth.access_token, request.data);
    defer allocator.free(response.body);

    if (response.status == 401) try api.reportApiFailure(allocator, io, opts.format, response);

    const redacted = redactJson(allocator, response.body) catch |err| switch (err) {
        error.NotJson => {
            if (opts.format == .json) {
                try out.emit(allocator, io, .json, "", .{
                    .schemaVersion = 1,
                    .ok = response.status >= 200 and response.status < 300,
                    .status = response.status,
                    .@"error" = "response_not_json",
                });
            } else {
                try out.print(io, std.Io.File.stdout(), "HTTP {d}\nresponse was not JSON\n", .{response.status});
            }
            if (response.status < 200 or response.status >= 300) return error.ApiFailed;
            return;
        },
        else => return err,
    };
    defer allocator.free(redacted);

    const ok = response.status >= 200 and response.status < 300;
    if (opts.format == .json) {
        var envelope: std.Io.Writer.Allocating = .init(allocator);
        defer envelope.deinit();
        try envelope.writer.print("{{\"schemaVersion\":1,\"ok\":{s},\"status\":{d},\"data\":{s}}}\n", .{
            if (ok) "true" else "false",
            response.status,
            redacted,
        });
        try out.writeAll(io, std.Io.File.stdout(), envelope.written());
    } else {
        try out.print(io, std.Io.File.stdout(), "{s}\n", .{redacted});
    }
    if (!ok) return error.ApiFailed;
}

fn redactValue(value: *std.json.Value) void {
    switch (value.*) {
        .object => |*map| {
            var it = map.iterator();
            while (it.next()) |entry| {
                if (isSecretKey(entry.key_ptr.*)) {
                    entry.value_ptr.* = .{ .string = "***" };
                } else {
                    redactValue(entry.value_ptr);
                }
            }
        },
        .array => |*items| {
            for (items.items) |*item| redactValue(item);
        },
        else => {},
    }
}

fn isSecretKey(name: []const u8) bool {
    const keys = [_][]const u8{
        "signingSecret",
        "clientSecret",
        "access_token",
        "refresh_token",
        "secret",
        "token",
        "privateKey",
        "private_key",
        "datadogApiKey",
        "sysdigApiToken",
        "splunkToken",
        "sentinelSharedKey",
        "password",
    };
    for (keys) |key| {
        if (std.mem.eql(u8, name, key)) return true;
    }
    return false;
}

fn eqlIgnoreCase(left: []const u8, right: []const u8) bool {
    if (left.len != right.len) return false;
    for (left, right) |a, b| {
        if (std.ascii.toLower(a) != std.ascii.toLower(b)) return false;
    }
    return true;
}

test "path stays on /v1 and secret fields are removed" {
    try std.testing.expect(validatePath("/v1/userinfo"));
    try std.testing.expect(validatePath("/v1/tenants/t/projects?limit=1"));
    try std.testing.expect(!validatePath("https://evil.example/v1/userinfo"));
    try std.testing.expect(!validatePath("/v1/../secrets"));
    try std.testing.expect(!validatePath("//evil.example/v1"));
    try std.testing.expect(!validatePath("/v2/userinfo"));
    try std.testing.expect(parseMethod("get") == .GET);
    try std.testing.expect(parseMethod("TRACE") == null);
    try std.testing.expect(!allowsBody(.GET));
    try std.testing.expect(allowsBody(.POST));

    const redacted = try redactJson(std.testing.allocator,
        \\{"ok":true,"signingSecret":"whsec_secret","nested":{"access_token":"sekret","id":"abc"},"token":"raw-token"}
    );
    defer std.testing.allocator.free(redacted);
    try std.testing.expect(std.mem.indexOf(u8, redacted, "whsec_secret") == null);
    try std.testing.expect(std.mem.indexOf(u8, redacted, "sekret") == null);
    try std.testing.expect(std.mem.indexOf(u8, redacted, "raw-token") == null);
    try std.testing.expect(std.mem.indexOf(u8, redacted, "\"id\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, redacted, "abc") != null);
    try std.testing.expect(std.mem.indexOf(u8, redacted, "***") != null);
}
