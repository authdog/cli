//! Command implementations for the Authdog CLI.

const std = @import("std");
const api = @import("api/client.zig");
const auth_session = @import("auth_session.zig");
const detect = @import("detect.zig");
const globals = @import("globals.zig");
const login = @import("login.zig");
const out = @import("out.zig");
const project = @import("project.zig");
const sdk_config = @import("sdk_config.zig");
const session = @import("session.zig");
const agent = @import("agent.zig");
const mcp_config = @import("mcp_config.zig");
const fs_util = @import("fs_util.zig");
const webhook_rewrite = @import("webhook_rewrite.zig");
const builtin = @import("builtin");

pub const LinkOptions = struct {
    tenant_id: ?[]const u8 = null,
    application_id: ?[]const u8 = null,
    environment_id: ?[]const u8 = null,
};

pub const InitOptions = struct {
    framework: ?[]const u8 = null,
    dry_run: bool = false,
    skip_install: bool = false,
    env_file: ?[]const u8 = null,
    pm: ?[]const u8 = null,
};

pub const ImpersonateOptions = struct {
    user_id: ?[]const u8 = null,
    duration_minutes: u32 = 30,
    reason: ?[]const u8 = null,
    create_grant: bool = true,
    revoke: bool = false,
    grant_id: ?[]const u8 = null,
};

pub const WebhooksVerifyOptions = struct {
    secret: []const u8,
    signature: []const u8,
    body: []const u8,
    tolerance_seconds: u32 = 300,
};

pub const WebhooksListenOptions = struct {
    forward: ?[]const u8 = null,
    interval_ms: u32 = 2000,
    max_polls: u32 = 60,
    tunnel: ?[]const u8 = null,
    channel_id: ?[]const u8 = null,
    restore_url: ?[]const u8 = null,
};

pub const ConfigPatchOptions = struct {
    dry_run: bool = false,
};

const EnvConfigBundle = struct {
    session: []const u8,
    redirect: []const u8,
    webhooks: []const u8,
    oidc: []const u8,
};

const nextjs_snippet = @import("snippets").nextjs;
const express_snippet = @import("snippets").express;

const SectionDiff = struct {
    matches: bool,
    sessionConfig: []const u8,
    redirectUris: []const u8,
    webhooks: []const u8,
    oidcClients: []const u8,
};

pub const DoctorOptions = struct {
    check_mcp: bool = false,
    check_agent_sign_in: bool = false,
};

pub const McpCommand = struct {
    action: enum { install, list, uninstall } = .install,
    client: ?mcp_config.Client = null,
    all: bool = false,
};

pub fn parseMcpClient(name: []const u8) ?mcp_config.Client {
    return mcp_config.parseClient(name);
}

fn requireLinked(
    allocator: std.mem.Allocator,
    io: std.Io,
    format: globals.OutputFormat,
    project_path: []const u8,
) !project.LinkedProject {
    const linked = try project.load(allocator, io, project_path);
    if (linked) |value| return value;
    try out.fail(allocator, io, format, "no linked project; run `authdog link` first");
}

pub fn runWhoami(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
) !void {
    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const resp = try api.getJson(allocator, io, auth.api_origin, "/v1/userinfo", auth.access_token);
    defer allocator.free(resp.body);
    if (resp.status < 200 or resp.status >= 300) {
        try api.reportApiFailure(allocator, io, opts.format, resp);
    }

    if (opts.format == .json) {
        try out.emit(allocator, io, .json, "", .{
            .schemaVersion = 1,
            .ok = true,
            .userinfo = resp.body,
        });
        return;
    }

    const display = extractUserDisplay(allocator, resp.body) catch "Authenticated user";
    defer if (!std.mem.eql(u8, display, "Authenticated user")) allocator.free(display);
    try out.print(io, std.Io.File.stdout(), "{s}\n", .{display});
}

fn extractUserDisplay(allocator: std.mem.Allocator, body: []const u8) ![]const u8 {
    const parsed = try std.json.parseFromSlice(struct {
        user: ?struct {
            displayName: ?[]const u8 = null,
            id: ?[]const u8 = null,
            emails: ?[]struct { value: ?[]const u8 = null } = null,
        } = null,
    }, allocator, body, .{ .ignore_unknown_fields = true });
    defer parsed.deinit();
    const user = parsed.value.user orelse return allocator.dupe(u8, "Authenticated user");
    if (user.displayName) |name| if (name.len != 0) return allocator.dupe(u8, name);
    if (user.emails) |emails| {
        for (emails) |entry| {
            if (entry.value) |email| if (email.len != 0) return allocator.dupe(u8, email);
        }
    }
    if (user.id) |id| if (id.len != 0) return std.fmt.allocPrint(allocator, "user {s}", .{id});
    return allocator.dupe(u8, "Authenticated user");
}

pub fn runLink(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    link: LinkOptions,
) !void {
    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const identity_origin = try globals.identityOriginFromEnv(allocator, environ);
    defer allocator.free(identity_origin);

    const tenant_id = try resolveTenantId(allocator, io, opts, auth, link.tenant_id);
    defer if (link.tenant_id == null) allocator.free(tenant_id);

    const application_id = try resolveApplicationId(allocator, io, opts, auth, tenant_id, link.application_id);
    defer if (link.application_id == null) allocator.free(application_id);

    const environment_id = try resolveEnvironmentId(allocator, io, opts, auth, tenant_id, application_id, link.environment_id);
    defer if (link.environment_id == null) allocator.free(environment_id);

    const linked = try project.newLinked(
        allocator,
        io,
        tenant_id,
        application_id,
        environment_id,
        auth.api_origin,
        identity_origin,
    );

    const path = try project.resolvePath(allocator, opts);
    defer allocator.free(path);
    try project.save(io, path, linked);
    linked.deinit(allocator);

    try out.emit(allocator, io, opts.format, "Linked project.", .{
        .schemaVersion = 1,
        .ok = true,
        .projectFile = path,
        .tenantId = tenant_id,
        .applicationId = application_id,
        .environmentId = environment_id,
    });
}

pub fn runUnlink(
    allocator: std.mem.Allocator,
    io: std.Io,
    opts: globals.GlobalOptions,
) !void {
    const path = try project.resolvePath(allocator, opts);
    defer allocator.free(path);
    std.Io.Dir.cwd().deleteFile(io, path) catch |err| switch (err) {
        error.FileNotFound => {},
        else => return err,
    };
    try out.emit(allocator, io, opts.format, "Removed project link.", .{ .schemaVersion = 1, .ok = true });
}

fn resolveTenantId(
    allocator: std.mem.Allocator,
    io: std.Io,
    opts: globals.GlobalOptions,
    auth: auth_session.AuthContext,
    explicit: ?[]const u8,
) ![]const u8 {
    if (explicit) |id| return allocator.dupe(u8, id);

    const resp = try api.getJson(allocator, io, auth.api_origin, "/v1/tenants", auth.access_token);
    defer allocator.free(resp.body);
    if (resp.status < 200 or resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, resp);

    const parsed = try std.json.parseFromSlice(struct {
        tenants: ?[]struct { id: []const u8 } = null,
    }, allocator, resp.body, .{ .ignore_unknown_fields = true });
    defer parsed.deinit();
    const tenants = parsed.value.tenants orelse {
        try out.fail(allocator, io, opts.format, "no tenants visible; pass --tenant-id");
    };
    if (tenants.len == 1) return allocator.dupe(u8, tenants[0].id);
    if (opts.non_interactive or tenants.len == 0) {
        try out.fail(allocator, io, opts.format, "multiple tenants; pass --tenant-id");
    }
    try out.fail(allocator, io, opts.format, "multiple tenants; pass --tenant-id");
}

fn resolveApplicationId(
    allocator: std.mem.Allocator,
    io: std.Io,
    opts: globals.GlobalOptions,
    auth: auth_session.AuthContext,
    tenant_id: []const u8,
    explicit: ?[]const u8,
) ![]const u8 {
    if (explicit) |id| return allocator.dupe(u8, id);

    const path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/projects", .{tenant_id});
    defer allocator.free(path);
    const resp = try api.getJson(allocator, io, auth.api_origin, path, auth.access_token);
    defer allocator.free(resp.body);
    if (resp.status < 200 or resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, resp);

    const parsed = try std.json.parseFromSlice(struct {
        projects: ?[]struct {
            id: []const u8,
            defaultEnvironmentId: ?[]const u8 = null,
        } = null,
    }, allocator, resp.body, .{ .ignore_unknown_fields = true });
    defer parsed.deinit();
    const projects = parsed.value.projects orelse {
        try out.fail(allocator, io, opts.format, "no projects; pass --application-id");
    };
    if (projects.len == 1) return allocator.dupe(u8, projects[0].id);
    try out.fail(allocator, io, opts.format, "multiple projects; pass --application-id");
}

fn resolveEnvironmentId(
    allocator: std.mem.Allocator,
    io: std.Io,
    opts: globals.GlobalOptions,
    auth: auth_session.AuthContext,
    tenant_id: []const u8,
    application_id: []const u8,
    explicit: ?[]const u8,
) ![]const u8 {
    if (explicit) |id| return allocator.dupe(u8, id);

    const path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/applications/{s}", .{ tenant_id, application_id });
    defer allocator.free(path);
    const resp = try api.getJson(allocator, io, auth.api_origin, path, auth.access_token);
    defer allocator.free(resp.body);
    if (resp.status < 200 or resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, resp);

    const parsed = try std.json.parseFromSlice(struct {
        project: ?struct {
            defaultEnvironmentId: ?[]const u8 = null,
            environments: ?[]struct { id: []const u8 } = null,
        } = null,
    }, allocator, resp.body, .{ .ignore_unknown_fields = true });
    defer parsed.deinit();
    const proj = parsed.value.project orelse {
        try out.fail(allocator, io, opts.format, "project not found; pass --environment-id");
    };
    if (proj.defaultEnvironmentId) |env| if (env.len != 0) return allocator.dupe(u8, env);
    if (proj.environments) |envs| if (envs.len != 0) return allocator.dupe(u8, envs[0].id);
    try out.fail(allocator, io, opts.format, "no environment found; pass --environment-id");
}

pub fn runInit(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    init_opts: InitOptions,
) !void {
    const framework = if (init_opts.framework) |name|
        detect.Framework.parse(name) orelse {
            try out.fail(allocator, io, opts.format, "unknown --framework value");
        }
    else
        try detect.detectFramework(allocator, io);

    if (framework == .unknown and opts.non_interactive) {
        try out.fail(allocator, io, opts.format, "could not detect framework; pass --framework");
    }

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);

    var linked = project.load(allocator, io, project_path) catch null;
    if (linked == null) {
        try runLink(allocator, io, environ, opts, .{});
        linked = try project.load(allocator, io, project_path);
    }
    const proj = linked orelse {
        try out.fail(allocator, io, opts.format, "failed to link project");
    };
    defer proj.deinit(allocator);

    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const sdk_path = try sdk_config.fetchSdkConfigPath(
        allocator,
        proj.tenantId,
        proj.environmentId,
        proj.applicationId,
    );
    defer allocator.free(sdk_path);
    const sdk_resp = try api.getJson(allocator, io, auth.api_origin, sdk_path, auth.access_token);
    defer allocator.free(sdk_resp.body);
    if (sdk_resp.status < 200 or sdk_resp.status >= 300) {
        try api.reportApiFailure(allocator, io, opts.format, sdk_resp);
    }

    const wiring = try sdk_config.parseSdkConfigResponse(allocator, sdk_resp.body);
    defer sdk_config.deinitSdkWiring(allocator, wiring);
    const public_key = wiring.public_key;
    const client_id = wiring.client_id;

    const env_target = init_opts.env_file orelse ".env.local";
    const env_lines = try buildEnvLines(allocator, framework, public_key, client_id);
    defer {
        for (env_lines) |line| allocator.free(line);
        allocator.free(env_lines);
    }

    const package_manager = resolvePackageManager(io, init_opts.pm, framework) catch {
        try out.fail(allocator, io, opts.format, "unknown --pm value");
    };
    const integration = integrationPlan(io, framework);

    if (init_opts.dry_run) {
        try out.emit(allocator, io, opts.format, "Dry run — no files written.", .{
            .schemaVersion = 1,
            .ok = true,
            .framework = framework.label(),
            .envFile = env_target,
            .envLines = env_lines,
            .projectFile = project_path,
            .packageManager = package_manager,
            .integrationFiles = integration.slice(),
        });
        return;
    }

    try mergeEnvFile(allocator, io, env_target, env_lines);
    for (integration.slice()) |file| {
        if (std.mem.eql(u8, file.action, "create")) {
            try applyNewFile(io, file.path, snippetContents(framework));
        }
    }

    if (!init_opts.skip_install) {
        const package = framework.npmPackage() orelse framework.pythonPackage();
        if (package) |pkg| {
            try runPackageInstall(allocator, io, opts.format, pkg, package_manager);
        }
    }

    try out.emit(allocator, io, opts.format, "Authdog initialized in this project.", .{
        .schemaVersion = 1,
        .ok = true,
        .framework = framework.label(),
        .projectFile = project_path,
        .envFile = env_target,
        .publicKeySet = true,
        .clientIdSet = client_id != null,
        .packageManager = package_manager,
        .integrationFiles = integration.slice(),
    });
}

fn buildEnvLines(
    allocator: std.mem.Allocator,
    framework: detect.Framework,
    public_key: []const u8,
    client_id: ?[]const u8,
) ![][]const u8 {
    var list: std.ArrayList([]const u8) = .empty;
    errdefer {
        for (list.items) |line| allocator.free(line);
        list.deinit(allocator);
    }

    switch (framework) {
        .nextjs => {
            try list.append(allocator, try std.fmt.allocPrint(allocator, "NEXT_PUBLIC_PK_AUTHDOG={s}", .{public_key}));
            if (client_id) |id| try list.append(allocator, try std.fmt.allocPrint(allocator, "NEXT_PUBLIC_AUTHDOG_CLIENT_ID={s}", .{id}));
        },
        .javascript, .express => {
            try list.append(allocator, try std.fmt.allocPrint(allocator, "PK_AUTHDOG={s}", .{public_key}));
            if (client_id) |id| try list.append(allocator, try std.fmt.allocPrint(allocator, "AUTHDOG_CLIENT_ID={s}", .{id}));
        },
        else => {
            try list.append(allocator, try std.fmt.allocPrint(allocator, "PK_AUTHDOG={s}", .{public_key}));
            if (client_id) |id| try list.append(allocator, try std.fmt.allocPrint(allocator, "AUTHDOG_CLIENT_ID={s}", .{id}));
        },
    }
    return list.toOwnedSlice(allocator);
}

fn mergeEnvFile(allocator: std.mem.Allocator, io: std.Io, path: []const u8, lines: []const []const u8) !void {
    var existing: std.Io.Writer.Allocating = .init(allocator);
    defer existing.deinit();

    if (std.Io.Dir.cwd().readFileAlloc(io, path, allocator, .limited(1024 * 1024))) |raw| {
        defer allocator.free(raw);
        try existing.writer.writeAll(raw);
        if (raw.len > 0 and raw[raw.len - 1] != '\n') try existing.writer.writeAll("\n");
    } else |err| switch (err) {
        error.FileNotFound => {},
        else => return err,
    }

    for (lines) |line| {
        const eq = std.mem.indexOfScalar(u8, line, '=') orelse continue;
        const key = line[0..eq];
        if (std.mem.indexOf(u8, existing.written(), key) != null) continue;
        try existing.writer.print("{s}\n", .{line});
    }

    try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = path, .data = existing.written() });
}

fn fileExists(io: std.Io, name: []const u8) bool {
    std.Io.Dir.cwd().access(io, name, .{}) catch return false;
    return true;
}

const IntegrationFile = struct {
    path: []const u8,
    action: []const u8,
};

const IntegrationPlan = struct {
    files: [1]IntegrationFile = undefined,
    len: usize = 0,

    fn slice(self: *const IntegrationPlan) []const IntegrationFile {
        return self.files[0..self.len];
    }
};

fn isKnownPm(pm: []const u8) bool {
    const known = [_][]const u8{ "bun", "pnpm", "yarn", "npm", "uv", "pip" };
    for (known) |name| if (std.mem.eql(u8, pm, name)) return true;
    return false;
}

fn resolvePackageManager(io: std.Io, explicit: ?[]const u8, framework: detect.Framework) ![]const u8 {
    if (explicit) |pm| {
        if (!isKnownPm(pm)) return error.UnknownPackageManager;
        return pm;
    }
    if (framework == .fastapi) {
        if (fileExists(io, "uv.lock")) return "uv";
        return "pip";
    }
    if (fileExists(io, "bun.lock")) return "bun";
    if (fileExists(io, "pnpm-lock.yaml")) return "pnpm";
    if (fileExists(io, "yarn.lock")) return "yarn";
    return "npm";
}

fn snippetDest(framework: detect.Framework) ?[]const u8 {
    return switch (framework) {
        .nextjs => "app/authdog-provider.tsx",
        .express => "authdog.express.ts",
        else => null,
    };
}

fn snippetContents(framework: detect.Framework) []const u8 {
    return switch (framework) {
        .nextjs => nextjs_snippet,
        .express => express_snippet,
        else => "",
    };
}

fn integrationPlan(io: std.Io, framework: detect.Framework) IntegrationPlan {
    const dest = snippetDest(framework) orelse return .{};
    const action: []const u8 = if (fileExists(io, dest)) "skip-exists" else "create";
    return .{
        .files = .{.{ .path = dest, .action = action }},
        .len = 1,
    };
}

fn applyNewFile(io: std.Io, path: []const u8, contents: []const u8) !void {
    if (std.fs.path.dirname(path)) |dir| {
        if (dir.len != 0) try std.Io.Dir.cwd().createDirPath(io, dir);
    }
    try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = path, .data = contents });
}

fn runPackageInstall(
    allocator: std.mem.Allocator,
    io: std.Io,
    format: globals.OutputFormat,
    package: []const u8,
    pm: []const u8,
) !void {
    var buf: [3][]const u8 = undefined;
    const argv = installArgv(&buf, pm, package);
    spawnArgv(io, argv) catch |err| switch (err) {
        error.FileNotFound => {
            const message = try std.fmt.allocPrint(allocator, "package manager not found: {s}", .{pm});
            defer allocator.free(message);
            try out.fail(allocator, io, format, message);
        },
        else => return err,
    };
}

fn installArgv(buf: *[3][]const u8, pm: []const u8, package: []const u8) []const []const u8 {
    if (std.mem.eql(u8, pm, "bun")) {
        buf.* = .{ "bun", "add", package };
        return buf;
    }
    if (std.mem.eql(u8, pm, "pnpm")) {
        buf.* = .{ "pnpm", "add", package };
        return buf;
    }
    if (std.mem.eql(u8, pm, "yarn")) {
        buf.* = .{ "yarn", "add", package };
        return buf;
    }
    if (std.mem.eql(u8, pm, "uv")) {
        buf.* = .{ "uv", "add", package };
        return buf;
    }
    if (std.mem.eql(u8, pm, "pip")) {
        buf.* = .{ "pip", "install", package };
        return buf;
    }
    buf.* = .{ "npm", "install", package };
    return buf;
}

fn spawnArgv(io: std.Io, argv: []const []const u8) !void {
    var child = try std.process.spawn(io, .{
        .argv = argv,
        .stdin = .ignore,
        .stdout = .inherit,
        .stderr = .inherit,
    });
    const term = try child.wait(io);
    switch (term) {
        .exited => |code| if (code != 0) return error.Reported,
        else => return error.Reported,
    }
}

fn fetchEnvironmentConfigBundle(
    allocator: std.mem.Allocator,
    io: std.Io,
    format: globals.OutputFormat,
    auth: auth_session.AuthContext,
    linked: project.LinkedProject,
) !EnvConfigBundle {
    const session_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/session-config", .{ linked.tenantId, linked.environmentId });
    defer allocator.free(session_path);
    const redirect_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/applications/{s}/environments/{s}/redirect-uris", .{ linked.tenantId, linked.applicationId, linked.environmentId });
    defer allocator.free(redirect_path);
    const webhooks_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/webhooks", .{ linked.tenantId, linked.environmentId });
    defer allocator.free(webhooks_path);
    const oidc_path = try sdk_config.fetchOidcClientsPath(allocator, linked.tenantId, linked.applicationId, linked.environmentId);
    defer allocator.free(oidc_path);

    const session_resp = try api.getJson(allocator, io, auth.api_origin, session_path, auth.access_token);
    const redirect_resp = try api.getJson(allocator, io, auth.api_origin, redirect_path, auth.access_token);
    const webhooks_resp = try api.getJson(allocator, io, auth.api_origin, webhooks_path, auth.access_token);
    const oidc_resp = try api.getJson(allocator, io, auth.api_origin, oidc_path, auth.access_token);
    const responses = [_]api.Response{ session_resp, redirect_resp, webhooks_resp, oidc_resp };
    for (responses) |resp| {
        if (resp.status < 200 or resp.status >= 300) {
            for (responses) |item| allocator.free(item.body);
            try api.reportApiFailure(allocator, io, format, resp);
        }
    }
    return .{
        .session = session_resp.body,
        .redirect = redirect_resp.body,
        .webhooks = webhooks_resp.body,
        .oidc = oidc_resp.body,
    };
}

fn formatConfigDocument(allocator: std.mem.Allocator, bundle: EnvConfigBundle) ![]u8 {
    const session_json = try parseJsonValue(allocator, bundle.session);
    defer session_json.deinit();
    const redirect = try parseJsonValue(allocator, bundle.redirect);
    defer redirect.deinit();
    const webhooks = try parseJsonValue(allocator, bundle.webhooks);
    defer webhooks.deinit();
    const oidc = try parseJsonValue(allocator, bundle.oidc);
    defer oidc.deinit();

    var webhooks_value = webhooks.value;
    var oidc_value = oidc.value;
    try redactSecrets(allocator, &webhooks_value);
    try redactSecrets(allocator, &oidc_value);

    var doc: std.Io.Writer.Allocating = .init(allocator);
    errdefer doc.deinit();
    try doc.writer.writeAll("{\"schemaVersion\":1,\"sessionConfig\":");
    try doc.writer.print("{f}", .{std.json.fmt(session_json.value, .{})});
    try doc.writer.writeAll(",\"redirectUris\":");
    try doc.writer.print("{f}", .{std.json.fmt(redirect.value, .{})});
    try doc.writer.writeAll(",\"webhooks\":");
    try doc.writer.print("{f}", .{std.json.fmt(webhooks_value, .{})});
    try doc.writer.writeAll(",\"oidcClients\":");
    try doc.writer.print("{f}", .{std.json.fmt(oidc_value, .{})});
    try doc.writer.writeAll("}");
    return doc.toOwnedSlice();
}

pub fn runConfigPull(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
) !void {
    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked = try requireLinked(allocator, io, opts.format, project_path);
    defer linked.deinit(allocator);

    const bundle = try fetchEnvironmentConfigBundle(allocator, io, opts.format, auth, linked);
    defer {
        allocator.free(bundle.session);
        allocator.free(bundle.redirect);
        allocator.free(bundle.webhooks);
        allocator.free(bundle.oidc);
    }

    const doc = formatConfigDocument(allocator, bundle) catch {
        try out.fail(allocator, io, opts.format, "config response was not JSON");
    };
    defer allocator.free(doc);

    const config_path = globals.default_config_file;
    try std.Io.Dir.cwd().createDirPath(io, ".authdog");
    try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = config_path, .data = doc });

    try out.emit(allocator, io, opts.format, "Pulled config to .authdog/config.json", .{
        .schemaVersion = 1,
        .ok = true,
        .configFile = config_path,
    });
}

pub fn runConfigDiff(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
) !void {
    const config_path = globals.default_config_file;
    const local = std.Io.Dir.cwd().readFileAlloc(io, config_path, allocator, .limited(1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => try out.fail(allocator, io, opts.format, "missing .authdog/config.json; run `authdog config pull`"),
        else => return err,
    };
    defer allocator.free(local);

    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked = try requireLinked(allocator, io, opts.format, project_path);
    defer linked.deinit(allocator);

    const bundle = try fetchEnvironmentConfigBundle(allocator, io, opts.format, auth, linked);
    defer {
        allocator.free(bundle.session);
        allocator.free(bundle.redirect);
        allocator.free(bundle.webhooks);
        allocator.free(bundle.oidc);
    }
    const live = formatConfigDocument(allocator, bundle) catch {
        try out.fail(allocator, io, opts.format, "config response was not JSON");
    };
    defer allocator.free(live);

    const diff = diffConfigDocuments(allocator, local, live) catch {
        try out.fail(allocator, io, opts.format, "config file is not valid JSON");
    };
    try out.emit(allocator, io, opts.format, if (diff.matches) "Local config matches live." else "Local config differs from live.", .{
        .schemaVersion = 1,
        .ok = true,
        .matches = diff.matches,
        .sections = .{
            .sessionConfig = diff.sessionConfig,
            .redirectUris = diff.redirectUris,
            .webhooks = diff.webhooks,
            .oidcClients = diff.oidcClients,
        },
    });
}

pub fn runConfigPatch(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    patch: ConfigPatchOptions,
) !void {
    const config_path = globals.default_config_file;
    const local = std.Io.Dir.cwd().readFileAlloc(io, config_path, allocator, .limited(1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => try out.fail(allocator, io, opts.format, "missing .authdog/config.json; run `authdog config pull`"),
        else => return err,
    };
    defer allocator.free(local);

    const local_value = parseJsonValue(allocator, local) catch {
        try out.fail(allocator, io, opts.format, "config file is not valid JSON");
    };
    defer local_value.deinit();
    if (containsSecretKey(local_value.value)) {
        try out.fail(allocator, io, opts.format, "patch_refused");
    }

    const LocalFile = struct {
        sessionConfig: ?struct {
            config: ?std.json.Value = null,
        } = null,
        redirectUris: ?struct {
            config: ?struct {
                successRedirectUri: ?[]const u8 = null,
                errorRedirectUri: ?[]const u8 = null,
            } = null,
        } = null,
    };
    const parsed = try std.json.parseFromSlice(LocalFile, allocator, local, .{ .ignore_unknown_fields = true });
    defer parsed.deinit();

    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked = try requireLinked(allocator, io, opts.format, project_path);
    defer linked.deinit(allocator);

    const bundle = try fetchEnvironmentConfigBundle(allocator, io, opts.format, auth, linked);
    defer {
        allocator.free(bundle.session);
        allocator.free(bundle.redirect);
        allocator.free(bundle.webhooks);
        allocator.free(bundle.oidc);
    }
    const live = formatConfigDocument(allocator, bundle) catch {
        try out.fail(allocator, io, opts.format, "config response was not JSON");
    };
    defer allocator.free(live);
    const diff = diffConfigDocuments(allocator, local, live) catch {
        try out.fail(allocator, io, opts.format, "config file is not valid JSON");
    };
    if (!std.mem.eql(u8, diff.oidcClients, "match")) {
        try out.fail(allocator, io, opts.format, "patch_refused");
    }
    const sections = .{
        .sessionConfig = diff.sessionConfig,
        .redirectUris = diff.redirectUris,
        .webhooks = diff.webhooks,
        .oidcClients = diff.oidcClients,
    };
    if (patch.dry_run) {
        try out.emit(allocator, io, opts.format, "Dry run: would apply patches.", .{
            .schemaVersion = 1,
            .ok = true,
            .dryRun = true,
            .sections = sections,
        });
        return;
    }

    if (std.mem.eql(u8, diff.sessionConfig, "differ")) {
        if (parsed.value.sessionConfig) |session_wrap| {
            const config_obj = session_wrap.config orelse try out.fail(allocator, io, opts.format, "sessionConfig.config missing in local file");
            var body_buf: std.Io.Writer.Allocating = .init(allocator);
            defer body_buf.deinit();
            try body_buf.writer.print("{f}", .{std.json.fmt(config_obj, .{})});
            const path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/session-config", .{ linked.tenantId, linked.environmentId });
            defer allocator.free(path);
            const url = try api.joinUrl(allocator, auth.api_origin, path);
            defer allocator.free(url);
            const resp = try api.fetch(allocator, io, .PUT, url, auth.access_token, body_buf.written());
            defer allocator.free(resp.body);
            if (resp.status < 200 or resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, resp);
        }
    }

    if (std.mem.eql(u8, diff.redirectUris, "differ")) {
        if (parsed.value.redirectUris) |redirect_wrap| {
            const cfg = redirect_wrap.config orelse try out.fail(allocator, io, opts.format, "redirectUris.config missing in local file");
            const success = cfg.successRedirectUri orelse try out.fail(allocator, io, opts.format, "redirectUris.config.successRedirectUri missing");
            var body_buf: std.Io.Writer.Allocating = .init(allocator);
            defer body_buf.deinit();
            try body_buf.writer.writeAll("{\"successRedirectUri\":");
            try body_buf.writer.print("{f}", .{std.json.fmt(success, .{})});
            if (cfg.errorRedirectUri) |err_uri| {
                try body_buf.writer.writeAll(",\"errorRedirectUri\":");
                try body_buf.writer.print("{f}", .{std.json.fmt(err_uri, .{})});
            }
            try body_buf.writer.writeAll("}");
            const path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/redirect-uris", .{ linked.tenantId, linked.environmentId });
            defer allocator.free(path);
            const url = try api.joinUrl(allocator, auth.api_origin, path);
            defer allocator.free(url);
            const resp = try api.fetch(allocator, io, .PUT, url, auth.access_token, body_buf.written());
            defer allocator.free(resp.body);
            if (resp.status < 200 or resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, resp);
        }
    }

    try out.emit(allocator, io, opts.format, "Applied config patch from local file.", .{
        .schemaVersion = 1,
        .ok = true,
        .sections = sections,
    });
}

pub fn runWebhooksVerify(
    allocator: std.mem.Allocator,
    io: std.Io,
    opts: globals.GlobalOptions,
    verify: WebhooksVerifyOptions,
) !void {
    const valid = try verifyWebhookSignature(io, verify.secret, verify.body, verify.signature, verify.tolerance_seconds);
    try out.emit(allocator, io, opts.format, if (valid) "Signature valid." else "Signature invalid.", .{
        .schemaVersion = 1,
        .ok = true,
        .valid = valid,
    });
}

fn verifyWebhookSignature(io: std.Io, secret: []const u8, payload: []const u8, header: []const u8, tolerance: u32) !bool {
    var t: ?i64 = null;
    var v1: ?[]const u8 = null;
    var parts = std.mem.splitSequence(u8, header, ",");
    while (parts.next()) |part| {
        const trimmed = std.mem.trim(u8, part, " \t");
        if (std.mem.startsWith(u8, trimmed, "t=")) t = std.fmt.parseInt(i64, trimmed[2..], 10) catch null;
        if (std.mem.startsWith(u8, trimmed, "v1=")) v1 = trimmed[3..];
    }
    const timestamp = t orelse return false;
    const provided = v1 orelse return false;

    const now: i64 = std.Io.Clock.Timestamp.now(io, std.Io.Clock.real).raw.toSeconds();
    if (@abs(now - timestamp) > @as(i64, @intCast(tolerance))) return false;

    var msg_buf: [8192]u8 = undefined;
    const msg = std.fmt.bufPrint(&msg_buf, "{d}.{s}", .{ timestamp, payload }) catch return false;

    var mac: [32]u8 = undefined;
    std.crypto.auth.hmac.sha2.HmacSha256.create(&mac, msg, secret);

    var hex_buf: [64]u8 = undefined;
    const hex_len = hmacToHexLower(&mac, &hex_buf);
    const expected = hex_buf[0..hex_len];

    if (expected.len != provided.len) return false;
    var mismatch: u8 = 0;
    for (expected, 0..) |byte, index| mismatch |= byte ^ provided[index];
    return mismatch == 0;
}

fn hmacToHexLower(mac: *const [32]u8, hex_out: *[64]u8) usize {
    const hex = "0123456789abcdef";
    for (mac, 0..) |byte, index| {
        hex_out[index * 2] = hex[byte >> 4];
        hex_out[index * 2 + 1] = hex[byte & 0x0f];
    }
    return 64;
}

fn pollSleep(io: std.Io, interval_ms: u32) !void {
    const ns: i96 = @as(i96, interval_ms) * std.time.ns_per_ms;
    const d = std.Io.Clock.Duration{
        .raw = .{ .nanoseconds = ns },
        .clock = .boot,
    };
    try d.sleep(io);
}

pub fn runWebhooksListen(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    listen: WebhooksListenOptions,
) !void {
    if (listen.tunnel != null) {
        if (listen.channel_id == null) try out.fail(allocator, io, opts.format, "pass --channel-id with --tunnel");
        if (listen.restore_url == null) try out.fail(allocator, io, opts.format, "pass --restore-url; the API does not return the current webhook URL");
        if (!opts.yes) try out.fail(allocator, io, opts.format, "pass --yes to rewrite the webhook URL until this command exits");
        if (!webhook_rewrite.isChannelId(listen.channel_id.?)) try out.fail(allocator, io, opts.format, "invalid --channel-id");
        if (!webhook_rewrite.isHttpUrl(listen.tunnel.?) or !webhook_rewrite.isHttpUrl(listen.restore_url.?)) {
            try out.fail(allocator, io, opts.format, "webhook URL must be http or https");
        }
    }
    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked = try requireLinked(allocator, io, opts.format, project_path);
    defer linked.deinit(allocator);

    const path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/webhooks/deliveries?limit=20", .{ linked.tenantId, linked.environmentId });
    defer allocator.free(path);

    var tunnel_state = TunnelRewrite{
        .allocator = allocator,
        .io = io,
        .origin = auth.api_origin,
        .bearer = auth.access_token,
    };
    defer tunnel_state.restoreIfNeeded();
    if (listen.tunnel != null) {
        try tunnel_state.rewrite(opts, linked.tenantId, linked.environmentId, listen);
        armListenStop();
    }

    var seen: std.StringHashMap(void) = .init(allocator);
    defer {
        var keys = seen.keyIterator();
        while (keys.next()) |key| allocator.free(key.*);
        seen.deinit();
    }

    var forwarded: u32 = 0;
    var forwarded_with_body: u32 = 0;
    var polls: u32 = 0;
    const max_polls = if (listen.forward != null or listen.tunnel != null) listen.max_polls else 1;

    while (polls < max_polls and !listen_stop.load(.acquire)) : (polls += 1) {
        const resp = try api.getJson(allocator, io, auth.api_origin, path, auth.access_token);
        defer allocator.free(resp.body);
        if (resp.status < 200 or resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, resp);

        const parsed = std.json.parseFromSlice(std.json.Value, allocator, resp.body, .{}) catch continue;
        defer parsed.deinit();
        const data = objectField(parsed.value, "data") orelse continue;
        if (data != .array) continue;

        for (data.array.items) |item| {
            const id = jsonString(item, "id") orelse continue;
            if (seen.contains(id)) continue;
            try seen.put(try allocator.dupe(u8, id), {});

            if (listen.forward) |forward_url| {
                const shaped = try forwardDelivery(allocator, item);
                defer allocator.free(shaped.payload);
                const fwd = try api.postJson(allocator, io, forward_url, "", shaped.payload);
                defer allocator.free(fwd.body);
                if (fwd.status >= 200 and fwd.status < 300) {
                    forwarded += 1;
                    if (shaped.with_body) forwarded_with_body += 1;
                }
            }
        }

        if (polls + 1 < max_polls and !listen_stop.load(.acquire)) try pollSleep(io, listen.interval_ms);
    }

    const restored = tunnel_state.finish();
    if (listen.tunnel != null and !restored) {
        try out.fail(allocator, io, opts.format, "webhook URL was not restored; the endpoint still points at the tunnel");
    }

    if (listen.tunnel) |_| {
        try out.emit(allocator, io, opts.format, "Webhook delivery poll complete.", .{
            .schemaVersion = 1,
            .ok = true,
            .polls = polls,
            .trackedDeliveries = seen.count(),
            .forwarded = forwarded,
            .forwardedWithBody = forwarded_with_body,
            .forward = listen.forward,
            .channelId = listen.channel_id,
            .rewritten = true,
            .restored = true,
        });
    } else {
        try out.emit(allocator, io, opts.format, "Webhook delivery poll complete.", .{
            .schemaVersion = 1,
            .ok = true,
            .polls = polls,
            .trackedDeliveries = seen.count(),
            .forwarded = forwarded,
            .forwardedWithBody = forwarded_with_body,
            .forward = listen.forward,
        });
    }
}

const TunnelRewrite = struct {
    allocator: std.mem.Allocator,
    io: std.Io,
    origin: []const u8,
    bearer: []const u8,
    item_path: ?[]u8 = null,
    restore_body: ?[]u8 = null,
    needs_restore: bool = false,

    fn rewrite(
        self: *TunnelRewrite,
        opts: globals.GlobalOptions,
        tenant_id: []const u8,
        environment_id: []const u8,
        listen: WebhooksListenOptions,
    ) !void {
        const tunnel = listen.tunnel orelse return;
        const channel_id = listen.channel_id orelse try out.fail(self.allocator, self.io, opts.format, "pass --channel-id with --tunnel");
        const restore_url = listen.restore_url orelse try out.fail(self.allocator, self.io, opts.format, "pass --restore-url; the API does not return the current webhook URL");
        if (!opts.yes) try out.fail(self.allocator, self.io, opts.format, "pass --yes to rewrite the webhook URL until this command exits");
        if (!webhook_rewrite.isChannelId(channel_id)) try out.fail(self.allocator, self.io, opts.format, "invalid --channel-id");
        if (!webhook_rewrite.isHttpUrl(tunnel) or !webhook_rewrite.isHttpUrl(restore_url)) {
            try out.fail(self.allocator, self.io, opts.format, "webhook URL must be http or https");
        }

        const list_path = try std.fmt.allocPrint(self.allocator, "/v1/tenants/{s}/environments/{s}/webhooks", .{ tenant_id, environment_id });
        defer self.allocator.free(list_path);
        const listed = try api.getJson(self.allocator, self.io, self.origin, list_path, self.bearer);
        defer self.allocator.free(listed.body);
        if (listed.status < 200 or listed.status >= 300) try api.reportApiFailure(self.allocator, self.io, opts.format, listed);

        const tunnel_body = webhook_rewrite.updateBody(self.allocator, listed.body, channel_id, tunnel) catch |err| switch (err) {
            error.ChannelNotFound => try out.fail(self.allocator, self.io, opts.format, "webhook endpoint not found"),
            error.MissingName => try out.fail(self.allocator, self.io, opts.format, "webhook endpoint has no name"),
            error.MissingEnabled => try out.fail(self.allocator, self.io, opts.format, "webhook endpoint is missing enabled"),
            error.InvalidJson => try out.fail(self.allocator, self.io, opts.format, "webhook list was not valid JSON"),
            else => return err,
        };
        defer self.allocator.free(tunnel_body);
        self.restore_body = webhook_rewrite.updateBody(self.allocator, listed.body, channel_id, restore_url) catch |err| switch (err) {
            error.ChannelNotFound => try out.fail(self.allocator, self.io, opts.format, "webhook endpoint not found"),
            error.MissingName => try out.fail(self.allocator, self.io, opts.format, "webhook endpoint has no name"),
            error.MissingEnabled => try out.fail(self.allocator, self.io, opts.format, "webhook endpoint is missing enabled"),
            error.InvalidJson => try out.fail(self.allocator, self.io, opts.format, "webhook list was not valid JSON"),
            else => return err,
        };
        self.item_path = try std.fmt.allocPrint(self.allocator, "/v1/tenants/{s}/environments/{s}/webhooks/{s}", .{ tenant_id, environment_id, channel_id });
        const updated = try api.putJson(self.allocator, self.io, self.origin, self.item_path.?, self.bearer, tunnel_body);
        defer self.allocator.free(updated.body);
        if (updated.status < 200 or updated.status >= 300) try api.reportApiFailure(self.allocator, self.io, opts.format, updated);
        self.needs_restore = true;
    }

    fn finish(self: *TunnelRewrite) bool {
        if (!self.needs_restore) return true;
        return self.restore();
    }

    fn restoreIfNeeded(self: *TunnelRewrite) void {
        if (!self.needs_restore) {
            self.deinit();
            return;
        }
        _ = self.restore();
        self.deinit();
    }

    fn restore(self: *TunnelRewrite) bool {
        self.needs_restore = false;
        const path = self.item_path orelse return false;
        const body = self.restore_body orelse return false;
        const updated = api.putJson(self.allocator, self.io, self.origin, path, self.bearer, body) catch {
            out.errPrint(self.io, self.allocator, "webhook URL was not restored; the endpoint still points at the tunnel", .{}) catch {};
            return false;
        };
        defer self.allocator.free(updated.body);
        if (updated.status < 200 or updated.status >= 300) {
            out.errPrint(self.io, self.allocator, "webhook URL was not restored; the endpoint still points at the tunnel", .{}) catch {};
            return false;
        }
        return true;
    }

    fn deinit(self: *TunnelRewrite) void {
        if (self.item_path) |path| self.allocator.free(path);
        if (self.restore_body) |body| self.allocator.free(body);
        self.item_path = null;
        self.restore_body = null;
    }
};

var listen_stop: std.atomic.Value(bool) = .init(false);

const listen_handler = switch (builtin.os.tag) {
    .windows => struct {},
    else => struct {
        fn onStop(_: std.posix.SIG) callconv(.c) void {
            listen_stop.store(true, .release);
        }
    },
};

fn armListenStop() void {
    listen_stop.store(false, .release);
    switch (builtin.os.tag) {
        .windows => {},
        else => {
            const action = std.posix.Sigaction{
                .handler = .{ .handler = listen_handler.onStop },
                .mask = std.posix.sigemptyset(),
                .flags = 0,
            };
            std.posix.sigaction(std.posix.SIG.INT, &action, null);
            std.posix.sigaction(std.posix.SIG.TERM, &action, null);
        },
    }
}

pub fn runImpersonate(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    imp: ImpersonateOptions,
) !void {
    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked = try requireLinked(allocator, io, opts.format, project_path);
    defer linked.deinit(allocator);

    if (imp.revoke) {
        const grant_id = imp.grant_id orelse try out.fail(allocator, io, opts.format, "missing --grant-id");
        const grant_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/impersonation-grants/{s}/revoke", .{ linked.tenantId, linked.environmentId, grant_id });
        defer allocator.free(grant_path);
        const grant_url = try api.joinUrl(allocator, auth.api_origin, grant_path);
        defer allocator.free(grant_url);
        const grant_resp = try api.postJson(allocator, io, grant_url, auth.access_token, "{}");
        defer allocator.free(grant_resp.body);
        if (grant_resp.status < 200 or grant_resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, grant_resp);
        try out.emit(allocator, io, opts.format, "Impersonation grant revoked.", .{
            .schemaVersion = 1,
            .ok = true,
            .grantId = grant_id,
            .revoked = true,
        });
        return;
    }

    const user_id = imp.user_id orelse {
        try out.fail(allocator, io, opts.format, "missing --user-id");
    };
    if (opts.non_interactive and imp.create_grant and (imp.reason == null or imp.reason.?.len == 0)) {
        try out.fail(allocator, io, opts.format, "missing --reason");
    }

    const userinfo = try api.getJson(allocator, io, auth.api_origin, "/v1/userinfo", auth.access_token);
    defer allocator.free(userinfo.body);
    if (userinfo.status < 200 or userinfo.status >= 300) try api.reportApiFailure(allocator, io, opts.format, userinfo);

    const actor_id = try extractUserId(allocator, userinfo.body) orelse {
        try out.fail(allocator, io, opts.format, "could not resolve actor user id from userinfo");
    };
    defer allocator.free(actor_id);

    var grant_id: ?[]u8 = null;
    defer if (grant_id) |id| allocator.free(id);

    if (imp.create_grant) {
        const grant_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/impersonation-grants", .{ linked.tenantId, linked.environmentId });
        defer allocator.free(grant_path);
        const grant_url = try api.joinUrl(allocator, auth.api_origin, grant_path);
        defer allocator.free(grant_url);
        const reason = imp.reason orelse "CLI impersonation";
        var body_buf: std.Io.Writer.Allocating = .init(allocator);
        defer body_buf.deinit();
        try body_buf.writer.print("{f}", .{std.json.fmt(.{
            .actorUserId = actor_id,
            .targetUserId = user_id,
            .durationMinutes = imp.duration_minutes,
            .reason = reason,
        }, .{})});
        const grant_resp = try api.postJson(allocator, io, grant_url, auth.access_token, body_buf.written());
        defer allocator.free(grant_resp.body);
        if (grant_resp.status < 200 or grant_resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, grant_resp);
        grant_id = try extractGrantId(allocator, grant_resp.body);
    }

    const identity_url = try std.fmt.allocPrint(allocator, "{s}/api/v1/identity/{s}/impersonate", .{ linked.identityOrigin, linked.environmentId });
    defer allocator.free(identity_url);
    var imp_buf: std.Io.Writer.Allocating = .init(allocator);
    defer imp_buf.deinit();
    try imp_buf.writer.print("{f}", .{std.json.fmt(.{ .userId = user_id, .tenantId = linked.tenantId }, .{})});
    const imp_resp = try api.postJson(allocator, io, identity_url, auth.access_token, imp_buf.written());
    defer allocator.free(imp_resp.body);
    if (imp_resp.status < 200 or imp_resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, imp_resp);

    const shaped = try shapeImpersonation(allocator, imp_resp.body);
    defer if (shaped.sign_in_url) |url| allocator.free(url);
    if (shaped.sign_in_url) |url| {
        if (grant_id) |id| {
            try out.emit(allocator, io, opts.format, "Impersonation sign-in URL ready.", .{
                .schemaVersion = 1,
                .ok = true,
                .grantId = id,
                .signInUrl = url,
            });
        } else {
            try out.emit(allocator, io, opts.format, "Impersonation sign-in URL ready.", .{
                .schemaVersion = 1,
                .ok = true,
                .signInUrl = url,
            });
        }
    } else {
        const instructions = shaped.instructions.?;
        if (grant_id) |id| {
            try out.emit(allocator, io, opts.format, instructions, .{
                .schemaVersion = 1,
                .ok = true,
                .grantId = id,
                .instructions = instructions,
            });
        } else {
            try out.emit(allocator, io, opts.format, instructions, .{
                .schemaVersion = 1,
                .ok = true,
                .instructions = instructions,
            });
        }
    }
}

fn extractUserId(allocator: std.mem.Allocator, body: []const u8) !?[]const u8 {
    const parsed = try std.json.parseFromSlice(struct {
        user: ?struct { id: ?[]const u8 = null } = null,
    }, allocator, body, .{ .ignore_unknown_fields = true });
    defer parsed.deinit();
    if (parsed.value.user) |user| {
        if (user.id) |id| if (id.len != 0) return try allocator.dupe(u8, id);
    }
    return null;
}

pub fn runDeployStatus(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
) !void {
    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked = try requireLinked(allocator, io, opts.format, project_path);
    defer linked.deinit(allocator);

    const posture_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/security/posture", .{ linked.tenantId, linked.environmentId });
    defer allocator.free(posture_path);
    const vanity_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/vanity-domains", .{ linked.tenantId, linked.environmentId });
    defer allocator.free(vanity_path);
    const redirect_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/applications/{s}/environments/{s}/redirect-uris", .{ linked.tenantId, linked.applicationId, linked.environmentId });
    defer allocator.free(redirect_path);
    const webhooks_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/webhooks", .{ linked.tenantId, linked.environmentId });
    defer allocator.free(webhooks_path);

    const Readiness = struct {
        name: []const u8,
        path: []const u8,
        ok_detail: []const u8,
    };
    const rows = [_]Readiness{
        .{ .name = "security-posture", .path = posture_path, .ok_detail = "score available" },
        .{ .name = "vanity-domains", .path = vanity_path, .ok_detail = "listed" },
        .{ .name = "redirects", .path = redirect_path, .ok_detail = "listed" },
        .{ .name = "webhooks", .path = webhooks_path, .ok_detail = "listed" },
    };
    const Check = struct { name: []const u8, ok: bool, detail: []const u8 };
    var checks: [4]Check = undefined;
    for (rows, 0..) |row, index| {
        const resp = try api.getJson(allocator, io, auth.api_origin, row.path, auth.access_token);
        defer allocator.free(resp.body);
        if (resp.status == 401) try api.reportApiFailure(allocator, io, opts.format, resp);
        const ok = resp.status >= 200 and resp.status < 300;
        checks[index] = .{
            .name = row.name,
            .ok = ok,
            .detail = if (ok) row.ok_detail else "request failed",
        };
    }

    try out.emit(allocator, io, opts.format, "Deploy readiness snapshot.", .{
        .schemaVersion = 1,
        .ok = true,
        .checks = checks,
    });
}

pub fn runDoctor(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    doctor: DoctorOptions,
) !void {
    var checks: std.ArrayList(agent.DoctorCheck) = .empty;
    defer {
        for (checks.items) |c| allocator.free(c.detail);
        checks.deinit(allocator);
    }

    try checks.append(allocator, .{ .name = "cli_version", .ok = true, .detail = @import("build_options").version });

    const logged_in = (try session.loadSession(allocator, io, environ)) != null;
    try checks.append(allocator, .{
        .name = "credentials",
        .ok = logged_in,
        .detail = if (logged_in) try allocator.dupe(u8, "session present") else try allocator.dupe(u8, "run authdog login"),
    });

    const health = try api.getJson(allocator, io, opts.api_origin, "/v1/health", "");
    defer allocator.free(health.body);
    try checks.append(allocator, .{
        .name = "api_health",
        .ok = health.status == 200,
        .detail = try std.fmt.allocPrint(allocator, "HTTP {d}", .{health.status}),
    });

    if (logged_in) {
        const auth = try auth_session.requireAuth(allocator, io, environ, opts.api_origin);
        defer auth_session.deinitAuth(allocator, auth);
        const userinfo = try api.getJson(allocator, io, auth.api_origin, "/v1/userinfo", auth.access_token);
        defer allocator.free(userinfo.body);
        try checks.append(allocator, .{
            .name = "userinfo",
            .ok = userinfo.status >= 200 and userinfo.status < 300,
            .detail = try std.fmt.allocPrint(allocator, "HTTP {d}", .{userinfo.status}),
        });
    }

    if (doctor.check_mcp) {
        const mcp_url = try api.joinUrl(allocator, opts.api_origin, "/.well-known/oauth-protected-resource/mcp");
        defer allocator.free(mcp_url);
        const mcp = try api.fetch(allocator, io, .GET, mcp_url, null, null);
        defer allocator.free(mcp.body);
        try checks.append(allocator, .{
            .name = "mcp_metadata",
            .ok = mcp.status >= 200 and mcp.status < 300,
            .detail = try std.fmt.allocPrint(allocator, "HTTP {d}", .{mcp.status}),
        });
    }

    if (doctor.check_agent_sign_in) {
        try agent.appendAgentSignInDoctorChecks(allocator, io, environ, opts, &checks);
    }

    const all_ok = blk: {
        for (checks.items) |c| {
            if (!c.ok) break :blk false;
        }
        break :blk true;
    };

    if (opts.format == .json) {
        try out.emit(allocator, io, .json, "", .{ .schemaVersion = 1, .ok = all_ok, .checks = checks.items });
        return;
    }

    for (checks.items) |c| {
        const mark = if (c.ok) "ok" else "FAIL";
        try out.print(io, std.Io.File.stdout(), "[{s}] {s}: {s}\n", .{ mark, c.name, c.detail });
    }
}

pub fn runMcp(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    sub: McpCommand,
) !void {
    const paths = mcpOsPaths(environ) catch {
        try out.fail(allocator, io, opts.format, "HOME is not set");
    };
    const mcp_url = try api.joinUrl(allocator, opts.api_origin, "/mcp");
    defer allocator.free(mcp_url);

    if (sub.action == .list) {
        try listMcpClients(allocator, io, opts, paths, sub, mcp_url);
        return;
    }

    const multi = sub.all or (sub.client != null and sub.client.? != .cursor);
    if (!multi) {
        const config_path = try mcp_config.configPath(allocator, paths, .cursor);
        defer allocator.free(config_path);
        switch (sub.action) {
            .list => unreachable,
            .install => {
                const written = upsertCursorMcpConfig(allocator, io, config_path, mcp_url) catch |err| switch (err) {
                    error.InvalidJson => try out.fail(allocator, io, opts.format, "mcp config is not valid JSON"),
                    error.StreamTooLong => try out.fail(allocator, io, opts.format, "mcp config is too large"),
                    else => return err,
                };
                defer freeNameList(allocator, written.preserved);
                try out.emit(allocator, io, opts.format, "Registered Authdog MCP in Cursor config.", .{
                    .schemaVersion = 1,
                    .ok = true,
                    .configPath = config_path,
                    .serverUrl = mcp_url,
                    .preservedServers = written.preserved,
                });
            },
            .uninstall => {
                const written = removeCursorMcpConfig(allocator, io, config_path) catch |err| switch (err) {
                    error.InvalidJson => try out.fail(allocator, io, opts.format, "mcp config is not valid JSON"),
                    error.StreamTooLong => try out.fail(allocator, io, opts.format, "mcp config is too large"),
                    else => return err,
                };
                defer freeNameList(allocator, written.preserved);
                try out.emit(allocator, io, opts.format, "Removed Authdog MCP from Cursor config.", .{
                    .schemaVersion = 1,
                    .ok = true,
                    .configPath = config_path,
                    .removed = written.removed,
                    .preservedServers = written.preserved,
                });
            },
        }
        return;
    }

    try applyMcpClients(allocator, io, opts, paths, sub, mcp_url);
}

fn mcpConfigExists(io: std.Io, path: []const u8) !bool {
    const stat = std.Io.Dir.cwd().statFile(io, path, .{}) catch |err| switch (err) {
        error.FileNotFound => return false,
        else => return err,
    };
    _ = stat;
    return true;
}

const McpWrite = struct {
    preserved: [][]u8,
    removed: bool = false,
};

fn freeNameList(allocator: std.mem.Allocator, names: [][]u8) void {
    for (names) |name| allocator.free(name);
    allocator.free(names);
}

fn upsertCursorMcpConfig(
    allocator: std.mem.Allocator,
    io: std.Io,
    path: []const u8,
    url: []const u8,
) !McpWrite {
    const written = try editMcpFile(allocator, io, .cursor, path, url, false);
    defer allocator.free(written.doc);
    return .{ .preserved = written.preserved };
}

fn removeCursorMcpConfig(
    allocator: std.mem.Allocator,
    io: std.Io,
    path: []const u8,
) !McpWrite {
    const written = try editMcpFile(allocator, io, .cursor, path, "", true);
    defer allocator.free(written.doc);
    return .{ .preserved = written.preserved, .removed = written.removed };
}

const McpSlot = struct {
    name: []const u8,
    path: []u8,
    doc: ?[]u8 = null,
    preserved: [][]u8,
    status: []const u8,
    removed: bool = false,
    write: bool = false,

    fn deinit(self: *McpSlot, allocator: std.mem.Allocator) void {
        allocator.free(self.path);
        if (self.doc) |doc| allocator.free(doc);
        freeNameList(allocator, self.preserved);
    }
};

fn mcpOsPaths(environ: *const std.process.Environ.Map) !mcp_config.OsPaths {
    const home = environ.get("HOME") orelse return error.NoHome;
    if (home.len == 0) return error.NoHome;
    const config_home = environ.get("XDG_CONFIG_HOME") orelse "";
    return .{
        .home = home,
        .config_home = if (config_home.len != 0) config_home else home,
        .appdata = environ.get("APPDATA"),
    };
}

fn resolvedConfigHome(allocator: std.mem.Allocator, paths: mcp_config.OsPaths) ![]u8 {
    if (environConfigIsExplicit(paths)) return allocator.dupe(u8, paths.config_home);
    return std.fs.path.join(allocator, &.{ paths.home, ".config" });
}

fn environConfigIsExplicit(paths: mcp_config.OsPaths) bool {
    return paths.config_home.len != 0 and !std.mem.eql(u8, paths.config_home, paths.home);
}

fn listMcpClients(
    allocator: std.mem.Allocator,
    io: std.Io,
    opts: globals.GlobalOptions,
    paths: mcp_config.OsPaths,
    sub: McpCommand,
    mcp_url: []const u8,
) !void {
    const config_home = try resolvedConfigHome(allocator, paths);
    defer allocator.free(config_home);
    const os_paths = mcp_config.OsPaths{
        .home = paths.home,
        .config_home = config_home,
        .appdata = paths.appdata,
    };
    const selected = selectedClients(sub);
    const Listed = struct {
        name: []const u8,
        configPath: []const u8,
        installed: bool,
        authdog: bool,
        readable: bool,
    };
    var rows: [mcp_config.all_clients.len]Listed = undefined;
    var paths_owned: [mcp_config.all_clients.len][]u8 = undefined;
    var count: usize = 0;
    defer {
        for (paths_owned[0..count]) |path| allocator.free(path);
    }
    for (selected) |client| {
        const path = try mcp_config.configPath(allocator, os_paths, client);
        paths_owned[count] = path;
        const installed = mcp_config.isInstalled(allocator, io, os_paths, client) catch false;
        var authdog = false;
        var readable = true;
        if (mcp_config.fileExists(io, path)) {
            const bytes = readMcpFile(allocator, io, path) catch |err| switch (err) {
                error.StreamTooLong => blk: {
                    readable = false;
                    break :blk null;
                },
                else => return err,
            };
            if (bytes) |body| {
                defer allocator.free(body);
                const presence = mcp_config.inspect(allocator, client, body);
                readable = presence.readable;
                authdog = presence.authdog;
            }
        }
        rows[count] = .{
            .name = mcp_config.clientName(client),
            .configPath = path,
            .installed = installed,
            .authdog = authdog,
            .readable = readable,
        };
        count += 1;
    }
    const cursor_path = rows[0].configPath;
    const exists = mcp_config.fileExists(io, cursor_path);
    var text: std.Io.Writer.Allocating = .init(allocator);
    defer text.deinit();
    for (rows[0..count]) |row| {
        const mark = if (!row.installed) "not installed" else if (!row.readable) "unreadable" else if (row.authdog) "authdog" else "absent";
        try text.writer.print("{s}\t{s}\t{s}\n", .{ row.name, row.configPath, mark });
    }
    try out.emit(allocator, io, opts.format, text.written(), .{
        .schemaVersion = 1,
        .ok = true,
        .configPath = cursor_path,
        .exists = exists,
        .serverUrl = mcp_url,
        .clients = rows[0..count],
    });
}

fn selectedClients(sub: McpCommand) []const mcp_config.Client {
    if (sub.client) |client| {
        return switch (client) {
            .cursor => mcp_config.all_clients[0..1],
            .claude => mcp_config.all_clients[1..2],
            .vscode => mcp_config.all_clients[2..3],
            .codex => mcp_config.all_clients[3..4],
        };
    }
    return &mcp_config.all_clients;
}

fn applyMcpClients(
    allocator: std.mem.Allocator,
    io: std.Io,
    opts: globals.GlobalOptions,
    paths: mcp_config.OsPaths,
    sub: McpCommand,
    mcp_url: []const u8,
) !void {
    const config_home = try resolvedConfigHome(allocator, paths);
    defer allocator.free(config_home);
    const os_paths = mcp_config.OsPaths{
        .home = paths.home,
        .config_home = config_home,
        .appdata = paths.appdata,
    };
    var slots: [mcp_config.all_clients.len]McpSlot = undefined;
    var count: usize = 0;
    errdefer {
        for (slots[0..count]) |*slot| slot.deinit(allocator);
    }
    const remove = sub.action == .uninstall;
    for (mcp_config.all_clients) |client| {
        const chosen = sub.all or (sub.client != null and sub.client.? == client);
        if (!chosen) continue;
        const path = mcp_config.configPath(allocator, os_paths, client) catch {
            try out.fail(allocator, io, opts.format, "HOME is not set");
        };
        const installed = mcp_config.isInstalled(allocator, io, os_paths, client) catch false;
        if (sub.all and !installed) {
            slots[count] = .{
                .name = mcp_config.clientName(client),
                .path = path,
                .preserved = try allocator.alloc([]u8, 0),
                .status = "skipped",
            };
            count += 1;
            continue;
        }
        const edited = planMcpEdit(allocator, io, client, path, mcp_url, remove) catch |err| switch (err) {
            error.InvalidJson => {
                allocator.free(path);
                try out.fail(allocator, io, opts.format, "mcp config is not valid JSON");
            },
            error.InvalidConfig => {
                allocator.free(path);
                try out.fail(allocator, io, opts.format, "mcp config is not valid");
            },
            error.StreamTooLong => {
                allocator.free(path);
                try out.fail(allocator, io, opts.format, "mcp config is too large");
            },
            else => {
                allocator.free(path);
                return err;
            },
        };
        const write = !remove or edited.removed;
        slots[count] = .{
            .name = mcp_config.clientName(client),
            .path = path,
            .doc = edited.doc,
            .preserved = edited.preserved,
            .status = if (remove) (if (edited.removed) "removed" else "unchanged") else "installed",
            .removed = edited.removed,
            .write = write,
        };
        count += 1;
    }

    var actionable: usize = 0;
    for (slots[0..count]) |slot| {
        if (!std.mem.eql(u8, slot.status, "skipped")) actionable += 1;
    }
    if (actionable == 0) try out.fail(allocator, io, opts.format, "no MCP clients detected; pass --client");

    for (slots[0..count]) |slot| {
        if (!slot.write) continue;
        const mode = try permissionsFor(io, slot.path);
        try writeConfigFileMode(io, slot.path, slot.doc.?, mode);
    }

    const Row = struct {
        name: []const u8,
        configPath: []const u8,
        status: []const u8,
        preservedServers: []const []const u8,
        removed: bool,
    };
    var rows: [mcp_config.all_clients.len]Row = undefined;
    for (slots[0..count], 0..) |slot, index| {
        rows[index] = .{
            .name = slot.name,
            .configPath = slot.path,
            .status = slot.status,
            .preservedServers = slot.preserved,
            .removed = slot.removed,
        };
    }
    var text: std.Io.Writer.Allocating = .init(allocator);
    defer text.deinit();
    for (rows[0..count]) |row| {
        try text.writer.print("{s}: {s} {s}\n", .{ row.name, row.status, row.configPath });
    }
    try out.emit(allocator, io, opts.format, text.written(), .{
        .schemaVersion = 1,
        .ok = true,
        .serverUrl = mcp_url,
        .clients = rows[0..count],
    });
    for (slots[0..count]) |*slot| slot.deinit(allocator);
    count = 0;
}

fn planMcpEdit(
    allocator: std.mem.Allocator,
    io: std.Io,
    client: mcp_config.Client,
    path: []const u8,
    url: []const u8,
    remove: bool,
) !mcp_config.Edit {
    const existing = try readMcpFile(allocator, io, path);
    defer if (existing) |bytes| allocator.free(bytes);
    if (remove and (existing == null or existing.?.len == 0)) {
        return .{ .doc = try allocator.alloc(u8, 0), .preserved = try allocator.alloc([]u8, 0) };
    }
    const source = if (existing) |bytes| if (bytes.len == 0) blankConfig(client) else bytes else blankConfig(client);
    if (client == .codex) {
        return if (remove)
            try mcp_config.stripToml(allocator, source)
        else
            try mcp_config.mergeToml(allocator, source, url);
    }
    const shape = mcp_config.jsonShape(client).?;
    return if (remove)
        try mcp_config.stripJson(allocator, source, shape)
    else
        try mcp_config.mergeJson(allocator, source, url, shape);
}

fn editMcpFile(
    allocator: std.mem.Allocator,
    io: std.Io,
    client: mcp_config.Client,
    path: []const u8,
    url: []const u8,
    remove: bool,
) !mcp_config.Edit {
    const edited = try planMcpEdit(allocator, io, client, path, url, remove);
    if (!remove or edited.removed) {
        const mode = try permissionsFor(io, path);
        writeConfigFileMode(io, path, edited.doc, mode) catch |err| {
            mcp_config.freeNames(allocator, edited.preserved);
            allocator.free(edited.doc);
            return err;
        };
    }
    return edited;
}

fn blankConfig(client: mcp_config.Client) []const u8 {
    return if (client == .codex) "" else "{}";
}

fn readMcpFile(allocator: std.mem.Allocator, io: std.Io, path: []const u8) !?[]u8 {
    return std.Io.Dir.cwd().readFileAlloc(io, path, allocator, .limited(8 * 1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => null,
        else => |e| return e,
    };
}

fn permissionsFor(io: std.Io, path: []const u8) !std.Io.File.Permissions {
    const stat = std.Io.Dir.cwd().statFile(io, path, .{}) catch |err| switch (err) {
        error.FileNotFound => return fs_util.privateFileMode(),
        else => |e| return e,
    };
    return stat.permissions;
}

fn writeConfigFileMode(io: std.Io, path: []const u8, data: []const u8, permissions: std.Io.File.Permissions) !void {
    if (std.fs.path.dirname(path)) |dir| try std.Io.Dir.cwd().createDirPath(io, dir);
    try std.Io.Dir.cwd().writeFile(io, .{
        .sub_path = path,
        .data = data,
        .flags = .{ .permissions = permissions },
    });
}

fn readOptionalFile(allocator: std.mem.Allocator, io: std.Io, path: []const u8) !?[]u8 {
    return std.Io.Dir.cwd().readFileAlloc(io, path, allocator, .limited(1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => null,
        else => err,
    };
}

fn writeConfigFile(io: std.Io, path: []const u8, data: []const u8) !void {
    if (std.fs.path.dirname(path)) |dir| try std.Io.Dir.cwd().createDirPath(io, dir);
    try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = path, .data = data });
}

const ServerEdit = struct {
    doc: []u8,
    preserved: [][]u8,
    removed: bool = false,
};

fn parseJsonValue(allocator: std.mem.Allocator, bytes: []const u8) !std.json.Parsed(std.json.Value) {
    return std.json.parseFromSlice(std.json.Value, allocator, bytes, .{}) catch return error.InvalidJson;
}

fn objectField(value: std.json.Value, name: []const u8) ?std.json.Value {
    return switch (value) {
        .object => |map| map.get(name),
        else => null,
    };
}

fn jsonString(value: std.json.Value, name: []const u8) ?[]const u8 {
    const field = objectField(value, name) orelse return null;
    return switch (field) {
        .string => |text| text,
        else => null,
    };
}

fn isSecretKey(name: []const u8) bool {
    return std.mem.eql(u8, name, "signingSecret") or
        std.mem.eql(u8, name, "secret") or
        std.mem.eql(u8, name, "clientSecret") or
        std.mem.eql(u8, name, "token");
}

fn redactSecrets(allocator: std.mem.Allocator, value: *std.json.Value) !void {
    switch (value.*) {
        .object => |*map| {
            var remove_keys: std.ArrayList([]const u8) = .empty;
            defer remove_keys.deinit(allocator);
            var it = map.iterator();
            while (it.next()) |entry| {
                if (isSecretKey(entry.key_ptr.*)) {
                    try remove_keys.append(allocator, entry.key_ptr.*);
                } else {
                    try redactSecrets(allocator, entry.value_ptr);
                }
            }
            for (remove_keys.items) |key| _ = map.orderedRemove(key);
        },
        .array => |*items| {
            for (items.items) |*item| try redactSecrets(allocator, item);
        },
        else => {},
    }
}

fn containsSecretKey(value: std.json.Value) bool {
    switch (value) {
        .object => |map| {
            var it = map.iterator();
            while (it.next()) |entry| {
                if (isSecretKey(entry.key_ptr.*)) return true;
                if (containsSecretKey(entry.value_ptr.*)) return true;
            }
            return false;
        },
        .array => |items| {
            for (items.items) |item| if (containsSecretKey(item)) return true;
            return false;
        },
        else => return false,
    }
}

fn jsonEqual(a: std.json.Value, b: std.json.Value) bool {
    if (std.meta.activeTag(a) != std.meta.activeTag(b)) return false;
    return switch (a) {
        .null => true,
        .bool => |value| value == b.bool,
        .integer => |value| value == b.integer,
        .float => |value| value == b.float,
        .number_string => |value| std.mem.eql(u8, value, b.number_string),
        .string => |value| std.mem.eql(u8, value, b.string),
        .array => |items| blk: {
            if (items.items.len != b.array.items.len) break :blk false;
            for (items.items, b.array.items) |left, right| {
                if (!jsonEqual(left, right)) break :blk false;
            }
            break :blk true;
        },
        .object => |map| blk: {
            if (map.count() != b.object.count()) break :blk false;
            var it = map.iterator();
            while (it.next()) |entry| {
                const other = b.object.get(entry.key_ptr.*) orelse break :blk false;
                if (!jsonEqual(entry.value_ptr.*, other)) break :blk false;
            }
            break :blk true;
        },
    };
}

fn sectionWord(local: std.json.Value, live: std.json.Value, name: []const u8) []const u8 {
    const left = objectField(local, name);
    const right = objectField(live, name);
    if (left == null and right == null) return "match";
    if (left == null or right == null) return "differ";
    return if (jsonEqual(left.?, right.?)) "match" else "differ";
}

fn diffConfigDocuments(allocator: std.mem.Allocator, local_bytes: []const u8, live_bytes: []const u8) !SectionDiff {
    const local = try parseJsonValue(allocator, local_bytes);
    defer local.deinit();
    const live = try parseJsonValue(allocator, live_bytes);
    defer live.deinit();
    const sessionConfig = sectionWord(local.value, live.value, "sessionConfig");
    const redirectUris = sectionWord(local.value, live.value, "redirectUris");
    const webhooks = sectionWord(local.value, live.value, "webhooks");
    const oidcClients = sectionWord(local.value, live.value, "oidcClients");
    const matches = std.mem.eql(u8, sessionConfig, "match") and
        std.mem.eql(u8, redirectUris, "match") and
        std.mem.eql(u8, webhooks, "match") and
        std.mem.eql(u8, oidcClients, "match");
    return .{
        .matches = matches,
        .sessionConfig = sessionConfig,
        .redirectUris = redirectUris,
        .webhooks = webhooks,
        .oidcClients = oidcClients,
    };
}

const DeliveryForward = struct {
    payload: []u8,
    with_body: bool,
};

fn forwardDelivery(allocator: std.mem.Allocator, item: std.json.Value) !DeliveryForward {
    if (jsonString(item, "body")) |body| {
        return .{ .payload = try allocator.dupe(u8, body), .with_body = true };
    }
    if (jsonString(item, "payload")) |body| {
        return .{ .payload = try allocator.dupe(u8, body), .with_body = true };
    }
    const payload = try std.fmt.allocPrint(allocator, "{f}", .{std.json.fmt(.{
        .deliveryId = jsonString(item, "id") orelse "",
        .eventType = jsonString(item, "eventType") orelse "",
        .payloadAvailable = false,
    }, .{})});
    return .{ .payload = payload, .with_body = false };
}

const ImpersonationShape = struct {
    sign_in_url: ?[]u8 = null,
    instructions: ?[]const u8 = null,
};

fn shapeImpersonation(allocator: std.mem.Allocator, body: []const u8) !ImpersonationShape {
    const parsed = parseJsonValue(allocator, body) catch {
        return .{ .instructions = "Complete sign-in in the app callback. The impersonation token is not printed." };
    };
    defer parsed.deinit();
    const token = jsonString(parsed.value, "token");
    const redirect = jsonString(parsed.value, "redirectTo");
    if (redirect) |target| if (token) |value| {
        const sep: []const u8 = if (std.mem.indexOfScalar(u8, target, '?') != null) "&" else "?";
        return .{
            .sign_in_url = try std.fmt.allocPrint(allocator, "{s}{s}token={s}", .{ target, sep, value }),
        };
    };
    return .{ .instructions = "Complete sign-in in the app callback. The impersonation token is not printed." };
}

fn extractGrantId(allocator: std.mem.Allocator, body: []const u8) !?[]u8 {
    const parsed = parseJsonValue(allocator, body) catch return null;
    defer parsed.deinit();
    if (objectField(parsed.value, "grant")) |grant| {
        if (jsonString(grant, "id")) |id| if (id.len != 0) return try allocator.dupe(u8, id);
    }
    if (jsonString(parsed.value, "grantId")) |id| if (id.len != 0) return try allocator.dupe(u8, id);
    if (jsonString(parsed.value, "id")) |id| if (id.len != 0) return try allocator.dupe(u8, id);
    return null;
}

fn preservedNames(allocator: std.mem.Allocator, servers: std.json.ObjectMap) ![][]u8 {
    var list: std.ArrayList([]u8) = .empty;
    errdefer {
        for (list.items) |item| allocator.free(item);
        list.deinit(allocator);
    }
    var it = servers.iterator();
    while (it.next()) |entry| {
        if (std.mem.eql(u8, entry.key_ptr.*, "authdog")) continue;
        try list.append(allocator, try allocator.dupe(u8, entry.key_ptr.*));
    }
    return list.toOwnedSlice(allocator);
}

fn mergeAuthdogServer(allocator: std.mem.Allocator, existing: []const u8, url: []const u8) !ServerEdit {
    const parsed = try parseJsonValue(allocator, existing);
    defer parsed.deinit();
    if (parsed.value != .object) return error.InvalidJson;
    var root = parsed.value;
    const gpa = parsed.arena.allocator();
    var servers = std.json.ObjectMap.empty;
    if (objectField(root, "mcpServers")) |current| {
        if (current != .object) return error.InvalidJson;
        servers = current.object;
    }
    const url_copy = try gpa.dupe(u8, url);
    var authdog_server = std.json.ObjectMap.empty;
    try authdog_server.put(gpa, "url", .{ .string = url_copy });
    try servers.put(gpa, "authdog", .{ .object = authdog_server });
    try root.object.put(gpa, "mcpServers", .{ .object = servers });

    const preserved = try preservedNames(allocator, servers);
    errdefer freeNameList(allocator, preserved);
    var doc: std.Io.Writer.Allocating = .init(allocator);
    errdefer doc.deinit();
    try doc.writer.print("{f}\n", .{std.json.fmt(root, .{ .whitespace = .indent_2 })});
    return .{ .doc = try doc.toOwnedSlice(), .preserved = preserved };
}

fn stripAuthdogServer(allocator: std.mem.Allocator, existing: []const u8) !ServerEdit {
    const parsed = try parseJsonValue(allocator, existing);
    defer parsed.deinit();
    if (parsed.value != .object) return error.InvalidJson;
    var root = parsed.value;
    const gpa = parsed.arena.allocator();
    var removed = false;
    var servers = std.json.ObjectMap.empty;
    if (objectField(root, "mcpServers")) |current| {
        if (current != .object) return error.InvalidJson;
        servers = current.object;
        removed = servers.orderedRemove("authdog");
        try root.object.put(gpa, "mcpServers", .{ .object = servers });
    }
    const preserved = try preservedNames(allocator, servers);
    errdefer freeNameList(allocator, preserved);
    var doc: std.Io.Writer.Allocating = .init(allocator);
    errdefer doc.deinit();
    try doc.writer.print("{f}\n", .{std.json.fmt(root, .{ .whitespace = .indent_2 })});
    return .{ .doc = try doc.toOwnedSlice(), .preserved = preserved, .removed = removed };
}

test "config diff names the changed section and ignores key order" {
    const local =
        \\{"schemaVersion":1,"sessionConfig":{"a":1},"redirectUris":{"b":2},"webhooks":{},"oidcClients":{}}
    ;
    const reordered =
        \\{"oidcClients":{},"webhooks":{},"redirectUris":{"b":2},"sessionConfig":{"a":1}}
    ;
    const same = try diffConfigDocuments(std.testing.allocator, local, reordered);
    try std.testing.expect(same.matches);
    try std.testing.expectEqualStrings("match", same.sessionConfig);

    const changed =
        \\{"sessionConfig":{"a":1},"redirectUris":{"b":9},"webhooks":{},"oidcClients":{}}
    ;
    const diff = try diffConfigDocuments(std.testing.allocator, local, changed);
    try std.testing.expect(!diff.matches);
    try std.testing.expectEqualStrings("match", diff.sessionConfig);
    try std.testing.expectEqualStrings("differ", diff.redirectUris);
    try std.testing.expectEqualStrings("match", diff.webhooks);
    try std.testing.expectEqualStrings("match", diff.oidcClients);
}

test "config document redacts secrets and keeps oidc clients" {
    const doc = try formatConfigDocument(std.testing.allocator, .{
        .session = "{}",
        .redirect = "{}",
        .webhooks = "{\"id\":\"1\",\"signingSecret\":\"whsec_secret\"}",
        .oidc = "{\"clients\":[{\"clientId\":\"abc\",\"clientSecret\":\"sekret\"}]}",
    });
    defer std.testing.allocator.free(doc);
    try std.testing.expect(std.mem.indexOf(u8, doc, "whsec_secret") == null);
    try std.testing.expect(std.mem.indexOf(u8, doc, "sekret") == null);
    try std.testing.expect(std.mem.indexOf(u8, doc, "signingSecret") == null);
    try std.testing.expect(std.mem.indexOf(u8, doc, "oidcClients") != null);
    try std.testing.expect(std.mem.indexOf(u8, doc, "abc") != null);
}

test "listen forward is honest about a missing body" {
    const missing = try parseJsonValue(std.testing.allocator, "{\"id\":\"d1\",\"eventType\":\"LOGIN\"}");
    defer missing.deinit();
    const empty = try forwardDelivery(std.testing.allocator, missing.value);
    defer std.testing.allocator.free(empty.payload);
    try std.testing.expect(!empty.with_body);
    try std.testing.expect(std.mem.indexOf(u8, empty.payload, "payloadAvailable") != null);
    try std.testing.expect(std.mem.indexOf(u8, empty.payload, "d1") != null);

    const present = try parseJsonValue(std.testing.allocator, "{\"id\":\"d2\",\"body\":\"{\\\"ok\\\":true}\"}");
    defer present.deinit();
    const forwarded = try forwardDelivery(std.testing.allocator, present.value);
    defer std.testing.allocator.free(forwarded.payload);
    try std.testing.expect(forwarded.with_body);
    try std.testing.expectEqualStrings("{\"ok\":true}", forwarded.payload);
}

test "impersonation url omits a token field" {
    const shaped = try shapeImpersonation(std.testing.allocator, "{\"token\":\"sekret\",\"redirectTo\":\"https://app.example/cb\"}");
    defer if (shaped.sign_in_url) |url| std.testing.allocator.free(url);
    try std.testing.expect(std.mem.startsWith(u8, shaped.sign_in_url.?, "https://app.example/cb?token="));
    var rendered: std.Io.Writer.Allocating = .init(std.testing.allocator);
    defer rendered.deinit();
    try rendered.writer.print("{f}", .{std.json.fmt(.{ .signInUrl = shaped.sign_in_url.? }, .{})});
    try std.testing.expect(std.mem.indexOf(u8, rendered.written(), "\"token\":") == null);

    const hidden = try shapeImpersonation(std.testing.allocator, "{\"token\":\"sekret\"}");
    try std.testing.expect(hidden.sign_in_url == null);
    try std.testing.expect(std.mem.indexOf(u8, hidden.instructions.?, "sekret") == null);
}

test "mcp merge keeps other servers and uninstall removes only authdog" {
    const merged = try mergeAuthdogServer(
        std.testing.allocator,
        "{\"mcpServers\":{\"other\":{\"url\":\"http://example\"}}}",
        "https://api.authdog.com/mcp",
    );
    defer std.testing.allocator.free(merged.doc);
    defer freeNameList(std.testing.allocator, merged.preserved);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "\"other\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "\"authdog\"") != null);
    try std.testing.expectEqual(@as(usize, 1), merged.preserved.len);
    try std.testing.expectEqualStrings("other", merged.preserved[0]);

    try std.testing.expectError(error.InvalidJson, mergeAuthdogServer(std.testing.allocator, "{", "https://api.authdog.com/mcp"));

    const stripped = try stripAuthdogServer(std.testing.allocator, merged.doc);
    defer std.testing.allocator.free(stripped.doc);
    defer freeNameList(std.testing.allocator, stripped.preserved);
    try std.testing.expect(stripped.removed);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "\"authdog\"") == null);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "\"other\"") != null);
}

test "invalid mcp json is not rewritten" {
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const path = try std.fmt.allocPrint(std.testing.allocator, ".zig-cache/tmp/{s}/mcp.json", .{&tmp.sub_path});
    defer std.testing.allocator.free(path);
    try writeConfigFile(io, path, "{");
    try std.testing.expectError(error.InvalidJson, upsertCursorMcpConfig(std.testing.allocator, io, path, "https://api.authdog.com/mcp"));
    const after = (try readOptionalFile(std.testing.allocator, io, path)).?;
    defer std.testing.allocator.free(after);
    try std.testing.expectEqualStrings("{", after);
}

test "init snippet skips an existing file and leaves layout bytes" {
    const io = std.testing.io;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const layout = try std.fmt.allocPrint(std.testing.allocator, ".zig-cache/tmp/{s}/app/layout.tsx", .{&tmp.sub_path});
    defer std.testing.allocator.free(layout);
    const snippet = try std.fmt.allocPrint(std.testing.allocator, ".zig-cache/tmp/{s}/app/authdog-provider.tsx", .{&tmp.sub_path});
    defer std.testing.allocator.free(snippet);
    try applyNewFile(io, layout, "layout-bytes");
    const created = if (fileExists(io, snippet)) "skip-exists" else blk: {
        try applyNewFile(io, snippet, "snippet-bytes");
        break :blk "create";
    };
    const skipped = if (fileExists(io, snippet)) "skip-exists" else "create";
    const layout_after = (try readOptionalFile(std.testing.allocator, io, layout)).?;
    defer std.testing.allocator.free(layout_after);
    const snippet_after = (try readOptionalFile(std.testing.allocator, io, snippet)).?;
    defer std.testing.allocator.free(snippet_after);
    try std.testing.expectEqualStrings("create", created);
    try std.testing.expectEqualStrings("skip-exists", skipped);
    try std.testing.expectEqualStrings("layout-bytes", layout_after);
    try std.testing.expectEqualStrings("snippet-bytes", snippet_after);
    try std.testing.expectEqualStrings("app/authdog-provider.tsx", snippetDest(.nextjs).?);
    try std.testing.expectEqualStrings("bun", try resolvePackageManager(io, "bun", .nextjs));
    try std.testing.expectError(error.UnknownPackageManager, resolvePackageManager(io, "cargo", .nextjs));
}
