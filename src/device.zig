//! RFC 8628 device authorization grant (025 T011).

const std = @import("std");
const api = @import("api/client.zig");
const globals = @import("globals.zig");
const login = @import("login.zig");
const out = @import("out.zig");
const project = @import("project.zig");

pub const AuthorizeOptions = struct {
    client_id: ?[]const u8 = null,
    client_secret: ?[]const u8 = null,
    issuer: ?[]const u8 = null,
    scope: ?[]const u8 = null,
    poll: bool = true,
};

const DeviceStart = struct {
    device_code: ?[]const u8 = null,
    user_code: ?[]const u8 = null,
    verification_uri: ?[]const u8 = null,
    verification_uri_complete: ?[]const u8 = null,
    interval: ?i64 = null,
    @"error": ?[]const u8 = null,
};

const TokenResp = struct {
    access_token: ?[]const u8 = null,
    @"error": ?[]const u8 = null,
};

fn duration(nanoseconds: i96) std.Io.Clock.Duration {
    return .{
        .raw = .{ .nanoseconds = nanoseconds },
        .clock = .boot,
    };
}

const PollStep = union(enum) {
    wait: u32,
    stop: []const u8,
    approved,
};

fn pollStepFromTokenError(error_code: ?[]const u8, interval: u32) PollStep {
    const err = error_code orelse return .{ .stop = "invalid_grant" };
    if (std.mem.eql(u8, err, "authorization_pending")) return .{ .wait = @max(1, interval) };
    if (std.mem.eql(u8, err, "slow_down")) return .{ .wait = @max(1, interval + 5) };
    if (std.mem.eql(u8, err, "expired_token")) return .{ .stop = "expired_token" };
    if (std.mem.eql(u8, err, "access_denied")) return .{ .stop = "access_denied" };
    if (std.mem.eql(u8, err, "invalid_grant")) return .{ .stop = "invalid_grant" };
    return .{ .stop = err };
}

fn basicAuthHeader(allocator: std.mem.Allocator, client_id: []const u8, client_secret: []const u8) ![]u8 {
    const raw = try std.fmt.allocPrint(allocator, "{s}:{s}", .{ client_id, client_secret });
    defer allocator.free(raw);
    const enc_len = std.base64.standard.Encoder.calcSize(raw.len);
    const enc = try allocator.alloc(u8, enc_len);
    defer allocator.free(enc);
    _ = std.base64.standard.Encoder.encode(enc, raw);
    return std.fmt.allocPrint(allocator, "Basic {s}", .{enc});
}

fn resolveIssuer(
    allocator: std.mem.Allocator,
    environ: *const std.process.Environ.Map,
    opts: AuthorizeOptions,
    linked: ?project.LinkedProject,
) ![]u8 {
    if (opts.issuer) |iss| {
        const trimmed = login.trimTrailingSlashes(iss);
        return allocator.dupe(u8, trimmed);
    }
    if (environ.get("ISSUER")) |iss| {
        const trimmed = login.trimTrailingSlashes(iss);
        return allocator.dupe(u8, trimmed);
    }
    const proj = linked orelse return error.MissingIssuer;
    return std.fmt.allocPrint(allocator, "{s}/oidc/{s}", .{
        login.trimTrailingSlashes(proj.identityOrigin),
        proj.environmentId,
    });
}

fn credentialsFromEnv(
    allocator: std.mem.Allocator,
    environ: *const std.process.Environ.Map,
    opts: AuthorizeOptions,
) !struct { client_id: []const u8, client_secret: []const u8 } {
    const id = opts.client_id orelse environ.get("CLIENT_ID") orelse environ.get("AUTHDOG_OIDC_CLIENT_ID") orelse {
        return error.MissingClientId;
    };
    const secret = opts.client_secret orelse environ.get("CLIENT_SECRET") orelse environ.get("AUTHDOG_OIDC_CLIENT_SECRET") orelse {
        return error.MissingClientSecret;
    };
    return .{
        .client_id = try allocator.dupe(u8, id),
        .client_secret = try allocator.dupe(u8, secret),
    };
}

pub fn runAuthorize(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    auth_opts: AuthorizeOptions,
) !void {
    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);

    var linked_proj: ?project.LinkedProject = null;
    if (project.load(allocator, io, project_path)) |loaded| {
        if (loaded) |l| linked_proj = l;
    } else |_| {}
    defer if (linked_proj) |l| l.deinit(allocator);

    const issuer = resolveIssuer(allocator, environ, auth_opts, linked_proj) catch |err| switch (err) {
        error.MissingIssuer => try out.fail(allocator, io, opts.format, "set --issuer or link project / ISSUER env"),
        else => return err,
    };
    defer allocator.free(issuer);

    const creds = credentialsFromEnv(allocator, environ, auth_opts) catch |err| switch (err) {
        error.MissingClientId => try out.fail(allocator, io, opts.format, "set CLIENT_ID or --client-id"),
        error.MissingClientSecret => try out.fail(allocator, io, opts.format, "set CLIENT_SECRET or --client-secret"),
        else => return err,
    };
    defer {
        allocator.free(creds.client_id);
        allocator.free(creds.client_secret);
    }

    const scope = auth_opts.scope orelse "openid";
    const auth_header = try basicAuthHeader(allocator, creds.client_id, creds.client_secret);
    defer allocator.free(auth_header);

    const device_url = try std.fmt.allocPrint(allocator, "{s}/device", .{issuer});
    defer allocator.free(device_url);
    const start_body = try std.fmt.allocPrint(allocator, "client_id={s}&scope={s}", .{
        creds.client_id,
        scope,
    });
    defer allocator.free(start_body);

    const start_resp = try api.postForm(allocator, io, device_url, auth_header, start_body);
    defer allocator.free(start_resp.body);

    const started = try std.json.parseFromSlice(DeviceStart, allocator, start_resp.body, .{
        .ignore_unknown_fields = true,
    });
    defer started.deinit();

    if (start_resp.status < 200 or start_resp.status >= 300 or started.value.device_code == null) {
        const msg = started.value.@"error" orelse "device authorization failed";
        try out.fail(allocator, io, opts.format, msg);
    }

    const device_code = started.value.device_code.?;
    const user_code = started.value.user_code orelse "";
    const verify_url = started.value.verification_uri_complete orelse started.value.verification_uri orelse "";
    var interval: u32 = @intCast(started.value.interval orelse 5);
    if (interval == 0) interval = 5;

    if (opts.format == .text) {
        try out.print(io, std.Io.File.stdout(), "User code: {s}\nOpen: {s}\n", .{
            user_code,
            verify_url,
        });
    }

    if (!auth_opts.poll) {
        try out.emit(allocator, io, opts.format, "Device authorization started.", .{
            .schemaVersion = 1,
            .ok = true,
            .userCode = user_code,
            .verificationUri = verify_url,
            .deviceCode = device_code,
        });
        return;
    }

    const token_url = try std.fmt.allocPrint(allocator, "{s}/token", .{issuer});
    defer allocator.free(token_url);

    while (true) {
        const token_body = try std.fmt.allocPrint(allocator, "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code&device_code={s}&client_id={s}", .{
            device_code,
            creds.client_id,
        });
        defer allocator.free(token_body);

        const token_resp = try api.postForm(allocator, io, token_url, auth_header, token_body);
        defer allocator.free(token_resp.body);

        const token = try std.json.parseFromSlice(TokenResp, allocator, token_resp.body, .{
            .ignore_unknown_fields = true,
        });
        defer token.deinit();

        if (token.value.access_token) |access| {
            if (access.len != 0) {
                try out.emit(allocator, io, opts.format, if (opts.format == .text) "Approved" else "Device authorization approved.", .{
                    .schemaVersion = 1,
                    .ok = true,
                    .userCode = user_code,
                    .verificationUri = verify_url,
                    .accessToken = access,
                });
                return;
            }
        }

        const step = pollStepFromTokenError(token.value.@"error", interval);
        switch (step) {
            .approved => {},
            .wait => |seconds| {
                interval = seconds;
                try duration(@as(i96, seconds) * std.time.ns_per_s).sleep(io);
            },
            .stop => |err| try out.fail(allocator, io, opts.format, err),
        }
    }
}

test "pollStepFromTokenError maps RFC errors" {
    const pending = pollStepFromTokenError("authorization_pending", 5);
    try std.testing.expect(pending.wait == 5);

    const slow = pollStepFromTokenError("slow_down", 5);
    try std.testing.expect(slow.wait == 10);

    const denied = pollStepFromTokenError("access_denied", 5);
    try std.testing.expect(std.mem.eql(u8, denied.stop, "access_denied"));
}

test {
    _ = api;
}
