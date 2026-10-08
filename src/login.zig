//! Browser OAuth hosted on Identity: `/signin/...` with `cli_sess` correlates browser + CLI.
//! The CLI listens on `http://127.0.0.1` and passes `cli_redirect` so Identity redirects the
//! browser here with `?grant=`; tokens are redeemed via `POST /api/v1/cli/oauth/redeem`.

const builtin = @import("builtin");
const std = @import("std");
const session = @import("session.zig");

pub const loopback_oauth_redirect_path = "/oauth/callback";
pub const default_identity_origin = "https://identity.authdog.com";
pub const default_console_environment_id = "ed89ef1e-2e76-4674-8272-5634064ae293";

const callback_success_html = @import("callback_page").html;
const browser_wait_ns: i96 = 8 * 60 * std.time.ns_per_s;
const redeem_wait_ns: i96 = 2 * 60 * std.time.ns_per_s;
const redeem_retry_ns: i96 = 500 * std.time.ns_per_ms;

pub const AuthConfig = struct {
    identity_origin: []const u8,
    environment_id: []const u8,

    pub fn fromEnv(allocator: std.mem.Allocator, environ: *const std.process.Environ.Map) !AuthConfig {
        const origin_raw = environ.get("AUTHDOG_IDENTITY_ORIGIN") orelse default_identity_origin;
        const environment = environ.get("AUTHDOG_CONSOLE_ENVIRONMENT_ID") orelse default_console_environment_id;
        return .{
            .identity_origin = try allocator.dupe(u8, trimTrailingSlashes(origin_raw)),
            .environment_id = try allocator.dupe(u8, environment),
        };
    }
};

pub fn trimTrailingSlashes(value: []const u8) []const u8 {
    var end = value.len;
    while (end > 0 and value[end - 1] == '/') end -= 1;
    return value[0..end];
}

pub fn redeemUrl(allocator: std.mem.Allocator, cfg: AuthConfig) ![]u8 {
    return std.fmt.allocPrint(allocator, "{s}/api/v1/cli/oauth/redeem", .{cfg.identity_origin});
}

pub fn pollUrl(allocator: std.mem.Allocator, cfg: AuthConfig, session_id: []const u8) ![]u8 {
    return std.fmt.allocPrint(allocator, "{s}/api/v1/cli/oauth/poll?session={s}", .{
        cfg.identity_origin,
        session_id,
    });
}

pub fn signinUrl(
    allocator: std.mem.Allocator,
    cfg: AuthConfig,
    session_id: []const u8,
    loopback_port: u16,
) ![]u8 {
    var redirect_buf: [80]u8 = undefined;
    const redirect = std.fmt.bufPrint(&redirect_buf, "http://127.0.0.1:{d}{s}", .{
        loopback_port,
        loopback_oauth_redirect_path,
    }) catch unreachable;

    var encoded_redirect: [240]u8 = undefined;
    const encoded_redirect_len = percentEncode(redirect, &encoded_redirect);
    var encoded_session: [80]u8 = undefined;
    const encoded_session_len = percentEncode(session_id, &encoded_session);

    return std.fmt.allocPrint(allocator, "{s}/signin/{s}?cli_sess={s}&cli_redirect={s}", .{
        cfg.identity_origin,
        cfg.environment_id,
        encoded_session[0..encoded_session_len],
        encoded_redirect[0..encoded_redirect_len],
    });
}

pub const PollStep = union(enum) {
    wait_ms: u64,
    tokens: session.StoredSession,
    fail: []const u8,
};

pub fn pollStep(allocator: std.mem.Allocator, status: u16, body: []const u8) !PollStep {
    if (status == 410) {
        return .{ .fail = "login session expired server-side (HTTP 410); close the browser tab and run /login again" };
    }
    if (status < 200 or status >= 300) {
        return .{ .fail = try std.fmt.allocPrint(allocator, "poll HTTP {d}: {s}", .{ status, body }) };
    }

    const parsed = std.json.parseFromSlice(PollResp, allocator, body, .{
        .ignore_unknown_fields = true,
    }) catch {
        return .{ .fail = try std.fmt.allocPrint(allocator, "invalid poll JSON (HTTP {d}): {s}", .{ status, body }) };
    };
    defer parsed.deinit();

    if (std.mem.eql(u8, parsed.value.status, "pending")) return .{ .wait_ms = 750 };
    if (std.mem.eql(u8, parsed.value.status, "complete")) {
        const access = parsed.value.access_token orelse return .{ .fail = "poll missing access_token" };
        const refresh = parsed.value.refresh_token orelse return .{ .fail = "poll missing refresh_token" };
        if (access.len == 0) return .{ .fail = "poll missing access_token" };
        if (refresh.len == 0) return .{ .fail = "poll missing refresh_token" };
        return .{ .tokens = .{
            .access_token = try allocator.dupe(u8, access),
            .refresh_token = try allocator.dupe(u8, refresh),
        } };
    }
    if (std.mem.eql(u8, parsed.value.status, "error")) {
        const message = parsed.value.@"error" orelse "poll error";
        if (message.len == 0) return .{ .fail = "poll error" };
        return .{ .fail = try allocator.dupe(u8, message) };
    }
    return .{ .wait_ms = 750 };
}

const PollResp = struct {
    status: []const u8,
    access_token: ?[]const u8 = null,
    refresh_token: ?[]const u8 = null,
    @"error": ?[]const u8 = null,
};

pub fn runBrowserLogin(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    cfg: AuthConfig,
) !void {
    const addr = try std.Io.net.IpAddress.parseIp4("127.0.0.1", 0);
    var listener = std.Io.net.IpAddress.listen(&addr, io, .{}) catch {
        try eprint(io, allocator, "bind 127.0.0.1 listener failed", .{});
        return error.Reported;
    };
    var close_listener = true;
    defer if (close_listener) listener.deinit(io);

    const port = listener.socket.address.getPort();
    var session_id_buf: [36]u8 = undefined;
    const session_id = try randomUuidV4(io, &session_id_buf);
    const signin = try signinUrl(allocator, cfg, &session_id, port);

    try eprint(io, allocator, "Redirecting to your browser for authentication…", .{});
    openBrowser(io, signin) catch |err| {
        try eprint(io, allocator, "open {s}: {t}", .{ signin, err });
        return error.Reported;
    };

    const browser_deadline = std.Io.Clock.Timestamp.fromNow(io, duration(browser_wait_ns));
    const grant = acceptGrant(io, &listener, browser_deadline, &close_listener) catch |err| switch (err) {
        error.TimedOut => {
            try eprint(io, allocator, "timed out waiting for browser redirect to http://127.0.0.1 (check VPN / firewall / proxy)", .{});
            return error.Reported;
        },
        else => return err,
    };

    const url = try redeemUrl(allocator, cfg);
    const body = try std.fmt.allocPrint(allocator, "{{\"grant\":\"{s}\"}}", .{grant});
    var tokens = try redeemTokens(allocator, io, url, body);
    const api_origin_raw = environ.get("AUTHDOG_API_ORIGIN") orelse "https://api.authdog.com";
    tokens.api_origin = try allocator.dupe(u8, trimTrailingSlashes(api_origin_raw));
    try session.saveSession(allocator, io, environ, tokens);
}

fn acceptGrant(
    io: std.Io,
    listener: *std.Io.net.Server,
    deadline: std.Io.Clock.Timestamp,
    close_listener: *bool,
) ![64]u8 {
    if (builtin.os.tag == .windows) return acceptGrantWindows(io, listener, deadline, close_listener);
    return acceptGrantPosix(io, listener, deadline);
}

fn acceptGrantPosix(io: std.Io, listener: *std.Io.net.Server, deadline: std.Io.Clock.Timestamp) ![64]u8 {
    while (std.Io.Clock.Timestamp.now(io, deadline.clock).compare(.lt, deadline)) {
        if (!try waitReadable(listener.socket.handle, 1000)) continue;
        if (try takeGrant(io, listener)) |grant| return grant;
    }
    return error.TimedOut;
}

/// Windows sockets in Zig are AFD handles, so `poll` is unavailable. A helper
/// thread closes the listener when the browser deadline passes, which unblocks
/// `accept`.
fn acceptGrantWindows(
    io: std.Io,
    listener: *std.Io.net.Server,
    deadline: std.Io.Clock.Timestamp,
    close_listener: *bool,
) ![64]u8 {
    const Guard = struct {
        handle: std.os.windows.HANDLE,
        stop: *std.atomic.Value(bool),

        fn run(guard: @This()) void {
            const sleep_ms: u32 = 200;
            var remaining_ms: u64 = @intCast(@divTrunc(browser_wait_ns, std.time.ns_per_ms));
            while (remaining_ms > 0) {
                if (guard.stop.load(.acquire)) return;
                const step: u32 = if (remaining_ms > sleep_ms) sleep_ms else @intCast(remaining_ms);
                windowsSleep(step);
                remaining_ms -|= step;
            }
            if (guard.stop.swap(true, .acq_rel)) return;
            std.os.windows.CloseHandle(guard.handle);
        }
    };

    var stop = std.atomic.Value(bool).init(false);
    const thread = try std.Thread.spawn(.{}, Guard.run, .{Guard{
        .handle = listener.socket.handle,
        .stop = &stop,
    }});
    defer {
        const guard_closed = stop.swap(true, .acq_rel);
        thread.join();
        if (guard_closed) close_listener.* = false;
    }

    while (std.Io.Clock.Timestamp.now(io, deadline.clock).compare(.lt, deadline)) {
        const grant = takeGrant(io, listener) catch |err| {
            if (!std.Io.Clock.Timestamp.now(io, deadline.clock).compare(.lt, deadline)) return error.TimedOut;
            return err;
        };
        if (grant) |value| return value;
    }
    return error.TimedOut;
}

fn takeGrant(io: std.Io, listener: *std.Io.net.Server) !?[64]u8 {
    const stream = listener.accept(io) catch |err| switch (err) {
        error.WouldBlock, error.ConnectionAborted => return null,
        else => return err,
    };
    defer stream.close(io);

    var grant: [64]u8 = undefined;
    const got = readGrant(io, stream, &grant) catch |err| {
        stream.shutdown(io, .both) catch {};
        return err;
    };
    stream.shutdown(io, .both) catch {};
    if (!got) return null;
    return grant;
}

fn waitReadable(handle: std.posix.fd_t, timeout_ms: i32) !bool {
    if (builtin.os.tag == .windows) return true;
    var poll_fds = [1]std.posix.pollfd{
        .{
            .fd = handle,
            .events = std.posix.POLL.IN,
            .revents = 0,
        },
    };
    const ready = try std.posix.poll(&poll_fds, timeout_ms);
    return ready != 0;
}

fn windowsSleep(milliseconds: u32) void {
    const kernel32 = struct {
        extern "kernel32" fn Sleep(dwMilliseconds: u32) callconv(.winapi) void;
    };
    kernel32.Sleep(milliseconds);
}

fn readGrant(io: std.Io, stream: std.Io.net.Stream, grant_out: *[64]u8) !bool {
    if (!try waitReadable(stream.socket.handle, 8000)) return error.ReadTimeout;

    var read_buf: [8192]u8 = undefined;
    var reader = stream.reader(io, &read_buf);
    const request_line_raw = reader.interface.takeDelimiterExclusive('\n') catch return error.BadRequest;
    const request_line = stripCarriageReturn(request_line_raw);

    var parts = std.mem.tokenizeScalar(u8, request_line, ' ');
    const method = parts.next() orelse {
        try respond(io, stream, "400 Bad Request", "text/plain; charset=utf-8", "bad request", false);
        return false;
    };
    const uri = parts.next() orelse {
        try respond(io, stream, "400 Bad Request", "text/plain; charset=utf-8", "bad request", false);
        return false;
    };

    while (true) {
        const header = reader.interface.takeDelimiterExclusive('\n') catch return error.BadRequest;
        const line = stripCarriageReturn(header);
        if (line.len == 0) break;
    }

    if (!std.ascii.eqlIgnoreCase(method, "GET")) {
        try respond(io, stream, "405 Method Not Allowed", "text/plain; charset=utf-8", "use GET", false);
        return false;
    }

    if (parseGrant(uri)) |grant| {
        @memcpy(grant_out, grant);
        try respond(io, stream, "200 OK", "text/html; charset=utf-8", callback_success_html, true);
        return true;
    }

    try respond(io, stream, "404 Not Found", "text/plain; charset=utf-8", "expected /oauth/callback?grant=", false);
    return false;
}

fn respond(
    io: std.Io,
    stream: std.Io.net.Stream,
    status_line: []const u8,
    content_type: []const u8,
    body: []const u8,
    prevent_browser_cache: bool,
) !void {
    var write_buf: [1024]u8 = undefined;
    var writer = stream.writer(io, &write_buf);
    try writer.interface.print("HTTP/1.1 {s}\r\nContent-Type: {s}\r\nContent-Length: {d}\r\n", .{
        status_line,
        content_type,
        body.len,
    });
    if (prevent_browser_cache) {
        try writer.interface.writeAll("Cache-Control: no-store, max-age=0\r\nPragma: no-cache\r\n");
    }
    try writer.interface.writeAll("Connection: close\r\n\r\n");
    try writer.interface.writeAll(body);
    try writer.interface.flush();
}

fn redeemTokens(allocator: std.mem.Allocator, io: std.Io, url: []const u8, body: []const u8) !session.StoredSession {
    var client: std.http.Client = .{
        .allocator = allocator,
        .io = io,
    };
    defer client.deinit();

    const deadline = std.Io.Clock.Timestamp.fromNow(io, duration(redeem_wait_ns));
    var last_fail: []const u8 = "redeem request never succeeded";

    while (std.Io.Clock.Timestamp.now(io, deadline.clock).compare(.lt, deadline)) {
        var response: std.Io.Writer.Allocating = .init(allocator);
        defer response.deinit();

        const fetched = client.fetch(.{
            .location = .{ .url = url },
            .method = .POST,
            .payload = body,
            .keep_alive = false,
            .extra_headers = &.{
                .{ .name = "Content-Type", .value = "application/json" },
            },
            .response_writer = &response.writer,
        }) catch |err| {
            last_fail = std.fmt.allocPrint(allocator, "{t}", .{err}) catch "redeem request failed";
            try duration(redeem_retry_ns).sleep(io);
            continue;
        };

        switch (try pollStep(allocator, @intFromEnum(fetched.status), response.written())) {
            .tokens => |tokens| return tokens,
            .wait_ms => {},
            .fail => |message| last_fail = message,
        }
        try duration(redeem_retry_ns).sleep(io);
    }

    try eprint(io, allocator, "{s}", .{last_fail});
    return error.Reported;
}

fn openBrowser(io: std.Io, url: []const u8) !void {
    var command_buf: [2048]u8 = undefined;
    const argv: []const []const u8 = switch (builtin.os.tag) {
        .windows => &.{
            "cmd.exe",
            "/C",
            std.fmt.bufPrint(&command_buf, "start \"\" \"{s}\"", .{url}) catch return error.CommandTooLong,
        },
        .macos => &.{ "open", url },
        else => &.{ "xdg-open", url },
    };
    var child = try std.process.spawn(io, .{
        .argv = argv,
        .stdin = .ignore,
        .stdout = .ignore,
        .stderr = .ignore,
    });
    const term = try child.wait(io);
    switch (term) {
        .exited => |code| if (code != 0) return error.BrowserFailed,
        else => return error.BrowserFailed,
    }
}

fn randomUuidV4(io: std.Io, out: *[36]u8) ![36]u8 {
    var bytes: [16]u8 = undefined;
    try io.randomSecure(&bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const groups = [_]usize{ 4, 2, 2, 2, 6 };
    var byte_index: usize = 0;
    var out_index: usize = 0;
    for (groups, 0..) |group, group_index| {
        if (group_index != 0) {
            out[out_index] = '-';
            out_index += 1;
        }
        for (0..group) |_| {
            const hex = "0123456789abcdef";
            out[out_index] = hex[bytes[byte_index] >> 4];
            out[out_index + 1] = hex[bytes[byte_index] & 0x0f];
            out_index += 2;
            byte_index += 1;
        }
    }
    return out.*;
}

fn duration(nanoseconds: i96) std.Io.Clock.Duration {
    return .{
        .raw = .{ .nanoseconds = nanoseconds },
        .clock = .boot,
    };
}

fn eprint(io: std.Io, allocator: std.mem.Allocator, comptime fmt: []const u8, args: anytype) !void {
    var message: std.Io.Writer.Allocating = .init(allocator);
    defer message.deinit();
    try message.writer.print(fmt, args);
    try message.writer.writeByte('\n');

    var buf: [256]u8 = undefined;
    var stderr = std.Io.File.stderr().writer(io, &buf);
    try stderr.interface.writeAll(message.written());
    try stderr.interface.flush();
}

fn stripCarriageReturn(value: []const u8) []const u8 {
    if (value.len > 0 and value[value.len - 1] == '\r') return value[0 .. value.len - 1];
    return value;
}

fn parseGrant(uri: []const u8) ?[]const u8 {
    if (uri.len == 0 or uri[0] != '/') return null;
    const query_at = std.mem.indexOfScalar(u8, uri, '?') orelse return null;
    const path = uri[0..query_at];
    if (!std.mem.eql(u8, path, loopback_oauth_redirect_path)) return null;

    var pairs = std.mem.splitScalar(u8, uri[query_at + 1 ..], '&');
    while (pairs.next()) |pair| {
        const eq = std.mem.indexOfScalar(u8, pair, '=') orelse continue;
        if (!std.mem.eql(u8, pair[0..eq], "grant")) continue;
        const value = pair[eq + 1 ..];
        if (looksLikeGrant(value)) return value;
    }
    return null;
}

fn looksLikeGrant(value: []const u8) bool {
    if (value.len != 64) return false;
    for (value) |byte| {
        if (!std.ascii.isHex(byte)) return false;
    }
    return true;
}

fn percentEncode(input: []const u8, out: []u8) usize {
    const hex = "0123456789ABCDEF";
    var n: usize = 0;
    for (input) |byte| {
        if (std.ascii.isAlphanumeric(byte) or byte == '-' or byte == '.' or byte == '_' or byte == '~') {
            out[n] = byte;
            n += 1;
        } else {
            out[n] = '%';
            out[n + 1] = hex[byte >> 4];
            out[n + 2] = hex[byte & 0x0f];
            n += 3;
        }
    }
    return n;
}

test "parses grant query from redirect target" {
    const hex = "00000000000000000000000000000001" ++ "00000000000000000000000000000000";
    const uri = "/oauth/callback?grant=" ++ hex;
    try std.testing.expectEqualStrings(hex, parseGrant(uri).?);
    try std.testing.expect(parseGrant("/nope") == null);
    try std.testing.expect(parseGrant("/oauth/callback?grant=nothex") == null);
}

test "cli urls strip trailing slash on origin" {
    const allocator = std.testing.allocator;
    const cfg = AuthConfig{
        .identity_origin = trimTrailingSlashes("https://identity.authdog.com/"),
        .environment_id = "env-id",
    };
    const sid = "00000000-0000-4000-b000-000000000042";
    const signin = try signinUrl(allocator, cfg, sid, 42424);
    defer allocator.free(signin);
    try std.testing.expect(std.mem.startsWith(u8, signin, "https://identity.authdog.com/signin/env-id?"));
    try std.testing.expect(std.mem.indexOf(u8, signin, "cli_sess=" ++ sid) != null);
    try std.testing.expect(std.mem.indexOf(
        u8,
        signin,
        "cli_redirect=http%3A%2F%2F127.0.0.1%3A42424%2Foauth%2Fcallback",
    ) != null);

    const poll = try pollUrl(allocator, cfg, sid);
    defer allocator.free(poll);
    try std.testing.expectEqualStrings(
        "https://identity.authdog.com/api/v1/cli/oauth/poll?session=00000000-0000-4000-b000-000000000042",
        poll,
    );
}

test "poll step pending maps to wait" {
    const step = try pollStep(std.testing.allocator, 200, "{\"status\":\"pending\"}");
    try std.testing.expectEqual(@as(u64, 750), step.wait_ms);
}

test "poll step complete requires tokens" {
    var arena_state = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena_state.deinit();
    const allocator = arena_state.allocator();

    const step = try pollStep(
        allocator,
        200,
        "{\"status\":\"complete\",\"access_token\":\"aa\",\"refresh_token\":\"bb\"}",
    );
    try std.testing.expectEqualStrings("aa", step.tokens.access_token);
    try std.testing.expectEqualStrings("bb", step.tokens.refresh_token);

    const missing = try pollStep(
        allocator,
        200,
        "{\"status\":\"complete\",\"access_token\":\"\",\"refresh_token\":\"b\"}",
    );
    try std.testing.expectEqualStrings("poll missing access_token", missing.fail);
}

test "poll step expired returns err" {
    const step = try pollStep(std.testing.allocator, 410, "{}");
    try std.testing.expect(std.mem.indexOf(u8, step.fail, "expired") != null);
}

test "poll step error propagates message" {
    var arena_state = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena_state.deinit();
    const step = try pollStep(arena_state.allocator(), 200, "{\"status\":\"error\",\"error\":\"oops\"}");
    try std.testing.expect(std.mem.indexOf(u8, step.fail, "oops") != null);
}

test "poll step unknown terminal status fallback wait" {
    const step = try pollStep(
        std.testing.allocator,
        200,
        "{\"status\":\"unexpected\",\"access_token\":\"\",\"refresh_token\":\"\"}",
    );
    try std.testing.expectEqual(@as(u64, 750), step.wait_ms);
}
