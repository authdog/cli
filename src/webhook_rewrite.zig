//! Build a webhook endpoint update that changes only the destination URL.
//! The list payload's signing secret is never copied into the update body.

const std = @import("std");

pub fn isHttpUrl(url: []const u8) bool {
    const rest = if (std.mem.startsWith(u8, url, "https://"))
        url["https://".len..]
    else if (std.mem.startsWith(u8, url, "http://"))
        url["http://".len..]
    else
        return false;
    if (rest.len == 0) return false;
    if (std.mem.indexOfAny(u8, url, " \t\r\n") != null) return false;
    return true;
}

pub fn isChannelId(id: []const u8) bool {
    if (id.len == 0 or id.len > 128) return false;
    for (id) |byte| {
        const ok = std.ascii.isAlphanumeric(byte) or byte == '-' or byte == '_';
        if (!ok) return false;
    }
    return true;
}

pub fn updateBody(allocator: std.mem.Allocator, list_json: []const u8, channel_id: []const u8, url: []const u8) ![]u8 {
    const parsed = std.json.parseFromSlice(std.json.Value, allocator, list_json, .{}) catch return error.InvalidJson;
    defer parsed.deinit();
    const data = switch (parsed.value) {
        .object => |map| map.get("data") orelse return error.InvalidJson,
        else => return error.InvalidJson,
    };
    if (data != .array) return error.InvalidJson;
    for (data.array.items) |item| {
        if (item != .object) continue;
        const id = jsonString(item, "id") orelse continue;
        if (!std.mem.eql(u8, id, channel_id)) continue;
        const name = jsonString(item, "name") orelse return error.MissingName;
        if (name.len == 0) return error.MissingName;
        const enabled_value = switch (item) {
            .object => |map| map.get("enabled") orelse return error.MissingEnabled,
            else => return error.MissingEnabled,
        };
        if (enabled_value != .bool) return error.MissingEnabled;
        var event_types: std.ArrayList([]const u8) = .empty;
        defer event_types.deinit(allocator);
        if (switch (item) {
            .object => |map| map.get("eventTypes"),
            else => null,
        }) |events| {
            if (events != .array) return error.InvalidJson;
            for (events.array.items) |event| {
                if (event != .string) return error.InvalidJson;
                try event_types.append(allocator, event.string);
            }
        }
        var doc: std.Io.Writer.Allocating = .init(allocator);
        errdefer doc.deinit();
        try doc.writer.print("{f}", .{std.json.fmt(.{
            .name = name,
            .enabled = enabled_value.bool,
            .webhookUrl = url,
            .eventTypes = event_types.items,
        }, .{})});
        if (std.mem.indexOf(u8, doc.written(), "signingSecret") != null) return error.InvalidJson;
        return doc.toOwnedSlice();
    }
    return error.ChannelNotFound;
}

fn jsonString(value: std.json.Value, name: []const u8) ?[]const u8 {
    const field = switch (value) {
        .object => |map| map.get(name) orelse return null,
        else => return null,
    };
    return switch (field) {
        .string => |text| text,
        else => null,
    };
}

test "update body keeps name, enabled, and event types" {
    const list =
        \\{"data":[{"id":"chan_1","name":"Primary","enabled":false,"eventTypes":["USER_SIGNED_IN"],"signingSecret":"whsec_secret","webhookUrl":"***REDACTED***"}]}
    ;
    const body = try updateBody(std.testing.allocator, list, "chan_1", "https://tunnel.example/hook");
    defer std.testing.allocator.free(body);
    try std.testing.expect(std.mem.indexOf(u8, body, "\"name\":\"Primary\"") != null or std.mem.indexOf(u8, body, "\"name\": \"Primary\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, body, "USER_SIGNED_IN") != null);
    try std.testing.expect(std.mem.indexOf(u8, body, "https://tunnel.example/hook") != null);
    try std.testing.expect(std.mem.indexOf(u8, body, "false") != null);
    try std.testing.expect(std.mem.indexOf(u8, body, "whsec_secret") == null);
    try std.testing.expect(std.mem.indexOf(u8, body, "signingSecret") == null);
    try std.testing.expect(std.mem.indexOf(u8, body, "REDACTED") == null);
    try std.testing.expectError(error.ChannelNotFound, updateBody(std.testing.allocator, list, "missing", "https://tunnel.example/hook"));
}

test "http url and channel id checks" {
    try std.testing.expect(isHttpUrl("https://tunnel.example/hook"));
    try std.testing.expect(isHttpUrl("http://127.0.0.1:3000/webhooks/authdog"));
    try std.testing.expect(!isHttpUrl("***REDACTED***"));
    try std.testing.expect(!isHttpUrl("https://has space"));
    try std.testing.expect(isChannelId("chan_1"));
    try std.testing.expect(!isChannelId("chan/1"));
    try std.testing.expect(!isChannelId(""));
}
