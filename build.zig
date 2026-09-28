const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const release = readRelease(b);

    const options = b.addOptions();
    options.addOption([]const u8, "version", release.version);
    _ = release.stable;

    const exe = b.addExecutable(.{
        .name = "authdog",
        .version = std.SemanticVersion.parse(release.version) catch @panic("release.toml version is not semver"),
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
            .strip = optimize != .Debug,
        }),
    });
    exe.root_module.addOptions("build_options", options);
    const callback_page = callbackPageModule(b);
    exe.root_module.addImport("callback_page", callback_page);
    b.installArtifact(exe);

    const run_cmd = b.addRunArtifact(exe);
    run_cmd.step.dependOn(b.getInstallStep());
    if (b.args) |args| run_cmd.addArgs(args);
    const run_step = b.step("run", "Run the Authdog CLI");
    run_step.dependOn(&run_cmd.step);

    const unit_mod = b.createModule(.{
        .root_source_file = b.path("src/cli.zig"),
        .target = target,
        .optimize = optimize,
    });
    unit_mod.addOptions("build_options", options);
    unit_mod.addImport("callback_page", callback_page);
    const unit_tests = b.addTest(.{
        .root_module = unit_mod,
    });
    const run_unit_tests = b.addRunArtifact(unit_tests);

    const checks = b.addExecutable(.{
        .name = "cli-checks",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/cli_checks.zig"),
            .target = target,
            .optimize = optimize,
        }),
    });
    const run_checks = b.addRunArtifact(checks);
    run_checks.addFileArg(exe.getEmittedBin());

    const test_step = b.step("test", "Run unit tests and CLI checks");
    test_step.dependOn(&run_unit_tests.step);
    test_step.dependOn(&run_checks.step);
}

const ReleaseInfo = struct {
    version: []const u8,
    stable: bool,
};

fn callbackPageModule(b: *std.Build) *std.Build.Module {
    const files = b.addWriteFiles();
    _ = files.addCopyFile(b.path("assets/oauth_callback_success.html"), "oauth_callback_success.html");
    return b.createModule(.{
        .root_source_file = files.add("callback.zig",
            \\pub const html = @embedFile("oauth_callback_success.html");
            \\
        ),
    });
}

fn readRelease(b: *std.Build) ReleaseInfo {
    const text = b.build_root.handle.readFileAlloc(
        b.graph.io,
        "release.toml",
        b.allocator,
        .limited(16 * 1024),
    ) catch @panic("failed to read release.toml");

    var version: ?[]const u8 = null;
    var stable = false;
    var lines = std.mem.splitScalar(u8, text, '\n');
    while (lines.next()) |raw| {
        const line = std.mem.trim(u8, raw, " \t\r");
        if (line.len == 0 or line[0] == '#') continue;
        if (std.mem.startsWith(u8, line, "version")) {
            version = parseTomlString(b, line);
        } else if (std.mem.startsWith(u8, line, "stable")) {
            stable = std.mem.indexOf(u8, line, "true") != null;
        }
    }
    return .{
        .version = version orelse @panic("release.toml is missing version"),
        .stable = stable,
    };
}

fn parseTomlString(b: *std.Build, line: []const u8) []const u8 {
    const open = std.mem.indexOfScalar(u8, line, '"') orelse @panic("release.toml version must be a quoted string");
    const rest = line[open + 1 ..];
    const close = std.mem.indexOfScalar(u8, rest, '"') orelse @panic("release.toml version must be a quoted string");
    return b.dupe(rest[0..close]);
}
