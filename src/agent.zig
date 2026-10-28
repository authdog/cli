//! Agent Sign-In CLI (`authdog agent doctor|prove|onboard`).

const std = @import("std");
const fs_util = @import("fs_util.zig");
const agent_sign = @import("agent_sign.zig");
const api = @import("api/client.zig");
const management = @import("api/management.zig");
const auth_session = @import("auth_session.zig");
const globals = @import("globals.zig");
const login = @import("login.zig");
const out = @import("out.zig");
const project = @import("project.zig");

pub const ProveOptions = struct {
    accept_disclosure: bool = false,
    key_path: ?[]const u8 = null,
    kid: ?[]const u8 = null,
    auth_token: ?[]const u8 = null,
    identity_host: ?[]const u8 = null,
    environment_id: ?[]const u8 = null,
};

pub const DoctorOptions = struct {};

pub const RpInitOptions = struct {
    client_name: ?[]const u8 = null,
    redirect_storage: [10][]const u8 = undefined,
    redirect_count: u8 = 0,
    write_manifest: bool = true,

    pub fn redirectUris(self: *const RpInitOptions) []const []const u8 {
        return self.redirect_storage[0..self.redirect_count];
    }
};

pub const WaitOptions = struct {
    auth_token: ?[]const u8 = null,
    poll: bool = true,
};

pub const OnboardOptions = struct {
    name: ?[]const u8 = null,
    description: ?[]const u8 = null,
    scope_storage: [32][]const u8 = undefined,
    scope_count: usize = 0,
    tool_storage: [32][]const u8 = undefined,
    tool_count: usize = 0,
    delegation: bool = false,
    identity_assertion: bool = false,
    verify: bool = false,

    pub fn scopes(self: *const OnboardOptions) []const []const u8 {
        return self.scope_storage[0..self.scope_count];
    }

    pub fn tools(self: *const OnboardOptions) []const []const u8 {
        return self.tool_storage[0..self.tool_count];
    }
};

/// JSON body for `POST /v1/tenants/{tenant}/environments/{env}/agents/onboard`.
/// Unset optionals are omitted so Zod `.optional()` does not see null.
pub fn writeOnboardBody(writer: *std.Io.Writer, body: struct {
    application_id: []const u8,
    name: []const u8,
    description: ?[]const u8 = null,
    scopes: []const []const u8 = &.{},
    tools: []const []const u8 = &.{},
    delegation: bool = false,
    identity_assertion: bool = false,
    verify: bool = false,
}) !void {
    try writer.writeAll("{\"applicationId\":");
    try writer.print("{f}", .{std.json.fmt(body.application_id, .{})});
    try writer.writeAll(",\"name\":");
    try writer.print("{f}", .{std.json.fmt(body.name, .{})});
    if (body.description) |raw| {
        const description = std.mem.trim(u8, raw, " \t\r\n");
        if (description.len != 0) {
            try writer.writeAll(",\"description\":");
            try writer.print("{f}", .{std.json.fmt(description, .{})});
        }
    }
    if (body.scopes.len != 0) {
        try writer.writeAll(",\"allowedScopes\":");
        try writer.print("{f}", .{std.json.fmt(body.scopes, .{})});
    }
    if (body.tools.len != 0) {
        try writer.writeAll(",\"allowedTools\":");
        try writer.print("{f}", .{std.json.fmt(body.tools, .{})});
    }
    if (body.delegation) try writer.writeAll(",\"delegationEnabled\":true");
    if (body.identity_assertion) try writer.writeAll(",\"identityAssertion\":true");
    if (body.verify) try writer.writeAll(",\"verify\":true");
    try writer.writeByte('}');
}

pub const DoctorCheck = struct {
    name: []const u8,
    ok: bool,
    detail: []const u8,
};

const register_agent_rp_mutation =
    \\mutation RegisterOidcClient($meta: RegisterOidcClientInput!) {
    \\  registerOidcClient(meta: $meta) {
    \\    client { id clientId clientSecret name }
    \\    meta { error { message code } ok { message code } }
    \\  }
    \\}
;

const AgentEnv = struct {
    key_path: []const u8,
    kid: []const u8,
    identity_host: ?[]const u8,
    environment_id: ?[]const u8,
    auth_token: ?[]const u8,

    fn deinit(self: AgentEnv, allocator: std.mem.Allocator) void {
        allocator.free(self.key_path);
        allocator.free(self.kid);
        if (self.identity_host) |v| allocator.free(v);
        if (self.environment_id) |v| allocator.free(v);
        if (self.auth_token) |v| allocator.free(v);
    }
};

fn envOrAlloc(allocator: std.mem.Allocator, environ: *const std.process.Environ.Map, name: []const u8) !?[]const u8 {
    const v = environ.get(name) orelse return null;
    const trimmed = std.mem.trim(u8, v, " \t\r\n");
    if (trimmed.len == 0) return null;
    return try allocator.dupe(u8, trimmed);
}

fn resolveAgentEnv(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    prove: ProveOptions,
    require_auth_token: bool,
) !AgentEnv {
    const key_path = prove.key_path orelse try envOrAlloc(allocator, environ, "AUTHDOG_KEY") orelse {
        try out.fail(allocator, io, opts.format, "set AUTHDOG_KEY or --key");
    };
    const kid = prove.kid orelse try envOrAlloc(allocator, environ, "AUTHDOG_KID") orelse {
        try out.fail(allocator, io, opts.format, "set AUTHDOG_KID or --kid");
    };

    var identity_host: ?[]const u8 = null;
    var environment_id: ?[]const u8 = null;

    if (prove.identity_host) |h| {
        identity_host = try allocator.dupe(u8, login.trimTrailingSlashes(h));
    }
    if (prove.environment_id) |e| {
        environment_id = try allocator.dupe(u8, e);
    }

    if (identity_host == null or environment_id == null) {
        const project_path = try project.resolvePath(allocator, opts);
        defer allocator.free(project_path);
        if (project.load(allocator, io, project_path)) |linked| {
            defer if (linked) |l| l.deinit(allocator);
            if (linked) |l| {
                if (identity_host == null) {
                    identity_host = try allocator.dupe(u8, login.trimTrailingSlashes(l.identityOrigin));
                }
                if (environment_id == null) {
                    environment_id = try allocator.dupe(u8, l.environmentId);
                }
            }
        } else |_| {}
    }

    if (identity_host == null) identity_host = try envOrAlloc(allocator, environ, "IDENTITY_HOST");
    if (environment_id == null) environment_id = try envOrAlloc(allocator, environ, "ENVIRONMENT_ID");

    const auth_token = if (require_auth_token) blk: {
        if (prove.auth_token) |t| break :blk try allocator.dupe(u8, t);
        break :blk try envOrAlloc(allocator, environ, "AUTH_TOKEN") orelse {
            try out.fail(allocator, io, opts.format, "set AUTH_TOKEN or --auth-token");
        };
    } else null;

    return .{
        .key_path = try allocator.dupe(u8, key_path),
        .kid = try allocator.dupe(u8, kid),
        .identity_host = identity_host,
        .environment_id = environment_id,
        .auth_token = auth_token,
    };
}

pub fn appendAgentSignInDoctorChecks(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    checks: *std.ArrayList(DoctorCheck),
) !void {
    const cfg = resolveAgentEnv(allocator, io, environ, opts, .{}, false) catch |err| switch (err) {
        error.Reported => return,
        else => return err,
    };
    defer cfg.deinit(allocator);

    const key_ok = blk: {
        const pem = std.Io.Dir.cwd().readFileAlloc(io, cfg.key_path, allocator, .limited(256 * 1024)) catch {
            try checks.append(allocator, .{
                .name = "agent_key",
                .ok = false,
                .detail = try allocator.dupe(u8, "key file not found"),
            });
            break :blk false;
        };
        defer allocator.free(pem);
        _ = agent_sign.signAuthToken("probe", pem) catch {
            try checks.append(allocator, .{
                .name = "agent_key",
                .ok = false,
                .detail = try allocator.dupe(u8, "invalid EC private key (P-256 PEM expected)"),
            });
            break :blk false;
        };
        try checks.append(allocator, .{
            .name = "agent_key",
            .ok = true,
            .detail = try allocator.dupe(u8, "readable EC private key"),
        });
        break :blk true;
    };
    _ = key_ok;

    try checks.append(allocator, .{
        .name = "agent_kid",
        .ok = cfg.kid.len != 0,
        .detail = try allocator.dupe(u8, cfg.kid),
    });

    var issuer_ok = false;
    var issuer_detail = try allocator.dupe(u8, "set IDENTITY_HOST + ENVIRONMENT_ID or link project");
    if (cfg.identity_host != null and cfg.environment_id != null) {
        allocator.free(issuer_detail);
        const host = cfg.identity_host.?;
        const env_id = cfg.environment_id.?;
        const path = try std.fmt.allocPrint(allocator, "/oidc/{s}/.well-known/openid-configuration", .{env_id});
        defer allocator.free(path);
        const url = try api.joinUrl(allocator, host, path);
        defer allocator.free(url);
        const resp = try api.fetch(allocator, io, .GET, url, null, null);
        defer allocator.free(resp.body);
        issuer_ok = resp.status >= 200 and resp.status < 300;
        issuer_detail = try std.fmt.allocPrint(allocator, "HTTP {d}", .{resp.status});
    }
    try checks.append(allocator, .{ .name = "agent_issuer", .ok = issuer_ok, .detail = issuer_detail });
}

pub fn runDoctor(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    doctor: DoctorOptions,
) !void {
    _ = doctor;
    var checks: std.ArrayList(DoctorCheck) = .empty;
    defer {
        for (checks.items) |c| allocator.free(c.detail);
        checks.deinit(allocator);
    }
    try appendAgentSignInDoctorChecks(allocator, io, environ, opts, &checks);

    var all_ok = true;
    for (checks.items) |c| {
        if (!c.ok) all_ok = false;
    }

    if (opts.format == .json) {
        try out.emit(allocator, io, .json, "", .{
            .schemaVersion = 1,
            .ok = all_ok,
            .checks = checks.items,
            .hint = "Ready for authdog agent prove (set AUTH_TOKEN from the waiting page)",
        });
        return;
    }

    for (checks.items) |c| {
        const mark = if (c.ok) "✓" else "✗";
        try out.print(io, std.Io.File.stdout(), "{s} {s}: {s}\n", .{ mark, c.name, c.detail });
    }
    if (!all_ok) try out.fail(allocator, io, opts.format, "agent sign-in preflight failed");
    try out.print(io, std.Io.File.stdout(), "Ready for authdog agent prove (set AUTH_TOKEN from the waiting page)\n", .{});
}

pub fn runRpInit(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    rp: RpInitOptions,
) !void {
    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked_opt = try project.load(allocator, io, project_path);
    const linked = linked_opt orelse try out.fail(allocator, io, opts.format, "no linked project; run `authdog link` first");
    defer linked.deinit(allocator);

    const client_name = rp.client_name orelse {
        try out.fail(allocator, io, opts.format, "pass --name");
    };
    const redirect_uris = rp.redirectUris();
    if (redirect_uris.len == 0) {
        try out.fail(allocator, io, opts.format, "pass at least one --redirect-uri");
    }

    const GqlRequest = struct {
        query: []const u8,
        variables: struct {
            meta: struct {
                tenantId: []const u8,
                applicationId: []const u8,
                environmentId: []const u8,
                clientName: []const u8,
                redirectUris: []const []const u8,
                agentSignIn: bool = true,
                requirePkce: bool = true,
                grantTypes: []const []const u8 = &[_][]const u8{"authorization_code"},
                scopes: []const []const u8 = &[_][]const u8{ "openid", "email", "profile", "owner_profile", "owner_email" },
            },
        },
    };
    const request: GqlRequest = .{
        .query = register_agent_rp_mutation,
        .variables = .{
            .meta = .{
                .tenantId = linked.tenantId,
                .applicationId = linked.applicationId,
                .environmentId = linked.environmentId,
                .clientName = client_name,
                .redirectUris = redirect_uris,
            },
        },
    };
    var body: std.Io.Writer.Allocating = .init(allocator);
    defer body.deinit();
    try body.writer.print("{f}", .{std.json.fmt(request, .{})});

    const gql_url = management.endpointFromEnv(environ);
    const resp = try management.postGraphql(allocator, io, gql_url, auth.access_token, body.written());
    defer allocator.free(resp.body);

    if (resp.status < 200 or resp.status >= 300) {
        try api.reportApiFailure(allocator, io, opts.format, resp);
    }

    const GqlError = struct { message: ?[]const u8 = null };
    const RegisterMeta = struct { @"error": ?GqlError = null };
    const parsed = try std.json.parseFromSlice(struct {
        data: ?struct {
            registerOidcClient: ?struct {
                client: ?struct {
                    id: ?[]const u8 = null,
                    clientId: ?[]const u8 = null,
                    clientSecret: ?[]const u8 = null,
                    name: ?[]const u8 = null,
                } = null,
                meta: ?RegisterMeta = null,
            } = null,
        } = null,
        errors: ?[]GqlError = null,
    }, allocator, resp.body, .{ .ignore_unknown_fields = true });
    defer parsed.deinit();

    if (parsed.value.errors) |errs| {
        if (errs.len > 0) {
            const msg = errs[0].message orelse "GraphQL error";
            try out.fail(allocator, io, opts.format, msg);
        }
    }

    const reg = parsed.value.data orelse try out.fail(allocator, io, opts.format, "empty GraphQL response");
    const field = reg.registerOidcClient orelse try out.fail(allocator, io, opts.format, "registerOidcClient missing");
    if (field.meta) |meta| {
        if (meta.@"error") |err| {
            const msg = err.message orelse "registration failed";
            try out.fail(allocator, io, opts.format, msg);
        }
    }
    const client_row = field.client orelse try out.fail(allocator, io, opts.format, "client not returned");
    const client_id = client_row.clientId orelse try out.fail(allocator, io, opts.format, "clientId missing");
    const client_secret = client_row.clientSecret orelse try out.fail(allocator, io, opts.format, "clientSecret missing");

    if (rp.write_manifest) {
        const manifest_path = try std.fmt.allocPrint(allocator, "{s}/agent-rp.json", .{
            std.fs.path.dirname(project_path) orelse ".authdog",
        });
        defer allocator.free(manifest_path);
        var manifest: std.Io.Writer.Allocating = .init(allocator);
        defer manifest.deinit();
        try manifest.writer.print("{{\"schemaVersion\":1,\"clientId\":", .{});
        try manifest.writer.print("{f}", .{std.json.fmt(client_id, .{})});
        try manifest.writer.writeAll(",\"clientName\":");
        try manifest.writer.print("{f}", .{std.json.fmt(client_name, .{})});
        try manifest.writer.writeAll(",\"redirectUris\":");
        try manifest.writer.print("{f}", .{std.json.fmt(redirect_uris, .{})});
        try manifest.writer.writeAll("}\n");
        const parent = std.fs.path.dirname(manifest_path) orelse ".";
        try std.Io.Dir.cwd().createDirPath(io, parent);
        var parent_dir = try std.Io.Dir.cwd().openDir(io, parent, .{});
        defer parent_dir.close(io);
        var atomic = try parent_dir.createFileAtomic(io, std.fs.path.basename(manifest_path), .{
            .permissions = fs_util.sharedFileMode(),
            .replace = true,
        });
        defer atomic.deinit(io);
        var write_buf: [512]u8 = undefined;
        var file_writer = atomic.file.writer(io, &write_buf);
        try file_writer.interface.writeAll(manifest.written());
        try file_writer.interface.flush();
        try atomic.file.sync(io);
        try atomic.replace(io);
    }

    if (opts.format == .json) {
        try out.emit(allocator, io, .json, "", .{
            .schemaVersion = 1,
            .ok = true,
            .clientId = client_id,
            .clientSecret = client_secret,
            .clientName = client_name,
            .redirectUris = redirect_uris,
            .hint = "Copy clientSecret now; it is not stored locally",
        });
        return;
    }

    try out.print(io, std.Io.File.stdout(), "Registered Agent Sign-In client\n", .{});
    try out.print(io, std.Io.File.stdout(), "client_id: {s}\n", .{client_id});
    try out.print(io, std.Io.File.stdout(), "client_secret: {s}\n", .{client_secret});
    try out.print(io, std.Io.File.stderr(), "Copy the client secret now; it is not saved locally.\n", .{});
}

pub fn runOnboard(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    onboard: OnboardOptions,
) !void {
    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked_opt = try project.load(allocator, io, project_path);
    const linked = linked_opt orelse try out.fail(allocator, io, opts.format, "no linked project; run `authdog link` first");
    defer linked.deinit(allocator);

    const raw_name = onboard.name orelse try out.fail(allocator, io, opts.format, "pass --name");
    const name = std.mem.trim(u8, raw_name, " \t\r\n");
    if (name.len < 2) try out.fail(allocator, io, opts.format, "name must be at least 2 characters");

    const path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/agents/onboard", .{
        linked.tenantId,
        linked.environmentId,
    });
    defer allocator.free(path);
    const url = try api.joinUrl(allocator, opts.api_origin, path);
    defer allocator.free(url);

    var body: std.Io.Writer.Allocating = .init(allocator);
    defer body.deinit();
    try writeOnboardBody(&body.writer, .{
        .application_id = linked.applicationId,
        .name = name,
        .description = onboard.description,
        .scopes = onboard.scopes(),
        .tools = onboard.tools(),
        .delegation = onboard.delegation,
        .identity_assertion = onboard.identity_assertion,
        .verify = onboard.verify,
    });

    const resp = try api.postJson(allocator, io, url, auth.access_token, body.written());
    defer allocator.free(resp.body);
    if (resp.status < 200 or resp.status >= 300) {
        try api.reportApiFailure(allocator, io, opts.format, resp);
    }

    const parsed = std.json.parseFromSlice(struct {
        created: bool = false,
        entry: ?struct {
            id: ?[]const u8 = null,
            name: ?[]const u8 = null,
        } = null,
        clientId: ?[]const u8 = null,
        clientSecret: ?[]const u8 = null,
        meta: ?struct {
            @"error": ?struct {
                message: ?[]const u8 = null,
            } = null,
        } = null,
    }, allocator, resp.body, .{ .ignore_unknown_fields = true }) catch {
        try out.fail(allocator, io, opts.format, "invalid onboard response");
    };
    defer parsed.deinit();

    if (parsed.value.meta) |meta| {
        if (meta.@"error") |err| {
            if (err.message) |message| {
                if (message.len != 0) try out.fail(allocator, io, opts.format, message);
            }
        }
    }

    const created = parsed.value.created;
    const entry_id: ?[]const u8 = if (parsed.value.entry) |entry| entry.id else null;
    const entry_name: []const u8 = if (parsed.value.entry) |entry|
        (entry.name orelse name)
    else
        name;
    const client_id = parsed.value.clientId;

    const secret: ?[]const u8 = if (created) blk: {
        const value = parsed.value.clientSecret orelse try out.fail(allocator, io, opts.format, "clientSecret missing");
        if (value.len == 0) try out.fail(allocator, io, opts.format, "clientSecret missing");
        break :blk value;
    } else null;

    if (opts.format == .json) {
        if (secret) |client_secret| {
            try out.emit(allocator, io, .json, "", .{
                .schemaVersion = 1,
                .ok = true,
                .created = true,
                .entryId = entry_id,
                .name = entry_name,
                .clientId = client_id,
                .clientSecret = client_secret,
                .hint = "Copy clientSecret now; it is not stored locally",
            });
        } else {
            try out.emit(allocator, io, .json, "", .{
                .schemaVersion = 1,
                .ok = true,
                .created = false,
                .entryId = entry_id,
                .name = entry_name,
                .clientId = client_id,
            });
        }
        return;
    }

    if (created) {
        try out.print(io, std.Io.File.stdout(), "Onboarded agent\n", .{});
    } else {
        try out.print(io, std.Io.File.stdout(), "Agent already exists\n", .{});
    }
    try out.print(io, std.Io.File.stdout(), "name: {s}\n", .{entry_name});
    if (entry_id) |id| try out.print(io, std.Io.File.stdout(), "id: {s}\n", .{id});
    if (client_id) |id| try out.print(io, std.Io.File.stdout(), "client_id: {s}\n", .{id});
    if (secret) |client_secret| {
        try out.print(io, std.Io.File.stdout(), "client_secret: {s}\n", .{client_secret});
        try out.print(io, std.Io.File.stderr(), "Copy the client secret now; it is not saved locally.\n", .{});
    }
}

fn duration(nanoseconds: i96) std.Io.Clock.Duration {
    return .{
        .raw = .{ .nanoseconds = nanoseconds },
        .clock = .boot,
    };
}

pub fn runWait(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    wait: WaitOptions,
) !void {
    const cfg = try resolveAgentEnv(allocator, io, environ, opts, .{
        .auth_token = wait.auth_token,
    }, true);
    defer cfg.deinit(allocator);

    const identity_host = cfg.identity_host orelse try out.fail(allocator, io, opts.format, "set IDENTITY_HOST or link project");
    const environment_id = cfg.environment_id orelse try out.fail(allocator, io, opts.format, "set ENVIRONMENT_ID or link project");
    const auth_token = cfg.auth_token.?;

    if (!wait.poll) {
        try out.emit(allocator, io, opts.format, "Poll disabled.", .{
            .schemaVersion = 1,
            .ok = true,
            .authToken = auth_token,
        });
        return;
    }

    const PollBody = struct {
        status: ?[]const u8 = null,
        redirect: ?[]const u8 = null,
        expires_in: ?i64 = null,
    };

    while (true) {
        var encoded: [512]u8 = undefined;
        const enc_len = login.percentEncode(auth_token, &encoded);
        const path = try std.fmt.allocPrint(allocator, "/oidc/{s}/agents/wait?poll=1&auth_token={s}", .{
            environment_id,
            encoded[0..enc_len],
        });
        defer allocator.free(path);
        const url = try api.joinUrl(allocator, identity_host, path);
        defer allocator.free(url);

        const resp = try api.fetch(allocator, io, .GET, url, null, null);
        defer allocator.free(resp.body);
        if (resp.status < 200 or resp.status >= 300) {
            try out.fail(allocator, io, opts.format, "wait poll HTTP error");
        }

        const poll = try std.json.parseFromSlice(PollBody, allocator, resp.body, .{
            .ignore_unknown_fields = true,
        });
        defer poll.deinit();

        const status = poll.value.status orelse "unknown";
        if (std.mem.eql(u8, status, "issued")) {
            const redirect = poll.value.redirect orelse "";
            try out.emit(allocator, io, opts.format, "Agent Sign-In issued.", .{
                .schemaVersion = 1,
                .ok = true,
                .status = status,
                .redirect = redirect,
            });
            if (opts.format == .text and redirect.len != 0) {
                try out.print(io, std.Io.File.stdout(), "{s}\n", .{redirect});
            }
            return;
        }
        if (std.mem.eql(u8, status, "expired")) {
            try out.fail(allocator, io, opts.format, "auth_token expired or unknown");
        }

        try duration(750 * std.time.ns_per_ms).sleep(io);
    }
}

pub fn runProve(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    prove: ProveOptions,
) !void {
    if (!prove.accept_disclosure) {
        try out.fail(allocator, io, opts.format, "pass --accept-disclosure after the operator has seen the waiting-page disclosure");
    }

    const cfg = try resolveAgentEnv(allocator, io, environ, opts, prove, true);
    defer cfg.deinit(allocator);

    const identity_host = cfg.identity_host orelse try out.fail(allocator, io, opts.format, "set IDENTITY_HOST or link project with identityOrigin");
    const environment_id = cfg.environment_id orelse try out.fail(allocator, io, opts.format, "set ENVIRONMENT_ID or link project");

    const pem = try std.Io.Dir.cwd().readFileAlloc(io, cfg.key_path, allocator, .limited(256 * 1024));
    defer allocator.free(pem);

    const auth_token = cfg.auth_token.?;
    const raw_sig = agent_sign.signAuthToken(auth_token, pem) catch |err| switch (err) {
        agent_sign.SignError.InvalidPem,
        agent_sign.SignError.InvalidDer,
        agent_sign.SignError.NotEcPrivateKey,
        => try out.fail(allocator, io, opts.format, "invalid agent key PEM"),
        agent_sign.SignError.InvalidKeyLength,
        agent_sign.SignError.IdentityElement,
        => try out.fail(allocator, io, opts.format, "invalid P-256 key"),
    };
    const signature = try agent_sign.base64UrlEncode(allocator, &raw_sig);
    defer allocator.free(signature);

    const path = try std.fmt.allocPrint(allocator, "/oidc/{s}/agents/approve", .{environment_id});
    defer allocator.free(path);
    const url = try api.joinUrl(allocator, identity_host, path);
    defer allocator.free(url);

    var body_buf: std.Io.Writer.Allocating = .init(allocator);
    defer body_buf.deinit();
    try body_buf.writer.print("{f}", .{std.json.fmt(.{
        .auth_token = auth_token,
        .kid = cfg.kid,
        .signature = signature,
        .accept_disclosure = true,
    }, .{})});

    const resp = try api.fetch(allocator, io, .POST, url, null, body_buf.written());
    defer allocator.free(resp.body);

    if (resp.status < 200 or resp.status >= 300) {
        const message = api.apiErrorMessage(allocator, resp) catch try allocator.dupe(u8, "approve request failed");
        defer allocator.free(message);
        try out.fail(allocator, io, opts.format, message);
    }

    try out.emit(allocator, io, opts.format, "Agent Sign-In approved.", .{
        .schemaVersion = 1,
        .ok = true,
        .status = "proved",
    });
}

test "onboard body omits unset fields" {
    var buf: std.Io.Writer.Allocating = .init(std.testing.allocator);
    defer buf.deinit();
    try writeOnboardBody(&buf.writer, .{
        .application_id = "app_1",
        .name = "billing",
    });
    try std.testing.expectEqualStrings(
        "{\"applicationId\":\"app_1\",\"name\":\"billing\"}",
        buf.written(),
    );
}

test "onboard body includes optional flags and trims description" {
    var buf: std.Io.Writer.Allocating = .init(std.testing.allocator);
    defer buf.deinit();
    try writeOnboardBody(&buf.writer, .{
        .application_id = "app_1",
        .name = "billing",
        .description = "  bills  ",
        .scopes = &.{ "mcp:invoke", "openid" },
        .tools = &.{"checkout"},
        .delegation = true,
        .identity_assertion = true,
        .verify = true,
    });
    try std.testing.expectEqualStrings(
        "{\"applicationId\":\"app_1\",\"name\":\"billing\",\"description\":\"bills\",\"allowedScopes\":[\"mcp:invoke\",\"openid\"],\"allowedTools\":[\"checkout\"],\"delegationEnabled\":true,\"identityAssertion\":true,\"verify\":true}",
        buf.written(),
    );
    try std.testing.expect(std.mem.indexOf(u8, buf.written(), "null") == null);
}
