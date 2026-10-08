//! Shared CLI defaults and environment resolution.

const std = @import("std");
const login = @import("login.zig");

pub const default_api_origin = "https://api.authdog.com";
pub const default_project_file = ".authdog/project.json";
pub const default_config_file = ".authdog/config.json";

pub const GlobalOptions = struct {
    format: OutputFormat = .text,
    api_origin: []const u8 = default_api_origin,
    project_file: []const u8 = default_project_file,
    non_interactive: bool = false,
    yes: bool = false,
};

pub const OutputFormat = enum { text, json };

pub fn apiOriginFromEnv(environ: *const std.process.Environ.Map) []const u8 {
    if (environ.get("AUTHDOG_API_ORIGIN")) |v| {
        if (v.len != 0) return login.trimTrailingSlashes(v);
    }
    return default_api_origin;
}

pub fn identityOriginFromEnv(
    allocator: std.mem.Allocator,
    environ: *const std.process.Environ.Map,
) ![]const u8 {
    const cfg = try login.AuthConfig.fromEnv(allocator, environ);
    defer allocator.free(cfg.identity_origin);
    defer allocator.free(cfg.environment_id);
    return allocator.dupe(u8, cfg.identity_origin);
}

test {
    _ = login;
}
