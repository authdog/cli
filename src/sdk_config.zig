//! SDK wiring helpers (publishable key + OIDC client selection).

const std = @import("std");
const api = @import("api/client.zig");

pub const SdkWiring = struct {
    public_key: []const u8,
    client_id: ?[]const u8,
};

pub fn buildPublicKey(
    allocator: std.mem.Allocator,
    tenant_id: []const u8,
    environment_id: []const u8,
    identity_host: []const u8,
) ![]u8 {
    const json = try std.fmt.allocPrint(
        allocator,
        "{{\"environmentId\":\"{s}\",\"identityHost\":\"{s}\",\"version\":\"0.0.1\",\"region\":\"global\",\"tenantId\":\"{s}\"}}",
        .{ environment_id, identity_host, tenant_id },
    );
    defer allocator.free(json);

    const enc_len = std.base64.standard.Encoder.calcSize(json.len);
    const encoded = try allocator.alloc(u8, enc_len);
    defer allocator.free(encoded);
    _ = std.base64.standard.Encoder.encode(encoded, json);

    return std.fmt.allocPrint(allocator, "pk_{s}", .{encoded});
}

const OidcClientRow = struct {
    clientId: ?[]const u8 = null,
    tokenEndpointAuthMethod: ?[]const u8 = null,
    pkceRequired: ?bool = null,
    active: ?bool = null,
};

const OidcClientsResp = struct {
    clients: ?[]OidcClientRow = null,
};

pub fn pickRecommendedClientId(allocator: std.mem.Allocator, body: []const u8) !?[]const u8 {
    const parsed = try std.json.parseFromSlice(OidcClientsResp, allocator, body, .{
        .ignore_unknown_fields = true,
    });
    defer parsed.deinit();
    const clients = parsed.value.clients orelse return null;
    if (clients.len == 0) return null;

    var pool: []const OidcClientRow = clients;
    var active_only: std.ArrayList(OidcClientRow) = .empty;
    defer active_only.deinit(allocator);
    for (clients) |c| {
        if (c.active != false) try active_only.append(allocator, c);
    }
    if (active_only.items.len > 0) pool = active_only.items;

    for (pool) |client| {
        if (std.mem.eql(u8, client.tokenEndpointAuthMethod orelse "", "none")) {
            if (client.clientId) |id| {
                if (id.len != 0) return try allocator.dupe(u8, id);
            }
        }
    }
    for (pool) |client| {
        if (client.pkceRequired == true) {
            if (client.clientId) |id| {
                if (id.len != 0) return try allocator.dupe(u8, id);
            }
        }
    }
    for (pool) |client| {
        if (client.clientId) |id| {
            if (id.len != 0) return try allocator.dupe(u8, id);
        }
    }
    return null;
}

pub fn fetchOidcClientsPath(
    allocator: std.mem.Allocator,
    tenant_id: []const u8,
    application_id: []const u8,
    environment_id: []const u8,
) ![]u8 {
    return std.fmt.allocPrint(
        allocator,
        "/v1/tenants/{s}/applications/{s}/environments/{s}/oidc-clients",
        .{ tenant_id, application_id, environment_id },
    );
}

test "buildPublicKey prefix" {
    const pk = try buildPublicKey(
        std.testing.allocator,
        "tenant-1",
        "env-1",
        "https://identity.authdog.com",
    );
    defer std.testing.allocator.free(pk);
    try std.testing.expect(std.mem.startsWith(u8, pk, "pk_"));
}

test {
    _ = api;
}
