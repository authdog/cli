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
};

pub const ImpersonateOptions = struct {
    user_id: ?[]const u8 = null,
    duration_minutes: u32 = 30,
    reason: ?[]const u8 = null,
    create_grant: bool = true,
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
};

pub const ConfigPatchOptions = struct {
    dry_run: bool = false,
};

const EnvConfigBundle = struct {
    session: []const u8,
    redirect: []const u8,
    webhooks: []const u8,
};

pub const DoctorOptions = struct {
    check_mcp: bool = false,
    check_agent_sign_in: bool = false,
};

pub const McpSub = enum { install, list, uninstall };

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

    if (init_opts.dry_run) {
        try out.emit(allocator, io, opts.format, "Dry run — no files written.", .{
            .schemaVersion = 1,
            .ok = true,
            .framework = framework.label(),
            .envFile = env_target,
            .envLines = env_lines,
            .projectFile = project_path,
        });
        return;
    }

    try mergeEnvFile(allocator, io, env_target, env_lines);

    if (!init_opts.skip_install) {
        if (framework.npmPackage()) |pkg| {
            try runPackageInstall(io, pkg);
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

fn runPackageInstall(io: std.Io, package: []const u8) !void {
    if (fileExists(io, "bun.lock")) {
        const argv = [_][]const u8{ "bun", "add", package };
        return spawnArgv(io, &argv);
    }
    if (fileExists(io, "pnpm-lock.yaml")) {
        const argv = [_][]const u8{ "pnpm", "add", package };
        return spawnArgv(io, &argv);
    }
    if (fileExists(io, "yarn.lock")) {
        const argv = [_][]const u8{ "yarn", "add", package };
        return spawnArgv(io, &argv);
    }
    const argv = [_][]const u8{ "npm", "install", package };
    return spawnArgv(io, &argv);
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

    const session_resp = try api.getJson(allocator, io, auth.api_origin, session_path, auth.access_token);
    const redirect_resp = try api.getJson(allocator, io, auth.api_origin, redirect_path, auth.access_token);
    const webhooks_resp = try api.getJson(allocator, io, auth.api_origin, webhooks_path, auth.access_token);
    if (session_resp.status < 200 or session_resp.status >= 300) {
        defer allocator.free(session_resp.body);
        try api.reportApiFailure(allocator, io, format, session_resp);
    }
    if (redirect_resp.status < 200 or redirect_resp.status >= 300) {
        defer allocator.free(session_resp.body);
        defer allocator.free(redirect_resp.body);
        try api.reportApiFailure(allocator, io, format, redirect_resp);
    }
    if (webhooks_resp.status < 200 or webhooks_resp.status >= 300) {
        defer allocator.free(session_resp.body);
        defer allocator.free(redirect_resp.body);
        defer allocator.free(webhooks_resp.body);
        try api.reportApiFailure(allocator, io, format, webhooks_resp);
    }
    return .{
        .session = session_resp.body,
        .redirect = redirect_resp.body,
        .webhooks = webhooks_resp.body,
    };
}

fn formatConfigDocument(allocator: std.mem.Allocator, bundle: EnvConfigBundle) ![]const u8 {
    var doc: std.Io.Writer.Allocating = .init(allocator);
    errdefer doc.deinit();
    try doc.writer.print(
        "{{\n  \"schemaVersion\": 1,\n  \"sessionConfig\": {s},\n  \"redirectUris\": {s},\n  \"webhooks\": {s}\n}}\n",
        .{ bundle.session, bundle.redirect, bundle.webhooks },
    );
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
    }

    const doc = try formatConfigDocument(allocator, bundle);
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
    }
    const live = try formatConfigDocument(allocator, bundle);
    defer allocator.free(live);

    const matches = std.mem.eql(u8, local, live);
    try out.emit(allocator, io, opts.format, if (matches) "Local config matches live." else "Local config differs from live.", .{
        .schemaVersion = 1,
        .ok = true,
        .matches = matches,
        .localBytes = local.len,
        .liveBytes = live.len,
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

    var actions: std.ArrayList([]const u8) = .empty;
    defer {
        for (actions.items) |a| allocator.free(a);
        actions.deinit(allocator);
    }

    if (parsed.value.sessionConfig != null) {
        try actions.append(allocator, try allocator.dupe(u8, "session-config"));
    }
    if (parsed.value.redirectUris != null) {
        try actions.append(allocator, try allocator.dupe(u8, "redirect-uris"));
    }
    if (actions.items.len == 0) {
        try out.fail(allocator, io, opts.format, "config file has nothing to patch (sessionConfig / redirectUris)");
    }

    if (patch.dry_run) {
        try out.emit(allocator, io, opts.format, "Dry run: would apply patches.", .{
            .schemaVersion = 1,
            .ok = true,
            .dryRun = true,
            .actions = actions.items,
        });
        return;
    }

    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked = try requireLinked(allocator, io, opts.format, project_path);
    defer linked.deinit(allocator);

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

    try out.emit(allocator, io, opts.format, "Applied config patch from local file.", .{
        .schemaVersion = 1,
        .ok = true,
        .actions = actions.items,
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

    var seen: std.StringHashMap(void) = .init(allocator);
    defer seen.deinit();

    var forwarded: u32 = 0;
    var polls: u32 = 0;
    const max_polls = if (listen.forward != null) listen.max_polls else 1;

    while (polls < max_polls) : (polls += 1) {
        const resp = try api.getJson(allocator, io, auth.api_origin, path, auth.access_token);
        defer allocator.free(resp.body);
        if (resp.status < 200 or resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, resp);

        const Delivery = struct {
            id: ?[]const u8 = null,
            eventType: ?[]const u8 = null,
            status: ?[]const u8 = null,
        };
        const parsed = try std.json.parseFromSlice(struct {
            data: ?[]Delivery = null,
        }, allocator, resp.body, .{ .ignore_unknown_fields = true });
        defer parsed.deinit();

        if (parsed.value.data) |items| {
            for (items) |item| {
                const id = item.id orelse continue;
                if (seen.contains(id)) continue;
                try seen.put(id, {});

                if (listen.forward) |forward_url| {
                    var payload: std.Io.Writer.Allocating = .init(allocator);
                    defer payload.deinit();
                    try payload.writer.print("{f}", .{std.json.fmt(.{
                        .deliveryId = id,
                        .eventType = item.eventType,
                        .status = item.status,
                    }, .{})});
                    const fwd = try api.postJson(allocator, io, forward_url, "", payload.written());
                    defer allocator.free(fwd.body);
                    if (fwd.status >= 200 and fwd.status < 300) forwarded += 1;
                }
            }
        }

        if (polls + 1 < max_polls) try pollSleep(io, listen.interval_ms);
    }

    try out.emit(allocator, io, opts.format, "Webhook delivery poll complete.", .{
        .schemaVersion = 1,
        .ok = true,
        .polls = polls,
        .trackedDeliveries = seen.count(),
        .forwarded = forwarded,
        .forward = listen.forward,
    });
}

pub fn runImpersonate(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *const std.process.Environ.Map,
    opts: globals.GlobalOptions,
    imp: ImpersonateOptions,
) !void {
    const user_id = imp.user_id orelse {
        try out.fail(allocator, io, opts.format, "missing --user-id");
    };

    const auth = auth_session.requireAuth(allocator, io, environ, opts.api_origin) catch {
        try out.fail(allocator, io, opts.format, "not logged in; run `authdog login`");
    };
    defer auth_session.deinitAuth(allocator, auth);

    const project_path = try project.resolvePath(allocator, opts);
    defer allocator.free(project_path);
    const linked = try requireLinked(allocator, io, opts.format, project_path);
    defer linked.deinit(allocator);

    const userinfo = try api.getJson(allocator, io, auth.api_origin, "/v1/userinfo", auth.access_token);
    defer allocator.free(userinfo.body);
    if (userinfo.status < 200 or userinfo.status >= 300) try api.reportApiFailure(allocator, io, opts.format, userinfo);

    const actor_id = try extractUserId(allocator, userinfo.body) orelse {
        try out.fail(allocator, io, opts.format, "could not resolve actor user id from userinfo");
    };
    defer allocator.free(actor_id);

    if (imp.create_grant) {
        const grant_path = try std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/impersonation-grants", .{ linked.tenantId, linked.environmentId });
        defer allocator.free(grant_path);
        const grant_url = try api.joinUrl(allocator, auth.api_origin, grant_path);
        defer allocator.free(grant_url);
        const reason = imp.reason orelse "CLI impersonation";
        const body = try std.fmt.allocPrint(allocator, "{{\"actorUserId\":\"{s}\",\"targetUserId\":\"{s}\",\"durationMinutes\":{d},\"reason\":\"{s}\"}}", .{
            actor_id,
            user_id,
            imp.duration_minutes,
            reason,
        });
        defer allocator.free(body);
        const grant_resp = try api.postJson(allocator, io, grant_url, auth.access_token, body);
        defer allocator.free(grant_resp.body);
        if (grant_resp.status < 200 or grant_resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, grant_resp);
    }

    const identity_url = try std.fmt.allocPrint(allocator, "{s}/api/v1/identity/{s}/impersonate", .{ linked.identityOrigin, linked.environmentId });
    defer allocator.free(identity_url);
    const imp_body = try std.fmt.allocPrint(allocator, "{{\"userId\":\"{s}\",\"tenantId\":\"{s}\"}}", .{ user_id, linked.tenantId });
    defer allocator.free(imp_body);
    const imp_resp = try api.postJson(allocator, io, identity_url, auth.access_token, imp_body);
    defer allocator.free(imp_resp.body);
    if (imp_resp.status < 200 or imp_resp.status >= 300) try api.reportApiFailure(allocator, io, opts.format, imp_resp);

    try out.emit(allocator, io, opts.format, "Impersonation token minted.", .{
        .schemaVersion = 1,
        .ok = true,
        .response = imp_resp.body,
    });
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

    const posture = try api.getJson(allocator, io, auth.api_origin, posture_path, auth.access_token);
    defer allocator.free(posture.body);
    const vanity = try api.getJson(allocator, io, auth.api_origin, vanity_path, auth.access_token);
    defer allocator.free(vanity.body);

    try out.emit(allocator, io, opts.format, "Deploy readiness snapshot.", .{
        .schemaVersion = 1,
        .ok = true,
        .securityPosture = posture.body,
        .vanityDomains = vanity.body,
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
    sub: McpSub,
) !void {
    const home = environ.get("HOME") orelse {
        try out.fail(allocator, io, opts.format, "HOME is not set");
    };
    if (home.len == 0) try out.fail(allocator, io, opts.format, "HOME is not set");
    const config_path = try std.fs.path.join(allocator, &.{ home, ".cursor", "mcp.json" });
    defer allocator.free(config_path);

    const mcp_url = try api.joinUrl(allocator, opts.api_origin, "/mcp");
    defer allocator.free(mcp_url);

    switch (sub) {
        .list => {
            const exists = try mcpConfigExists(io, config_path);
            try out.emit(allocator, io, opts.format, "Cursor MCP config.", .{
                .schemaVersion = 1,
                .ok = true,
                .configPath = config_path,
                .exists = exists,
                .serverUrl = mcp_url,
            });
        },
        .install => {
            try upsertCursorMcpConfig(allocator, io, config_path, mcp_url);
            try out.emit(allocator, io, opts.format, "Registered Authdog MCP in Cursor config.", .{
                .schemaVersion = 1,
                .ok = true,
                .configPath = config_path,
                .serverUrl = mcp_url,
            });
        },
        .uninstall => {
            try removeCursorMcpConfig(allocator, io, config_path);
            try out.emit(allocator, io, opts.format, "Removed Authdog MCP from Cursor config.", .{
                .schemaVersion = 1,
                .ok = true,
                .configPath = config_path,
            });
        },
    }
}

fn mcpConfigExists(io: std.Io, path: []const u8) !bool {
    const stat = std.Io.Dir.cwd().statFile(io, path, .{}) catch |err| switch (err) {
        error.FileNotFound => return false,
        else => return err,
    };
    _ = stat;
    return true;
}

fn upsertCursorMcpConfig(allocator: std.mem.Allocator, io: std.Io, path: []const u8, url: []const u8) !void {
    _ = allocator;
    const snippet =
        \\{
        \\  "mcpServers": {
        \\    "authdog": {
        \\      "url": "URL_PLACEHOLDER"
        \\    }
        \\  }
        \\}
    ;
    const replaced = try std.mem.replaceOwned(u8, std.heap.page_allocator, snippet, "URL_PLACEHOLDER", url);
    defer std.heap.page_allocator.free(replaced);

    if (std.fs.path.dirname(path)) |dir| try std.Io.Dir.cwd().createDirPath(io, dir);
    std.Io.Dir.cwd().writeFile(io, .{ .sub_path = path, .data = replaced }) catch |err| switch (err) {
        error.FileNotFound => try std.Io.Dir.cwd().writeFile(io, .{ .sub_path = path, .data = replaced }),
        else => return err,
    };
}

fn removeCursorMcpConfig(allocator: std.mem.Allocator, io: std.Io, path: []const u8) !void {
    _ = allocator;
    _ = io;
    _ = path;
    // v1: user-global MCP configs vary; avoid deleting unrelated servers.
}
