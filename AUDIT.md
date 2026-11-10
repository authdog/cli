# Authdog CLI Audit Report

**Date**: November 2026  
**Auditor**: Automated repository audit  
**Repository**: authdog/cli  
**Scope**: Architecture, UX, tests, CI/CD, security, SDK alignment  

---

## Executive Summary

The Authdog CLI is a Zig-based command-line tool for authenticating users, managing projects, and operating the Authdog platform from the terminal. The codebase has a solid foundation with comprehensive product specification (`SPEC.md`), cross-platform release automation, and a working OAuth browser flow. However, there are notable gaps in test coverage, error handling UX, shell integration, and alignment with modern CLI standards set by tools like `gh`, `vercel`, and Clerk's CLI.

**Overall Maturity**: Early production (beta)

---

## Table of Contents

1. [Current State Summary](#1-current-state-summary)
2. [Top Findings with Evidence](#2-top-findings-with-evidence)
3. [Prioritized Improvements](#3-prioritized-improvements)
4. [Phased Task List](#4-phased-task-list)
5. [Open Questions](#5-open-questions)
6. [Appendix: Comparison Matrix](#appendix-comparison-matrix)

---

## 1. Current State Summary

### 1.1 Architecture Overview

| Aspect | Current State |
|--------|---------------|
| Language | Zig 0.16.0 |
| Build System | Zig build + Moon monorepo + just recipes |
| Lines of Code | ~3,500 lines across 15 source files |
| Dependencies | Zig stdlib only (minimal attack surface) |
| Distribution | GitHub Releases + Cloudflare Worker installer |

**Source File Breakdown**:
- `src/commands.zig` (2,200+ lines) - All command implementations
- `src/cli.zig` (1,050 lines) - Argument parsing and dispatch
- `src/login.zig` (560 lines) - OAuth browser flow
- `src/agent.zig` (780 lines) - Agent sign-in flows
- `src/mcp_config.zig` (510 lines) - MCP client configuration

### 1.2 Implemented Commands

| Category | Commands | Status |
|----------|----------|--------|
| Auth | `login`, `logout`, `status`, `whoami` | ✅ Complete |
| Project | `link`, `unlink`, `init` | ✅ Complete |
| Config | `config pull`, `config diff`, `config patch` | ✅ Complete |
| Webhooks | `webhooks listen`, `webhooks verify` | ✅ Complete |
| Operations | `impersonate`, `deploy status`, `doctor` | ✅ Complete |
| MCP | `mcp install`, `mcp list`, `mcp uninstall` | ✅ Complete |
| Agent | `agent doctor/prove/rp-init/wait/onboard` | ✅ Complete |
| Device | `device authorize` | ✅ Complete |
| API | `api METHOD PATH` | ✅ Complete |

### 1.3 Test Coverage

| Test Type | Files | Coverage |
|-----------|-------|----------|
| Unit tests | Embedded in source | Parsing, formatting, URL construction |
| Integration | `cli_checks.zig` | Help output, status redaction, logout |
| E2E/API | None | No mock server or contract tests |
| Snapshot | None | No golden file testing |

### 1.4 CI/CD Pipeline

| Workflow | Triggers | Purpose |
|----------|----------|---------|
| `ci.yml` | Push, PR | Lint (zig fmt), build, test on Linux/macOS |
| `release.yml` | Tags | Cross-compile 9 targets, publish GitHub Release |
| `cli-install-deploy.yml` | install/ changes | Deploy Cloudflare Worker installer |
| `create-tag.yml` | Manual | Create release tags from release.toml |

---

## 2. Top Findings with Evidence

### 2.1 Architecture & Code Quality

#### Finding A1: Monolithic command file approaching maintainability limits

**Evidence**: `src/commands.zig` is 2,230+ lines containing all command implementations.

```
     1|//! Command implementations for the Authdog CLI.
   ...
  2230|}
```

**Impact**: High cognitive load for contributors, merge conflicts, difficulty isolating changes.

**Recommendation**: Split into `src/commands/*.zig` modules (one per command group).

---

#### Finding A2: No structured error types with recovery hints

**Evidence**: Errors are reported as strings without actionable guidance.

```zig:79:92:src/cli.zig
error.AuthRequired => {
    try out.errPrint(io, allocator, "not logged in; run `authdog login`", .{});
    return 3;
},
```

**Impact**: Users hit errors without clear next steps beyond the basic message.

**Comparison**: `gh` and `vercel` provide rich error context with documentation links.

---

#### Finding A3: Session refresh not implemented (by design, but unclear to users)

**Evidence**: From `SPEC.md`:

> **Resolved (2026-10-17):** Identity CLI OAuth exposes `start`, `poll`, and `redeem` only. Redeem returns `access_token` and `refresh_token` and does not offer a refresh route. Expired sessions must ask the operator to `login` again.

`src/session.zig` stores `refresh_token` but it's never used.

**Impact**: Users see cryptic 401 errors instead of clear "session expired" messages.

---

### 2.2 Command UX & Developer Experience

#### Finding U1: Help text is comprehensive but dense

**Evidence**: `src/cli.zig` help text is a 40-line wall of text with no grouping.

```zig:174:216:src/cli.zig
const help_text =
    \\Usage: authdog [OPTIONS] <COMMAND>
    \\
    \\Authdog CLI — authenticate and manage projects from the terminal.
    \\
    \\Commands:
    \\  login                 Sign in through the hosted browser flow
    \\  logout                Delete locally stored credentials
    ...
```

**Comparison**: 
- `gh` groups commands by category (repo, pr, issue, etc.)
- `vercel` uses subcommand groups with `vercel help <topic>`
- Clerk CLI shows interactive menu when invoked with no args

**Impact**: New users struggle to find relevant commands.

---

#### Finding U2: No shell completions

**Evidence**: No completion scripts in repository. No `authdog completion` command.

**Comparison**: `gh completion -s bash/zsh/fish/powershell`, `vercel completion`

**Impact**: Users must type full command names; discoverability suffers.

---

#### Finding U3: Error messages lack context for common failures

**Evidence**: API failures show generic HTTP status.

```zig:156:168:src/api/client.zig
pub fn apiErrorMessage(allocator: std.mem.Allocator, resp: Response) ![]const u8 {
    const parsed = std.json.parseFromSlice(struct { @"error": ?[]const u8 = null }, allocator, resp.body, .{
        .ignore_unknown_fields = true,
    }) catch {
        return std.fmt.allocPrint(allocator, "HTTP {d}", .{resp.status});
    };
```

**Comparison**: WorkOS CLI shows specific remediation for each error code.

---

#### Finding U4: `init` flow is non-interactive compared to competitors

**Evidence**: `authdog init` detects framework and links, but doesn't prompt for choices.

```zig:316:325:src/commands.zig
if (framework == .unknown and opts.non_interactive) {
    try out.fail(allocator, io, opts.format, "could not detect framework; pass --framework");
}
```

**Comparison**: 
- Clerk CLI: Interactive wizard with framework selection, env var naming
- Vercel: `vercel link` prompts for project selection with fuzzy search

---

#### Finding U5: Output formatting is functional but not polished

**Evidence**: Text output uses simple strings; no colored status indicators.

```zig:1428:1432:src/commands.zig
for (checks.items) |c| {
    const mark = if (c.ok) "ok" else "FAIL";
    try out.print(io, std.Io.File.stdout(), "[{s}] {s}: {s}\n", .{ mark, c.name, c.detail });
}
```

**Comparison**: `gh`, `vercel` use colored checkmarks, spinners, progress bars.

---

### 2.3 Missing Features

#### Finding F1: No self-update mechanism

**Evidence**: No `authdog update` or `authdog upgrade` command.

**Comparison**: `gh extension upgrade`, Homebrew/Scoop integrations.

---

#### Finding F2: No shell aliases or configuration file

**Evidence**: No `~/.config/authdog/config.toml` or alias support.

**Comparison**: `gh` has `gh alias set`, global config for defaults.

---

#### Finding F3: Starter templates not implemented

**Evidence**: From `SPEC.md`:

> Starter templates, temporary webhook tunnel rewrites, and a generic API client remain outside this feature.

`authdog init --starter` mentioned but not implemented.

---

#### Finding F4: No `authdog open` to launch console

**Evidence**: No command to open web console in browser for current project.

**Comparison**: `vercel open`, `gh repo view --web`, `clerk open`

---

### 2.4 Documentation & README

#### Finding D1: README is comprehensive but lacks quick-start video/GIF

**Evidence**: README at 210 lines, good structure, but text-only.

**Comparison**: Clerk, WorkOS, and most modern CLI tools include terminal recordings.

---

#### Finding D2: No CONTRIBUTING.md or developer setup guide

**Evidence**: No contribution guidelines. Build instructions in README are minimal.

**Comparison**: `gh` has detailed CONTRIBUTING.md with architecture docs.

---

#### Finding D3: No man pages or `authdog help <command>` detailed help

**Evidence**: `--help` shows all commands; no per-command detailed help.

```bash
authdog init --help  # Shows global help, not init-specific
```

---

### 2.5 Tests & Coverage

#### Finding T1: No API mock server for integration tests

**Evidence**: Tests in source files only test parsing/formatting.

```zig:920:926:src/cli.zig
test "parses login command" {
    var env = std.process.Environ.Map.init(std.testing.allocator);
    defer env.deinit();
    const parsed = try parse(std.testing.allocator, &env, &.{"login"});
    try std.testing.expect(parsed == .run);
    try std.testing.expect(parsed.run.command == .login);
}
```

**Impact**: Command implementations untested beyond parsing.

---

#### Finding T2: No snapshot/golden file tests for JSON output

**Evidence**: JSON output schema not validated by tests.

**Impact**: Breaking changes to `--json` output go undetected.

---

#### Finding T3: CLI checks only cover help and logout

**Evidence**: `cli_checks.zig` tests help output and credential removal.

```zig:21:31:src/cli_checks.zig
fn checkHelp(allocator: std.mem.Allocator, io: std.Io, exe: []const u8) !void {
    const result = try run(allocator, io, null, exe, &.{"--help"});
    defer result.deinit(allocator);
    try expectExit(result.term, 0);
    try expectContains(result.stdout, "login");
    ...
}
```

**Impact**: Most commands have no integration test coverage.

---

### 2.6 CI, Release & Distribution

#### Finding C1: No Windows CI runner

**Evidence**: CI matrix in `.github/workflows/ci.yml`:

```yaml:36:40:.github/workflows/ci.yml
matrix:
  os:
    - ubuntu-latest
    - macos-latest
```

**Impact**: Windows-specific bugs (paths, process spawning) may ship.

---

#### Finding C2: No code coverage reporting

**Evidence**: No coverage tool configured (kcov, grcov equivalent for Zig).

---

#### Finding C3: Release notes are auto-generated without categorization

**Evidence**: `release.yml` uses `generate_release_notes: true` without template.

```yaml:133:137:.github/workflows/release.yml
- uses: softprops/action-gh-release@v2
  with:
    tag_name: ${{ env.RELEASE_TAG }}
    files: dist/*
    generate_release_notes: true
```

**Comparison**: Categorized changelogs (Features, Fixes, Breaking Changes).

---

### 2.7 Dependency Health & Security

#### Finding S1: No dependency scanning or SBOM

**Evidence**: No Dependabot, Snyk, or SBOM generation.

The install worker (`install/`) uses npm dependencies:

```json:install/package.json
{
  "devDependencies": {
    "wrangler": "^3.x"
  }
}
```

---

#### Finding S2: No security policy (SECURITY.md)

**Evidence**: File not present. No vulnerability disclosure process documented.

---

#### Finding S3: Credentials stored in plaintext JSON

**Evidence**: `credentials.json` with mode 0600, but no encryption.

```zig:57:64:src/cli_checks.zig
try std.Io.Dir.cwd().writeFile(io, .{
    .sub_path = creds,
    .data =
    \\{
    \\  "access_token": "access-secret",
    \\  "refresh_token": "refresh-secret"
    \\}
    ,
});
```

**Comparison**: `gh` uses system keychain on macOS, credential helpers.

---

### 2.8 SDK Alignment

#### Finding K1: Framework detection covers SDK package names

**Evidence**: `src/detect.zig` correctly maps to SDK packages:

```zig:22:30:src/detect.zig
pub fn npmPackage(self: Framework) ?[]const u8 {
    return switch (self) {
        .nextjs => "@authdog/nextjs",
        .express => "@authdog/express",
        .javascript => "@authdog/javascript",
        .react => "@authdog/react",
        ...
```

SDK packages in https://github.com/authdog/sdk:
- `@authdog/node-sdk` (Node.js)
- `authdog` (Python)

**Gap**: CLI uses `@authdog/javascript`, SDK repo has `@authdog/node-sdk`.

---

#### Finding K2: SDK wiring endpoint implemented

**Evidence**: `sdk_config.zig` fetches configuration:

```zig:src/sdk_config.zig
pub fn fetchSdkConfigPath(
    allocator: std.mem.Allocator,
    tenant_id: []const u8,
    environment_id: []const u8,
    application_id: []const u8,
) ![]u8 {
    return std.fmt.allocPrint(allocator, "/v1/tenants/{s}/environments/{s}/sdk-config?projectId={s}", ...);
}
```

**Status**: Aligned with platform API.

---

## 3. Prioritized Improvements

### Priority Matrix

| ID | Finding | Effort | Impact | Priority |
|----|---------|--------|--------|----------|
| U2 | Shell completions | M | High | P1 |
| U1 | Help text grouping | S | High | P1 |
| T1 | API mock server | L | High | P1 |
| A1 | Split commands.zig | M | Medium | P1 |
| U3 | Rich error messages | M | High | P1 |
| C1 | Windows CI | S | Medium | P1 |
| S2 | SECURITY.md | S | Medium | P1 |
| U4 | Interactive init | M | High | P2 |
| U5 | Colored output | S | Medium | P2 |
| T2 | JSON schema tests | M | Medium | P2 |
| F1 | Self-update | M | Medium | P2 |
| F4 | `authdog open` | S | Medium | P2 |
| D1 | Demo GIF/video | S | Medium | P2 |
| D2 | CONTRIBUTING.md | S | Low | P2 |
| C2 | Coverage reporting | M | Low | P3 |
| S1 | SBOM/scanning | M | Low | P3 |
| S3 | Keychain integration | L | Medium | P3 |
| K1 | SDK package name alignment | S | Low | P3 |
| F2 | Config file + aliases | M | Low | P3 |
| F3 | Starter templates | L | Medium | P3 |

**Effort Key**: S = Small (hours), M = Medium (days), L = Large (week+)

---

## 4. Phased Task List

### Phase 1: Foundation (Critical Path)

**Goal**: Establish testing infrastructure and fix highest-impact UX issues.

- [ ] **T1.1** Create API mock server for integration tests
  - Mock `/v1/userinfo`, `/v1/tenants`, `/v1/health` endpoints
  - Add test fixtures for error responses
  
- [ ] **T1.2** Add Windows to CI matrix
  - Update `.github/workflows/ci.yml` with `windows-latest`
  - Fix any path separator issues
  
- [ ] **T1.3** Implement shell completions
  - Add `authdog completion bash|zsh|fish|powershell` command
  - Include install instructions in README
  
- [ ] **T1.4** Group commands in help text
  - Categories: Auth, Project, Config, Webhooks, Operations, Agent
  - Add `authdog help <category>` for detailed help
  
- [ ] **T1.5** Create SECURITY.md
  - Vulnerability disclosure process
  - Security contact email
  - Supported versions

### Phase 2: Polish (User Experience)

**Goal**: Match UX standards of `gh` and `vercel` CLIs.

- [ ] **T2.1** Add colored output with spinner/progress
  - Use ANSI colors for success/error/warning
  - Spinner during network operations
  
- [ ] **T2.2** Enhance error messages with context
  - Add suggestion hints (e.g., "Did you mean...?")
  - Include documentation links for common errors
  
- [ ] **T2.3** Make `init` interactive when TTY detected
  - Framework selection menu
  - Environment selection from API
  
- [ ] **T2.4** Add `authdog open` command
  - Open console in browser for linked project
  
- [ ] **T2.5** Create demo GIF for README
  - Record `init` flow end-to-end

### Phase 3: Reliability (Testing & Coverage)

**Goal**: Prevent regressions and validate output contracts.

- [ ] **T3.1** Add JSON output schema tests
  - Golden file tests for each `--json` command
  - Schema versioning validation
  
- [ ] **T3.2** Split `commands.zig` into modules
  - `src/commands/init.zig`
  - `src/commands/config.zig`
  - `src/commands/webhooks.zig`
  - etc.
  
- [ ] **T3.3** Add code coverage reporting
  - Investigate Zig coverage options
  - Add coverage badge to README
  
- [ ] **T3.4** Create CONTRIBUTING.md
  - Development setup
  - Architecture overview
  - PR guidelines

### Phase 4: Ecosystem (Advanced Features)

**Goal**: Parity with competitor CLIs and deeper integration.

- [ ] **T4.1** Implement self-update mechanism
  - `authdog update` checks GitHub releases
  - Optional auto-update prompt
  
- [ ] **T4.2** Add configuration file support
  - `~/.config/authdog/config.toml`
  - Default API origin, output format
  
- [ ] **T4.3** Implement keychain credential storage
  - macOS Keychain, Windows Credential Manager
  - Fallback to current JSON file
  
- [ ] **T4.4** Align SDK package names with authdog/sdk
  - Audit `@authdog/javascript` vs `@authdog/node-sdk`
  - Update detection and install logic
  
- [ ] **T4.5** Add dependency scanning
  - Dependabot for `install/` npm deps
  - SBOM generation in release workflow

---

## 5. Open Questions

1. **SDK Package Naming**: The CLI installs `@authdog/javascript` but the SDK repo publishes `@authdog/node-sdk`. Which is canonical?

2. **Refresh Token Strategy**: The refresh token is stored but unused. Will Identity add a refresh route, or should the CLI stop storing it?

3. **TUI Mode**: `SPEC.md` mentions `authdog tui` and the installer hints at it. Is this a planned feature or deprecated reference?

4. **Webhook Body Availability**: The specs note delivery records may not include bodies. What's the platform timeline for adding them?

5. **Starter Templates**: Mentioned in SPEC.md but explicitly out of scope. Is this still planned for a future release?

6. **MCP Client Support**: Currently supports Cursor, Claude, VS Code, Codex. Are Windsurf, Continue, or other clients planned?

7. **Credential Encryption**: Is plaintext JSON acceptable for MVP, or should keychain integration be prioritized?

---

## Appendix: Comparison Matrix

| Feature | authdog | gh | vercel | clerk |
|---------|---------|-----|--------|-------|
| Shell completions | ❌ | ✅ | ✅ | ✅ |
| Colored output | ❌ | ✅ | ✅ | ✅ |
| Interactive prompts | Limited | ✅ | ✅ | ✅ |
| Self-update | ❌ | ✅ | ✅ | ✅ |
| Config file | ❌ | ✅ | ✅ | ❌ |
| Keychain storage | ❌ | ✅ | ❌ | ❌ |
| `--json` output | ✅ | ✅ | ✅ | ✅ |
| `--non-interactive` | ✅ | ✅ | ✅ | ✅ |
| Exit codes | ✅ | ✅ | ✅ | ✅ |
| MCP integration | ✅ | ❌ | ❌ | ❌ |
| Webhook forwarding | ✅ | ❌ | ❌ | ✅ |
| Browser OAuth | ✅ | ✅ | ✅ | ✅ |
| Cross-platform | ✅ | ✅ | ✅ | ✅ |
| Open source | ✅ | ✅ | ❌ | ❌ |

---

## Document History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | 2026-11-10 | Initial audit |
