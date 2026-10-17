//! HTTPS client for the Authdog REST API (`/v1`).

const std = @import("std");
const login = @import("../login.zig");
const out = @import("../out.zig");
const globals = @import("../globals.zig");

pub const ApiError = struct {
    status: u16,
    body: []const u8,
};

pub const Response = struct {
    status: u16,
    body: []const u8,
};

pub fn joinUrl(allocator: std.mem.Allocator, origin: []const u8, path: []const u8) ![]u8 {
    const base = login.trimTrailingSlashes(origin);
    if (path.len == 0) return allocator.dupe(u8, base);
    if (path[0] == '/') return std.fmt.allocPrint(allocator, "{s}{s}", .{ base, path });
    return std.fmt.allocPrint(allocator, "{s}/{s}", .{ base, path });
}

pub fn fetch(
    allocator: std.mem.Allocator,
    io: std.Io,
    method: std.http.Method,
    url: []const u8,
    bearer: ?[]const u8,
    payload: ?[]const u8,
) !Response {
    var client: std.http.Client = .{
        .allocator = allocator,
        .io = io,
    };
    defer client.deinit();

    var response_buf: std.Io.Writer.Allocating = .init(allocator);
    defer response_buf.deinit();

    var headers: [4]std.http.Header = undefined;
    var header_len: usize = 0;
    if (bearer) |token| {
        if (token.len != 0) {
            var auth_buf: [8192]u8 = undefined;
            const auth_value = try std.fmt.bufPrint(&auth_buf, "Bearer {s}", .{token});
            headers[header_len] = .{ .name = "Authorization", .value = auth_value };
            header_len += 1;
        }
    }
    if (payload != null) {
        headers[header_len] = .{ .name = "Content-Type", .value = "application/json" };
        header_len += 1;
    }

    const fetched = try client.fetch(.{
        .location = .{ .url = url },
        .method = method,
        .payload = payload,
        .keep_alive = false,
        .extra_headers = headers[0..header_len],
        .response_writer = &response_buf.writer,
    });

    const body = try allocator.dupe(u8, response_buf.written());
    return .{
        .status = @intFromEnum(fetched.status),
        .body = body,
    };
}

pub fn getJson(
    allocator: std.mem.Allocator,
    io: std.Io,
    api_origin: []const u8,
    path: []const u8,
    bearer: []const u8,
) !Response {
    const url = try joinUrl(allocator, api_origin, path);
    defer allocator.free(url);
    return fetch(allocator, io, .GET, url, bearer, null);
}

pub fn postJson(
    allocator: std.mem.Allocator,
    io: std.Io,
    url: []const u8,
    bearer: []const u8,
    body: []const u8,
) !Response {
    return fetch(allocator, io, .POST, url, bearer, body);
}

pub fn putJson(
    allocator: std.mem.Allocator,
    io: std.Io,
    api_origin: []const u8,
    path: []const u8,
    bearer: []const u8,
    body: []const u8,
) !Response {
    const url = try joinUrl(allocator, api_origin, path);
    defer allocator.free(url);
    return fetch(allocator, io, .PUT, url, bearer, body);
}

/// POST `application/x-www-form-urlencoded` (OAuth token/device endpoints).
pub fn postForm(
    allocator: std.mem.Allocator,
    io: std.Io,
    url: []const u8,
    authorization: ?[]const u8,
    form_body: []const u8,
) !Response {
    var client: std.http.Client = .{
        .allocator = allocator,
        .io = io,
    };
    defer client.deinit();

    var response_buf: std.Io.Writer.Allocating = .init(allocator);
    defer response_buf.deinit();

    var headers: [2]std.http.Header = undefined;
    var header_len: usize = 1;
    headers[0] = .{ .name = "Content-Type", .value = "application/x-www-form-urlencoded" };
    if (authorization) |value| {
        if (value.len != 0) {
            headers[header_len] = .{ .name = "Authorization", .value = value };
            header_len += 1;
        }
    }

    const fetched = try client.fetch(.{
        .location = .{ .url = url },
        .method = .POST,
        .payload = form_body,
        .keep_alive = false,
        .extra_headers = headers[0..header_len],
        .response_writer = &response_buf.writer,
    });

    const body = try allocator.dupe(u8, response_buf.written());
    return .{
        .status = @intFromEnum(fetched.status),
        .body = body,
    };
}

pub fn expect2xx(resp: Response) !void {
    if (resp.status >= 200 and resp.status < 300) return;
    return error.ApiFailed;
}

pub fn apiErrorMessage(allocator: std.mem.Allocator, resp: Response) ![]const u8 {
    const parsed = std.json.parseFromSlice(struct { @"error": ?[]const u8 = null }, allocator, resp.body, .{
        .ignore_unknown_fields = true,
    }) catch {
        return std.fmt.allocPrint(allocator, "HTTP {d}", .{resp.status});
    };
    defer parsed.deinit();
    if (parsed.value.@"error") |message| {
        if (message.len != 0) return try allocator.dupe(u8, message);
    }
    return std.fmt.allocPrint(allocator, "HTTP {d}", .{resp.status});
}

pub fn sessionExpiredCode(status: u16) ?[]const u8 {
    if (status == 401) return "session_expired";
    return null;
}

pub fn reportApiFailure(
    allocator: std.mem.Allocator,
    io: std.Io,
    format: globals.OutputFormat,
    resp: Response,
) error{ Reported, SessionExpired }!noreturn {
    if (sessionExpiredCode(resp.status)) |code| {
        switch (format) {
            .text => out.errPrint(io, allocator, "session expired; run authdog login", .{}) catch {},
            .json => out.emit(allocator, io, .json, "", .{
                .schemaVersion = 1,
                .ok = false,
                .@"error" = code,
            }) catch {},
        }
        return error.SessionExpired;
    }
    const message = apiErrorMessage(allocator, resp) catch {
        try out.fail(allocator, io, format, "API request failed");
    };
    defer allocator.free(message);
    try out.fail(allocator, io, format, message);
}

test "401 selects session_expired and hides token text" {
    const code = sessionExpiredCode(401).?;
    try std.testing.expectEqualStrings("session_expired", code);
    try std.testing.expect(std.mem.indexOf(u8, code, "access_token") == null);
    try std.testing.expect(sessionExpiredCode(403) == null);
}

test {
    _ = login;
    _ = out;
}
