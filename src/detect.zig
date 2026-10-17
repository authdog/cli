//! Framework and package-manager detection for `init`.

const std = @import("std");

pub const Framework = enum {
    nextjs,
    express,
    javascript,
    react,
    fastapi,
    unknown,

    pub fn parse(name: []const u8) ?Framework {
        if (std.mem.eql(u8, name, "nextjs") or std.mem.eql(u8, name, "next")) return .nextjs;
        if (std.mem.eql(u8, name, "express")) return .express;
        if (std.mem.eql(u8, name, "javascript")) return .javascript;
        if (std.mem.eql(u8, name, "react")) return .react;
        if (std.mem.eql(u8, name, "fastapi") or std.mem.eql(u8, name, "python")) return .fastapi;
        return null;
    }

    pub fn npmPackage(self: Framework) ?[]const u8 {
        return switch (self) {
            .nextjs => "@authdog/nextjs",
            .express => "@authdog/express",
            .javascript => "@authdog/javascript",
            .react => "@authdog/react",
            .fastapi => null,
            .unknown => null,
        };
    }

    pub fn pythonPackage(self: Framework) ?[]const u8 {
        return switch (self) {
            .fastapi => "authdog",
            else => null,
        };
    }

    pub fn label(self: Framework) []const u8 {
        return @tagName(self);
    }
};

pub fn frameworkFromManifests(package_json: ?[]const u8, pyproject: ?[]const u8) Framework {
    if (package_json) |raw| {
        if (std.mem.indexOf(u8, raw, "\"next\"") != null) return .nextjs;
        if (std.mem.indexOf(u8, raw, "@authdog/nextjs") != null) return .nextjs;
        if (std.mem.indexOf(u8, raw, "@authdog/express") != null) return .express;
        if (std.mem.indexOf(u8, raw, "@authdog/javascript") != null) return .javascript;
        if (std.mem.indexOf(u8, raw, "@authdog/react") != null or std.mem.indexOf(u8, raw, "\"react\"") != null) return .react;
        if (std.mem.indexOf(u8, raw, "\"express\"") != null) return .express;
    }
    if (pyproject) |raw| {
        if (std.mem.indexOf(u8, raw, "fastapi") != null or std.mem.indexOf(u8, raw, "authdog") != null) return .fastapi;
    }
    return .unknown;
}

pub fn detectFramework(allocator: std.mem.Allocator, io: std.Io) !Framework {
    const package_json = std.Io.Dir.cwd().readFileAlloc(io, "package.json", allocator, .limited(1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => null,
        else => return err,
    };
    defer if (package_json) |raw| allocator.free(raw);

    const pyproject = std.Io.Dir.cwd().readFileAlloc(io, "pyproject.toml", allocator, .limited(1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => null,
        else => return err,
    };
    defer if (pyproject) |raw| allocator.free(raw);

    return frameworkFromManifests(package_json, pyproject);
}

test "react manifest selects the react package" {
    const framework = frameworkFromManifests("{\"dependencies\":{\"react\":\"19.0.0\"}}", null);
    try std.testing.expectEqual(Framework.react, framework);
    try std.testing.expectEqualStrings("@authdog/react", framework.npmPackage().?);
}

test "nextjs wins over react" {
    const framework = frameworkFromManifests("{\"dependencies\":{\"next\":\"15.0.0\",\"react\":\"19.0.0\"}}", null);
    try std.testing.expectEqual(Framework.nextjs, framework);
}

test "fastapi pyproject selects the python package" {
    const framework = frameworkFromManifests(null, "[project]\ndependencies = [\"fastapi\"]\n");
    try std.testing.expectEqual(Framework.fastapi, framework);
    try std.testing.expectEqualStrings("authdog", framework.pythonPackage().?);
}

test "python project without fastapi stays unknown" {
    const framework = frameworkFromManifests(null, "[project]\nname = \"other\"\n");
    try std.testing.expectEqual(Framework.unknown, framework);
}
