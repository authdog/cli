//! Management GraphQL (console parity); used when REST omits one-time secrets.

const std = @import("std");
const client = @import("client.zig");
const login = @import("../login.zig");

pub const default_endpoint = "https://mgt.authdog.com/graphql";

pub fn endpointFromEnv(environ: *const std.process.Environ.Map) []const u8 {
    if (environ.get("AUTHDOG_MANAGEMENT_GRAPHQL")) |v| {
        if (v.len != 0) return login.trimTrailingSlashes(v);
    }
    return default_endpoint;
}

pub fn postGraphql(
    allocator: std.mem.Allocator,
    io: std.Io,
    graphql_url: []const u8,
    bearer: []const u8,
    body: []const u8,
) !client.Response {
    return client.fetch(allocator, io, .POST, graphql_url, bearer, body);
}
