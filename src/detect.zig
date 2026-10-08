//! Framework and package-manager detection for `init`.

const std = @import("std");

pub const Framework = enum {
    nextjs,
    express,
    javascript,
    fastapi,
    unknown,

    pub fn parse(name: []const u8) ?Framework {
        if (std.mem.eql(u8, name, "nextjs") or std.mem.eql(u8, name, "next")) return .nextjs;
        if (std.mem.eql(u8, name, "express")) return .express;
        if (std.mem.eql(u8, name, "javascript") or std.mem.eql(u8, name, "react")) return .javascript;
        if (std.mem.eql(u8, name, "fastapi") or std.mem.eql(u8, name, "python")) return .fastapi;
        return null;
    }

    pub fn npmPackage(self: Framework) ?[]const u8 {
        return switch (self) {
            .nextjs => "@authdog/nextjs",
            .express => "@authdog/express",
            .javascript => "@authdog/javascript",
            .fastapi => null,
            .unknown => null,
        };
    }

    pub fn label(self: Framework) []const u8 {
        return @tagName(self);
    }
};

pub fn detectFramework(allocator: std.mem.Allocator, io: std.Io) !Framework {
    const package_json = std.Io.Dir.cwd().readFileAlloc(io, "package.json", allocator, .limited(1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => null,
        else => return err,
    };
    if (package_json) |raw| {
        defer allocator.free(raw);
        if (std.mem.indexOf(u8, raw, "\"next\"") != null) return .nextjs;
        if (std.mem.indexOf(u8, raw, "@authdog/nextjs") != null) return .nextjs;
        if (std.mem.indexOf(u8, raw, "@authdog/express") != null) return .express;
        if (std.mem.indexOf(u8, raw, "@authdog/javascript") != null or std.mem.indexOf(u8, raw, "@authdog/react") != null) return .javascript;
        if (std.mem.indexOf(u8, raw, "\"express\"") != null) return .express;
    }

    const pyproject = std.Io.Dir.cwd().readFileAlloc(io, "pyproject.toml", allocator, .limited(1024 * 1024)) catch |err| switch (err) {
        error.FileNotFound => null,
        else => return err,
    };
    if (pyproject != null) {
        defer allocator.free(pyproject.?);
        return .fastapi;
    }

    return .unknown;
}
