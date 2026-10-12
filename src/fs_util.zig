//! POSIX vs Windows file permission helpers for atomic writes.

const builtin = @import("builtin");
const std = @import("std");

pub fn privateFileMode() std.Io.File.Permissions {
    if (builtin.os.tag == .windows) return .default_file;
    return .fromMode(0o600);
}

pub fn sharedFileMode() std.Io.File.Permissions {
    if (builtin.os.tag == .windows) return .default_file;
    return .fromMode(0o644);
}
