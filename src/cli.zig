//! Conventional process-level CLI.

const std = @import("std");
const login = @import("login.zig");
const session = @import("session.zig");
const globals = @import("globals.zig");
const out = @import("out.zig");
const commands = @import("commands.zig");
const auth_session = @import("auth_session.zig");

pub const OutputFormat = globals.OutputFormat;

pub const Command = union(enum) {
    login,
    logout,
    status,
    whoami,
    link: commands.LinkOptions,
    unlink,
    init: commands.InitOptions,
    config_pull,
    config_diff,
    config_patch,
    webhooks_verify: commands.WebhooksVerifyOptions,
    webhooks_listen: commands.WebhooksListenOptions,
    impersonate: commands.ImpersonateOptions,
    deploy_status,
    doctor: commands.DoctorOptions,
    mcp: commands.McpSub,
};

pub const Invocation = union(enum) {
    help,
    version,
    missing,
    bad: []const u8,
    run: struct {
        globals: globals.GlobalOptions,
        command: Command,
    },
};

pub fn run(init: std.process.Init) !u8 {
    const allocator = init.arena.allocator();
    const io = init.io;

    var args_buf: [128][]const u8 = undefined;
    const args = try collectArgs(allocator, init.minimal.args, &args_buf);

    switch (try parse(allocator, init.environ_map, args)) {
        .help => {
            try out.writeAll(io, std.Io.File.stdout(), help_text);
            return 0;
        },
        .version => {
            try out.print(io, std.Io.File.stdout(), "authdog {s}\n", .{@import("build_options").version});
            return 0;
        },
        .missing => {
            try out.writeAll(io, std.Io.File.stdout(), help_text);
            return 0;
        },
        .bad => |message| {
            try out.print(io, std.Io.File.stderr(), "error: {s}\n", .{message});
            return 2;
        },
        .run => |parsed| {
            dispatch(allocator, io, init.environ_map, parsed.globals, parsed.command) catch |err| switch (err) {
                error.Reported => return 2,
                error.AuthRequired => {
                    try out.errPrint(io, allocator, "not logged in; run `authdog login`", .{});
                    return 3;
                },
                else => return err,
            };
            return 0;
        },
    }
}

fn dispatch(
    allocator: std.mem.Allocator,
    io: std.Io,
    environ: *std.process.Environ.Map,
    opts: globals.GlobalOptions,
    command: Command,
) !void {
    switch (command) {
        .login => {
            const cfg = try login.AuthConfig.fromEnv(allocator, environ);
            try login.runBrowserLogin(allocator, io, environ, cfg);
            try emitStatus(allocator, io, opts.format, "Logged in to Authdog.", true);
        },
        .logout => {
            try session.clearSession(allocator, io, environ);
            try emitStatus(allocator, io, opts.format, "Logged out. Local credentials removed.", false);
        },
        .status => {
            const loaded = try session.loadSession(allocator, io, environ);
            defer if (loaded) |value| {
                allocator.free(value.access_token);
                allocator.free(value.refresh_token);
                if (value.api_origin) |origin| allocator.free(origin);
            };
            const path = try session.credentialsPath(allocator, io, environ);
            const logged_in = loaded != null;
            const text = try std.fmt.allocPrint(allocator, "Logged in: {s}\nCredentials: {s}", .{
                if (logged_in) "yes" else "no",
                path,
            });
            defer allocator.free(text);
            try emitStatusEx(allocator, io, opts.format, text, logged_in, path);
        },
        .whoami => try commands.runWhoami(allocator, io, environ, opts),
        .link => |link| try commands.runLink(allocator, io, environ, opts, link),
        .unlink => try commands.runUnlink(allocator, io, opts),
        .init => |init_opts| try commands.runInit(allocator, io, environ, opts, init_opts),
        .config_pull => try commands.runConfigPull(allocator, io, environ, opts),
        .config_diff => try commands.runConfigDiff(allocator, io, opts),
        .config_patch => try out.fail(allocator, io, opts.format, "config patch is not implemented yet"),
        .webhooks_verify => |verify| try commands.runWebhooksVerify(allocator, io, opts, verify),
        .webhooks_listen => |listen| try commands.runWebhooksListen(allocator, io, environ, opts, listen),
        .impersonate => |imp| try commands.runImpersonate(allocator, io, environ, opts, imp),
        .deploy_status => try commands.runDeployStatus(allocator, io, environ, opts),
        .doctor => |doc| try commands.runDoctor(allocator, io, environ, opts, doc),
        .mcp => |sub| try commands.runMcp(allocator, io, environ, opts, sub),
    }
}

fn emitStatus(allocator: std.mem.Allocator, io: std.Io, format: OutputFormat, text: []const u8, logged_in: bool) !void {
    try out.emit(allocator, io, format, text, .{ .logged_in = logged_in });
}

fn emitStatusEx(allocator: std.mem.Allocator, io: std.Io, format: OutputFormat, text: []const u8, logged_in: bool, path: []const u8) !void {
    try out.emit(allocator, io, format, text, .{
        .logged_in = logged_in,
        .credentials_path = path,
    });
}

const help_text =
    \\Usage: authdog [OPTIONS] <COMMAND>
    \\
    \\Authdog CLI — authenticate and manage projects from the terminal.
    \\
    \\Commands:
    \\  login                 Sign in through the hosted browser flow
    \\  logout                Delete locally stored credentials
    \\  status                Show local login status
    \\  whoami                Show the authenticated user (API userinfo)
    \\  link                  Link this directory to a tenant/project/environment
    \\  unlink                Remove .authdog/project.json
    \\  init                  Detect framework, link, and write SDK env vars
    \\  config pull           Pull environment config to .authdog/config.json
    \\  config diff           Compare local config (stub)
    \\  webhooks listen       Fetch recent webhook deliveries
    \\  webhooks verify       Verify an X-Authdog-Signature offline
    \\  impersonate           Mint an impersonation token (requires grant)
    \\  deploy status         Security posture and vanity domain snapshot
    \\  doctor                Connectivity and auth diagnostics
    \\  mcp install           Register Authdog MCP in Cursor (~/.cursor/mcp.json)
    \\  mcp list                Show MCP install target path
    \\  mcp uninstall           Reserved (does not delete shared config)
    \\
    \\  auth login|logout|status   Aliases for login, logout, status
    \\
    \\Options:
    \\  -o, --output <OUTPUT>       Output format [text, json]
    \\      --json                    Shorthand for --output json
    \\      --api-origin <URL>        API base (default https://api.authdog.com)
    \\      --project-file <PATH>     Link manifest (default .authdog/project.json)
    \\      --non-interactive         Fail instead of prompting
    \\      --yes                     Accept defaults in wizards
    \\  -h, --help                    Print help
    \\  -V, --version                 Print version
    \\
;

pub fn parse(allocator: std.mem.Allocator, environ: *std.process.Environ.Map, args: []const []const u8) !Invocation {
    var opts = globals.GlobalOptions{
        .api_origin = globals.apiOriginFromEnv(environ),
        .project_file = globals.default_project_file,
    };

    var command_args: std.ArrayList([]const u8) = .empty;
    defer command_args.deinit(allocator);

    var index: usize = 0;
    while (index < args.len) {
        const arg = args[index];
        if (std.mem.eql(u8, arg, "--help") or std.mem.eql(u8, arg, "-h")) return .help;
        if (std.mem.eql(u8, arg, "--version") or std.mem.eql(u8, arg, "-V")) return .version;
        if (parseGlobalFlag(allocator, arg, args, &index, &opts)) |handled| {
            if (handled) {
                index += 1;
                continue;
            }
        } else |err| switch (err) {
            error.MissingFlagValue => return .{ .bad = "missing flag value" },
            error.InvalidOutputFormat => return .{ .bad = try std.fmt.allocPrint(allocator, "invalid output format in '{s}'", .{arg}) },
        }
        if (std.mem.startsWith(u8, arg, "-")) {
            return .{ .bad = try std.fmt.allocPrint(allocator, "unexpected argument '{s}'", .{arg}) };
        }
        try command_args.append(allocator, arg);
        index += 1;
    }

    if (command_args.items.len == 0) return .missing;

    var cmd_index: usize = 0;
    const command = parseCommand(allocator, command_args.items, &cmd_index) catch |err| switch (err) {
        error.UnrecognizedCommand => return .{ .bad = try std.fmt.allocPrint(allocator, "unrecognized command '{s}'", .{command_args.items[0]}) },
        error.MissingSubcommand => return .{ .bad = "missing subcommand" },
        error.MissingFlagValue => return .{ .bad = "missing required flag or value" },
    };
    return .{ .run = .{ .globals = opts, .command = command } };
}

const GlobalFlagError = error{
    MissingFlagValue,
    InvalidOutputFormat,
};

fn parseGlobalFlag(_: std.mem.Allocator, arg: []const u8, args: []const []const u8, index: *usize, opts: *globals.GlobalOptions) GlobalFlagError!bool {
    if (std.mem.eql(u8, arg, "--json")) {
        opts.format = .json;
        return true;
    }
    if (std.mem.eql(u8, arg, "--non-interactive")) {
        opts.non_interactive = true;
        return true;
    }
    if (std.mem.eql(u8, arg, "--yes")) {
        opts.yes = true;
        return true;
    }
    if (std.mem.eql(u8, arg, "--output") or std.mem.eql(u8, arg, "-o")) {
        index.* += 1;
        if (index.* >= args.len) return error.MissingFlagValue;
        opts.format = parseFormat(args[index.*]) orelse return error.InvalidOutputFormat;
        return true;
    }
    if (std.mem.startsWith(u8, arg, "--output=")) {
        const value = arg["--output=".len..];
        opts.format = parseFormat(value) orelse return error.InvalidOutputFormat;
        return true;
    }
    if (std.mem.eql(u8, arg, "--api-origin")) {
        index.* += 1;
        if (index.* >= args.len) return error.MissingFlagValue;
        opts.api_origin = login.trimTrailingSlashes(args[index.*]);
        return true;
    }
    if (std.mem.startsWith(u8, arg, "--api-origin=")) {
        opts.api_origin = login.trimTrailingSlashes(arg["--api-origin=".len..]);
        return true;
    }
    if (std.mem.eql(u8, arg, "--project-file")) {
        index.* += 1;
        if (index.* >= args.len) return error.MissingFlagValue;
        opts.project_file = args[index.*];
        return true;
    }
    if (std.mem.startsWith(u8, arg, "--project-file=")) {
        opts.project_file = arg["--project-file=".len..];
        return true;
    }
    return false;
}

const ParseCommandError = error{
    UnrecognizedCommand,
    MissingSubcommand,
    MissingFlagValue,
};

fn parseCommand(allocator: std.mem.Allocator, args: []const []const u8, index: *usize) ParseCommandError!Command {
    const head = args[index.*];
    if (std.mem.eql(u8, head, "login")) return .login;
    if (std.mem.eql(u8, head, "logout")) return .logout;
    if (std.mem.eql(u8, head, "status")) return .status;
    if (std.mem.eql(u8, head, "whoami")) return .whoami;
    if (std.mem.eql(u8, head, "unlink")) return .unlink;
    if (std.mem.eql(u8, head, "link")) return try parseLink(allocator, args, index);
    if (std.mem.eql(u8, head, "init")) return try parseInit(allocator, args, index);
    if (std.mem.eql(u8, head, "config")) return try parseConfig(args, index);
    if (std.mem.eql(u8, head, "webhooks")) return try parseWebhooks(allocator, args, index);
    if (std.mem.eql(u8, head, "impersonate") or std.mem.eql(u8, head, "imp")) return try parseImpersonate(allocator, args, index);
    if (std.mem.eql(u8, head, "deploy")) return try parseDeploy(args, index);
    if (std.mem.eql(u8, head, "doctor")) return try parseDoctor(args, index);
    if (std.mem.eql(u8, head, "mcp")) return try parseMcp(args, index);
    if (std.mem.eql(u8, head, "auth")) return try parseAuth(args, index);
    return error.UnrecognizedCommand;
}

fn parseAuth(args: []const []const u8, index: *usize) ParseCommandError!Command {
    index.* += 1;
    if (index.* >= args.len) return error.MissingSubcommand;
    const sub = args[index.*];
    if (std.mem.eql(u8, sub, "login")) return .login;
    if (std.mem.eql(u8, sub, "logout")) return .logout;
    if (std.mem.eql(u8, sub, "status")) return .status;
    return error.MissingSubcommand;
}

fn parseLink(allocator: std.mem.Allocator, args: []const []const u8, index: *usize) ParseCommandError!Command {
    var link: commands.LinkOptions = .{};
    index.* += 1;
    while (index.* + 1 < args.len) {
        const flag = args[index.*];
        if (std.mem.eql(u8, flag, "--tenant-id")) {
            index.* += 1;
            link.tenant_id = args[index.*];
            index.* += 1;
            continue;
        }
        if (std.mem.eql(u8, flag, "--application-id")) {
            index.* += 1;
            link.application_id = args[index.*];
            index.* += 1;
            continue;
        }
        if (std.mem.eql(u8, flag, "--environment-id")) {
            index.* += 1;
            link.environment_id = args[index.*];
            index.* += 1;
            continue;
        }
        break;
    }
    _ = allocator;
    return .{ .link = link };
}

fn parseInit(allocator: std.mem.Allocator, args: []const []const u8, index: *usize) ParseCommandError!Command {
    var init_opts: commands.InitOptions = .{};
    index.* += 1;
    while (index.* < args.len and std.mem.startsWith(u8, args[index.*], "--")) {
        const flag = args[index.*];
        if (std.mem.eql(u8, flag, "--dry-run")) {
            init_opts.dry_run = true;
            index.* += 1;
            continue;
        }
        if (std.mem.eql(u8, flag, "--skip-install")) {
            init_opts.skip_install = true;
            index.* += 1;
            continue;
        }
        if (std.mem.eql(u8, flag, "--framework")) {
            index.* += 1;
            if (index.* >= args.len) return error.MissingFlagValue;
            init_opts.framework = args[index.*];
            index.* += 1;
            continue;
        }
        if (std.mem.eql(u8, flag, "--env-file")) {
            index.* += 1;
            if (index.* >= args.len) return error.MissingFlagValue;
            init_opts.env_file = args[index.*];
            index.* += 1;
            continue;
        }
        break;
    }
    _ = allocator;
    return .{ .init = init_opts };
}

fn parseConfig(args: []const []const u8, index: *usize) ParseCommandError!Command {
    index.* += 1;
    if (index.* >= args.len) return error.MissingSubcommand;
    const sub = args[index.*];
    if (std.mem.eql(u8, sub, "pull")) return .config_pull;
    if (std.mem.eql(u8, sub, "diff")) return .config_diff;
    if (std.mem.eql(u8, sub, "patch")) return .config_patch;
    return error.MissingSubcommand;
}

fn parseWebhooks(allocator: std.mem.Allocator, args: []const []const u8, index: *usize) ParseCommandError!Command {
    index.* += 1;
    if (index.* >= args.len) return error.MissingSubcommand;
    const sub = args[index.*];
    if (std.mem.eql(u8, sub, "listen")) {
        var listen: commands.WebhooksListenOptions = .{};
        index.* += 1;
        while (index.* + 1 < args.len and std.mem.eql(u8, args[index.*], "--forward")) {
            index.* += 1;
            listen.forward = args[index.*];
            index.* += 1;
        }
        return .{ .webhooks_listen = listen };
    }
    if (std.mem.eql(u8, sub, "verify")) {
        var verify: commands.WebhooksVerifyOptions = .{ .secret = "", .signature = "", .body = "" };
        index.* += 1;
        while (index.* + 1 < args.len) {
            const flag = args[index.*];
            if (std.mem.eql(u8, flag, "--secret")) {
                index.* += 1;
                verify.secret = args[index.*];
                index.* += 1;
                continue;
            }
            if (std.mem.eql(u8, flag, "--signature")) {
                index.* += 1;
                verify.signature = args[index.*];
                index.* += 1;
                continue;
            }
            if (std.mem.eql(u8, flag, "--body")) {
                index.* += 1;
                verify.body = args[index.*];
                index.* += 1;
                continue;
            }
            break;
        }
        if (verify.secret.len == 0 or verify.signature.len == 0 or verify.body.len == 0) {
            _ = allocator;
            return error.MissingFlagValue;
        }
        return .{ .webhooks_verify = verify };
    }
    return error.MissingSubcommand;
}

fn parseImpersonate(allocator: std.mem.Allocator, args: []const []const u8, index: *usize) ParseCommandError!Command {
    var imp: commands.ImpersonateOptions = .{};
    index.* += 1;
    while (index.* + 1 < args.len) {
        const flag = args[index.*];
        if (std.mem.eql(u8, flag, "--user-id")) {
            index.* += 1;
            imp.user_id = args[index.*];
            index.* += 1;
            continue;
        }
        if (std.mem.eql(u8, flag, "--duration-minutes")) {
            index.* += 1;
            imp.duration_minutes = std.fmt.parseInt(u32, args[index.*], 10) catch return error.MissingFlagValue;
            index.* += 1;
            continue;
        }
        if (std.mem.eql(u8, flag, "--reason")) {
            index.* += 1;
            imp.reason = args[index.*];
            index.* += 1;
            continue;
        }
        if (std.mem.eql(u8, flag, "--no-create-grant")) {
            imp.create_grant = false;
            index.* += 1;
            continue;
        }
        break;
    }
    _ = allocator;
    return .{ .impersonate = imp };
}

fn parseDeploy(args: []const []const u8, index: *usize) ParseCommandError!Command {
    index.* += 1;
    if (index.* >= args.len) return error.MissingSubcommand;
    if (std.mem.eql(u8, args[index.*], "status")) return .deploy_status;
    return error.MissingSubcommand;
}

fn parseDoctor(args: []const []const u8, index: *usize) ParseCommandError!Command {
    var doc: commands.DoctorOptions = .{};
    index.* += 1;
    while (index.* < args.len and std.mem.eql(u8, args[index.*], "--mcp")) {
        doc.check_mcp = true;
        index.* += 1;
    }
    return .{ .doctor = doc };
}

fn parseMcp(args: []const []const u8, index: *usize) ParseCommandError!Command {
    index.* += 1;
    if (index.* >= args.len) return error.MissingSubcommand;
    const sub = args[index.*];
    if (std.mem.eql(u8, sub, "install")) return .{ .mcp = .install };
    if (std.mem.eql(u8, sub, "list")) return .{ .mcp = .list };
    if (std.mem.eql(u8, sub, "uninstall")) return .{ .mcp = .uninstall };
    return error.MissingSubcommand;
}

fn parseFormat(value: []const u8) ?OutputFormat {
    if (std.mem.eql(u8, value, "text")) return .text;
    if (std.mem.eql(u8, value, "json")) return .json;
    return null;
}

fn collectArgs(allocator: std.mem.Allocator, args: std.process.Args, buf: [][]const u8) ![]const []const u8 {
    var iterator = try std.process.Args.Iterator.initAllocator(args, allocator);
    defer iterator.deinit();
    _ = iterator.next();
    var len: usize = 0;
    while (iterator.next()) |arg| {
        if (len >= buf.len) return error.TooManyArgs;
        buf[len] = try allocator.dupe(u8, arg);
        len += 1;
    }
    return buf[0..len];
}

test "parses login command" {
    var env = std.process.Environ.Map.init(std.testing.allocator);
    defer env.deinit();
    const parsed = try parse(std.testing.allocator, &env, &.{"login"});
    try std.testing.expect(parsed == .run);
    try std.testing.expect(parsed.run.command == .login);
}

test "parses auth status" {
    var env = std.process.Environ.Map.init(std.testing.allocator);
    defer env.deinit();
    const parsed = try parse(std.testing.allocator, &env, &.{ "auth", "status" });
    try std.testing.expect(parsed.run.command == .status);
}

test "parses whoami" {
    var env = std.process.Environ.Map.init(std.testing.allocator);
    defer env.deinit();
    const parsed = try parse(std.testing.allocator, &env, &.{"whoami"});
    try std.testing.expect(parsed.run.command == .whoami);
}
