//! User-global MCP client config paths and merge/strip edits.
//! Edits never store tokens. Unrelated servers and root keys stay.

const builtin = @import("builtin");
const std = @import("std");

pub const Client = enum { cursor, claude, vscode, codex };

pub const all_clients = [_]Client{ .cursor, .claude, .vscode, .codex };

pub const JsonShape = struct {
    servers_key: []const u8,
    typed_http: bool,
};

pub const Edit = struct {
    doc: []u8,
    preserved: [][]u8,
    removed: bool = false,
};

pub const OsPaths = struct {
    home: []const u8,
    /// Linux config root. Unused for Cursor, Claude, and Codex.
    config_home: []const u8,
    appdata: ?[]const u8 = null,
};

pub fn parseClient(name: []const u8) ?Client {
    if (std.mem.eql(u8, name, "cursor")) return .cursor;
    if (std.mem.eql(u8, name, "claude")) return .claude;
    if (std.mem.eql(u8, name, "vscode")) return .vscode;
    if (std.mem.eql(u8, name, "codex")) return .codex;
    return null;
}

pub fn clientName(client: Client) []const u8 {
    return switch (client) {
        .cursor => "cursor",
        .claude => "claude",
        .vscode => "vscode",
        .codex => "codex",
    };
}

pub fn jsonShape(client: Client) ?JsonShape {
    return switch (client) {
        .cursor => .{ .servers_key = "mcpServers", .typed_http = false },
        .claude => .{ .servers_key = "mcpServers", .typed_http = true },
        .vscode => .{ .servers_key = "servers", .typed_http = true },
        .codex => null,
    };
}

pub fn configPath(allocator: std.mem.Allocator, paths: OsPaths, client: Client) ![]u8 {
    return switch (client) {
        .cursor => std.fs.path.join(allocator, &.{ paths.home, ".cursor", "mcp.json" }),
        .claude => std.fs.path.join(allocator, &.{ paths.home, ".claude.json" }),
        .codex => std.fs.path.join(allocator, &.{ paths.home, ".codex", "config.toml" }),
        .vscode => switch (builtin.os.tag) {
            .windows => blk: {
                const appdata = paths.appdata orelse return error.NoHome;
                break :blk std.fs.path.join(allocator, &.{ appdata, "Code", "User", "mcp.json" });
            },
            .macos => std.fs.path.join(allocator, &.{ paths.home, "Library", "Application Support", "Code", "User", "mcp.json" }),
            else => std.fs.path.join(allocator, &.{ paths.config_home, "Code", "User", "mcp.json" }),
        },
    };
}

/// Directory that means the client is present. Claude also counts `~/.claude.json`.
pub fn installDir(allocator: std.mem.Allocator, paths: OsPaths, client: Client) ![]u8 {
    return switch (client) {
        .cursor => std.fs.path.join(allocator, &.{ paths.home, ".cursor" }),
        .claude => std.fs.path.join(allocator, &.{ paths.home, ".claude" }),
        .codex => std.fs.path.join(allocator, &.{ paths.home, ".codex" }),
        .vscode => switch (builtin.os.tag) {
            .windows => blk: {
                const appdata = paths.appdata orelse return error.NoHome;
                break :blk std.fs.path.join(allocator, &.{ appdata, "Code", "User" });
            },
            .macos => std.fs.path.join(allocator, &.{ paths.home, "Library", "Application Support", "Code", "User" }),
            else => std.fs.path.join(allocator, &.{ paths.config_home, "Code", "User" }),
        },
    };
}

pub fn dirExists(io: std.Io, path: []const u8) bool {
    const stat = std.Io.Dir.cwd().statFile(io, path, .{}) catch return false;
    return stat.kind == .directory;
}

pub fn fileExists(io: std.Io, path: []const u8) bool {
    const stat = std.Io.Dir.cwd().statFile(io, path, .{}) catch return false;
    return stat.kind == .file;
}

pub fn isInstalled(allocator: std.mem.Allocator, io: std.Io, paths: OsPaths, client: Client) !bool {
    if (client == .claude) {
        const file = try configPath(allocator, paths, .claude);
        defer allocator.free(file);
        if (fileExists(io, file)) return true;
    }
    const dir = try installDir(allocator, paths, client);
    defer allocator.free(dir);
    return dirExists(io, dir);
}

pub fn mergeJson(allocator: std.mem.Allocator, existing: []const u8, url: []const u8, shape: JsonShape) !Edit {
    const parsed = try parseJsonValue(allocator, existing);
    defer parsed.deinit();
    if (parsed.value != .object) return error.InvalidJson;
    var root = parsed.value;
    const gpa = parsed.arena.allocator();
    var servers = std.json.ObjectMap.empty;
    if (objectField(root, shape.servers_key)) |current| {
        if (current != .object) return error.InvalidJson;
        servers = current.object;
    }
    var authdog_server = std.json.ObjectMap.empty;
    if (servers.get("authdog")) |current| {
        if (current != .object) return error.InvalidJson;
        authdog_server = current.object;
    }
    const url_copy = try gpa.dupe(u8, url);
    try authdog_server.put(gpa, "url", .{ .string = url_copy });
    if (shape.typed_http) {
        const type_copy = try gpa.dupe(u8, "http");
        try authdog_server.put(gpa, "type", .{ .string = type_copy });
    }
    try servers.put(gpa, "authdog", .{ .object = authdog_server });
    try root.object.put(gpa, shape.servers_key, .{ .object = servers });

    const preserved = try preservedNames(allocator, servers);
    errdefer freeNames(allocator, preserved);
    var doc: std.Io.Writer.Allocating = .init(allocator);
    errdefer doc.deinit();
    try doc.writer.print("{f}\n", .{std.json.fmt(root, .{ .whitespace = .indent_2 })});
    return .{ .doc = try doc.toOwnedSlice(), .preserved = preserved };
}

pub fn stripJson(allocator: std.mem.Allocator, existing: []const u8, shape: JsonShape) !Edit {
    const parsed = try parseJsonValue(allocator, existing);
    defer parsed.deinit();
    if (parsed.value != .object) return error.InvalidJson;
    var root = parsed.value;
    const gpa = parsed.arena.allocator();
    var removed = false;
    var servers = std.json.ObjectMap.empty;
    if (objectField(root, shape.servers_key)) |current| {
        if (current != .object) return error.InvalidJson;
        servers = current.object;
        removed = servers.orderedRemove("authdog");
        try root.object.put(gpa, shape.servers_key, .{ .object = servers });
    }
    const preserved = try preservedNames(allocator, servers);
    errdefer freeNames(allocator, preserved);
    var doc: std.Io.Writer.Allocating = .init(allocator);
    errdefer doc.deinit();
    try doc.writer.print("{f}\n", .{std.json.fmt(root, .{ .whitespace = .indent_2 })});
    return .{ .doc = try doc.toOwnedSlice(), .preserved = preserved, .removed = removed };
}

pub fn mergeToml(allocator: std.mem.Allocator, existing: []const u8, url: []const u8) !Edit {
    if (std.mem.indexOfScalar(u8, existing, 0) != null) return error.InvalidConfig;
    const url_line = try tomlUrlLine(allocator, url);
    defer allocator.free(url_line);
    const lines = try splitLines(allocator, existing);
    defer allocator.free(lines);

    var out: std.ArrayList([]const u8) = .empty;
    defer out.deinit(allocator);
    var in_root = false;
    var wrote_url = false;
    var saw = false;
    for (lines) |line| {
        if (sectionName(line)) |name| {
            if (in_root and !wrote_url) try out.append(allocator, url_line);
            in_root = isAuthdogRoot(name);
            if (in_root) {
                saw = true;
                wrote_url = false;
            }
            try out.append(allocator, line);
            continue;
        }
        if (in_root and isUrlKey(line)) {
            try out.append(allocator, url_line);
            wrote_url = true;
            continue;
        }
        try out.append(allocator, line);
    }
    if (in_root and !wrote_url) try out.append(allocator, url_line);
    if (!saw) {
        if (out.items.len > 0) try out.append(allocator, "");
        try out.append(allocator, "[mcp_servers.authdog]");
        try out.append(allocator, url_line);
    }
    const preserved = try preservedToml(allocator, lines, false);
    errdefer freeNames(allocator, preserved);
    return .{ .doc = try joinLines(allocator, out.items), .preserved = preserved };
}

pub fn stripToml(allocator: std.mem.Allocator, existing: []const u8) !Edit {
    if (std.mem.indexOfScalar(u8, existing, 0) != null) return error.InvalidConfig;
    const lines = try splitLines(allocator, existing);
    defer allocator.free(lines);
    var out: std.ArrayList([]const u8) = .empty;
    defer out.deinit(allocator);
    var skipping = false;
    var removed = false;
    for (lines) |line| {
        if (sectionName(line)) |name| {
            skipping = isAuthdogRelated(name);
            if (skipping) {
                removed = true;
                continue;
            }
        }
        if (skipping) continue;
        try out.append(allocator, line);
    }
    const preserved = try preservedToml(allocator, lines, true);
    errdefer freeNames(allocator, preserved);
    return .{ .doc = try joinLines(allocator, out.items), .preserved = preserved, .removed = removed };
}

pub const Presence = struct {
    readable: bool,
    authdog: bool,
};

pub fn inspect(allocator: std.mem.Allocator, client: Client, bytes: []const u8) Presence {
    if (client == .codex) {
        if (std.mem.indexOfScalar(u8, bytes, 0) != null) return .{ .readable = false, .authdog = false };
        return .{ .readable = true, .authdog = authdogPresent(allocator, client, bytes) };
    }
    const parsed = parseJsonValue(allocator, bytes) catch return .{ .readable = false, .authdog = false };
    defer parsed.deinit();
    const shape = jsonShape(client) orelse return .{ .readable = false, .authdog = false };
    const servers = objectField(parsed.value, shape.servers_key) orelse return .{ .readable = true, .authdog = false };
    if (servers != .object) return .{ .readable = false, .authdog = false };
    return .{ .readable = true, .authdog = servers.object.get("authdog") != null };
}

pub fn authdogPresent(allocator: std.mem.Allocator, client: Client, bytes: []const u8) bool {
    if (client == .codex) {
        var lines = std.mem.splitScalar(u8, bytes, '\n');
        while (lines.next()) |line| {
            const name = sectionName(line) orelse continue;
            if (isAuthdogRoot(name)) return true;
        }
        return false;
    }
    const shape = jsonShape(client) orelse return false;
    const parsed = parseJsonValue(allocator, bytes) catch return false;
    defer parsed.deinit();
    const servers = objectField(parsed.value, shape.servers_key) orelse return false;
    if (servers != .object) return false;
    return servers.object.get("authdog") != null;
}

pub fn freeNames(allocator: std.mem.Allocator, names: [][]u8) void {
    for (names) |name| allocator.free(name);
    allocator.free(names);
}

fn parseJsonValue(allocator: std.mem.Allocator, bytes: []const u8) !std.json.Parsed(std.json.Value) {
    return std.json.parseFromSlice(std.json.Value, allocator, bytes, .{}) catch return error.InvalidJson;
}

fn objectField(value: std.json.Value, name: []const u8) ?std.json.Value {
    return switch (value) {
        .object => |map| map.get(name),
        else => null,
    };
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

fn tomlUrlLine(allocator: std.mem.Allocator, url: []const u8) ![]u8 {
    var escaped: std.Io.Writer.Allocating = .init(allocator);
    defer escaped.deinit();
    for (url) |byte| {
        if (byte == '\\' or byte == '"') try escaped.writer.writeByte('\\');
        try escaped.writer.writeByte(byte);
    }
    return std.fmt.allocPrint(allocator, "url = \"{s}\"", .{escaped.written()});
}

fn splitLines(allocator: std.mem.Allocator, existing: []const u8) ![][]const u8 {
    var list: std.ArrayList([]const u8) = .empty;
    errdefer list.deinit(allocator);
    if (existing.len == 0) return list.toOwnedSlice(allocator);
    var it = std.mem.splitScalar(u8, existing, '\n');
    while (it.next()) |line| {
        const trimmed_cr = if (line.len > 0 and line[line.len - 1] == '\r') line[0 .. line.len - 1] else line;
        try list.append(allocator, trimmed_cr);
    }
    if (list.items.len > 0 and list.items[list.items.len - 1].len == 0) _ = list.pop();
    return list.toOwnedSlice(allocator);
}

fn joinLines(allocator: std.mem.Allocator, lines: []const []const u8) ![]u8 {
    var doc: std.Io.Writer.Allocating = .init(allocator);
    errdefer doc.deinit();
    for (lines, 0..) |line, index| {
        if (index != 0) try doc.writer.writeByte('\n');
        try doc.writer.writeAll(line);
    }
    try doc.writer.writeByte('\n');
    return doc.toOwnedSlice();
}

fn sectionName(line: []const u8) ?[]const u8 {
    const trimmed = std.mem.trim(u8, line, " \t");
    if (trimmed.len < 3 or trimmed[0] != '[') return null;
    if (trimmed[1] == '[') return null;
    const end = std.mem.indexOfScalar(u8, trimmed, ']') orelse return null;
    if (end == 1) return null;
    return std.mem.trim(u8, trimmed[1..end], " \t");
}

fn isAuthdogRoot(name: []const u8) bool {
    return std.mem.eql(u8, name, "mcp_servers.authdog") or std.mem.eql(u8, name, "mcp_servers.\"authdog\"");
}

fn isAuthdogRelated(name: []const u8) bool {
    if (isAuthdogRoot(name)) return true;
    return std.mem.startsWith(u8, name, "mcp_servers.authdog.") or std.mem.startsWith(u8, name, "mcp_servers.\"authdog\".");
}

fn isUrlKey(line: []const u8) bool {
    const trimmed = std.mem.trim(u8, line, " \t");
    if (!std.mem.startsWith(u8, trimmed, "url")) return false;
    var index: usize = 3;
    while (index < trimmed.len and (trimmed[index] == ' ' or trimmed[index] == '\t')) : (index += 1) {}
    return index < trimmed.len and trimmed[index] == '=';
}

fn serverToken(section: []const u8) ?[]const u8 {
    const prefix = "mcp_servers.";
    if (!std.mem.startsWith(u8, section, prefix)) return null;
    const rest = section[prefix.len..];
    if (rest.len == 0) return null;
    if (rest[0] == '"') {
        const end = std.mem.indexOfScalar(u8, rest[1..], '"') orelse return null;
        if (end == 0) return null;
        return rest[1 .. 1 + end];
    }
    const dot = std.mem.indexOfScalar(u8, rest, '.') orelse rest.len;
    if (dot == 0) return null;
    return rest[0..dot];
}

fn preservedToml(allocator: std.mem.Allocator, lines: []const []const u8, after_strip: bool) ![][]u8 {
    var list: std.ArrayList([]u8) = .empty;
    errdefer {
        for (list.items) |item| allocator.free(item);
        list.deinit(allocator);
    }
    for (lines) |line| {
        const section = sectionName(line) orelse continue;
        const token = serverToken(section) orelse continue;
        if (std.mem.eql(u8, token, "authdog")) continue;
        var seen = false;
        for (list.items) |existing| {
            if (std.mem.eql(u8, existing, token)) seen = true;
        }
        if (seen) continue;
        if (after_strip and isAuthdogRelated(section)) continue;
        try list.append(allocator, try allocator.dupe(u8, token));
    }
    return list.toOwnedSlice(allocator);
}

test "json merge keeps other servers and root keys" {
    const cursor = try mergeJson(
        std.testing.allocator,
        "{\"mcpServers\":{\"other\":{\"url\":\"http://example\"}},\"numStartups\":3}",
        "https://api.authdog.com/mcp",
        jsonShape(.cursor).?,
    );
    defer std.testing.allocator.free(cursor.doc);
    defer freeNames(std.testing.allocator, cursor.preserved);
    try std.testing.expect(std.mem.indexOf(u8, cursor.doc, "\"other\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, cursor.doc, "\"authdog\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, cursor.doc, "numStartups") != null);
    try std.testing.expect(std.mem.indexOf(u8, cursor.doc, "\"type\"") == null);
    try std.testing.expectEqual(@as(usize, 1), cursor.preserved.len);
    try std.testing.expectEqualStrings("other", cursor.preserved[0]);
    try std.testing.expectError(error.InvalidJson, mergeJson(std.testing.allocator, "{", "https://api.authdog.com/mcp", jsonShape(.cursor).?));

    const vscode = try mergeJson(
        std.testing.allocator,
        "{\"inputs\":[],\"servers\":{\"other\":{\"type\":\"stdio\",\"command\":\"echo\"}}}",
        "https://api.authdog.com/mcp",
        jsonShape(.vscode).?,
    );
    defer std.testing.allocator.free(vscode.doc);
    defer freeNames(std.testing.allocator, vscode.preserved);
    try std.testing.expect(std.mem.indexOf(u8, vscode.doc, "\"inputs\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, vscode.doc, "\"command\": \"echo\"") != null or std.mem.indexOf(u8, vscode.doc, "\"command\":\"echo\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, vscode.doc, "\"type\": \"http\"") != null or std.mem.indexOf(u8, vscode.doc, "\"type\":\"http\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, vscode.doc, "Authorization") == null);

    const stripped = try stripJson(std.testing.allocator, cursor.doc, jsonShape(.cursor).?);
    defer std.testing.allocator.free(stripped.doc);
    defer freeNames(std.testing.allocator, stripped.preserved);
    try std.testing.expect(stripped.removed);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "\"authdog\"") == null);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "\"other\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "numStartups") != null);
}

test "toml merge updates authdog and leaves other servers" {
    const existing =
        \\model = "gpt"
        \\
        \\[mcp_servers.other]
        \\command = "echo"
        \\
        \\[mcp_servers.authdog]
        \\url = "http://old"
        \\enabled = true
        \\
        \\[mcp_servers.other.http_headers]
        \\X = "1"
        \\
    ;
    const merged = try mergeToml(std.testing.allocator, existing, "https://api.authdog.com/mcp");
    defer std.testing.allocator.free(merged.doc);
    defer freeNames(std.testing.allocator, merged.preserved);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "model = \"gpt\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "[mcp_servers.other]") != null);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "command = \"echo\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "url = \"https://api.authdog.com/mcp\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "http://old") == null);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "enabled = true") != null);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "[mcp_servers.other.http_headers]") != null);
    try std.testing.expectEqual(@as(usize, 1), merged.preserved.len);
    try std.testing.expectEqualStrings("other", merged.preserved[0]);
    try std.testing.expect(std.mem.indexOf(u8, merged.doc, "token") == null);

    const stripped = try stripToml(std.testing.allocator, merged.doc);
    defer std.testing.allocator.free(stripped.doc);
    defer freeNames(std.testing.allocator, stripped.preserved);
    try std.testing.expect(stripped.removed);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "[mcp_servers.authdog]") == null);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "enabled = true") == null);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "[mcp_servers.other]") != null);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "[mcp_servers.other.http_headers]") != null);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "model = \"gpt\"") != null);
}

test "toml uninstall removes a nested authdog table only" {
    const existing =
        \\[mcp_servers.authdog]
        \\url = "https://api.authdog.com/mcp"
        \\
        \\[mcp_servers.authdog.http_headers]
        \\X = "keep-out"
        \\
        \\[mcp_servers.other]
        \\command = "echo"
        \\
    ;
    const stripped = try stripToml(std.testing.allocator, existing);
    defer std.testing.allocator.free(stripped.doc);
    defer freeNames(std.testing.allocator, stripped.preserved);
    try std.testing.expect(stripped.removed);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "authdog") == null);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "keep-out") == null);
    try std.testing.expect(std.mem.indexOf(u8, stripped.doc, "[mcp_servers.other]") != null);
}

test "invalid json is unreadable" {
    const presence = inspect(std.testing.allocator, .cursor, "{");
    try std.testing.expect(!presence.readable);
    try std.testing.expect(!presence.authdog);
}

test "linux vscode path uses the config home" {
    const path = try configPath(std.testing.allocator, .{
        .home = "/home/me",
        .config_home = "/home/me/.config",
    }, .vscode);
    defer std.testing.allocator.free(path);
    if (builtin.os.tag != .windows and builtin.os.tag != .macos) {
        try std.testing.expectEqualStrings("/home/me/.config/Code/User/mcp.json", path);
    }
    const claude = try configPath(std.testing.allocator, .{
        .home = "/home/me",
        .config_home = "/home/me/.config",
    }, .claude);
    defer std.testing.allocator.free(claude);
    try std.testing.expectEqualStrings("/home/me/.claude.json", claude);
}
