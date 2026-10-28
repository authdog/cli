<p align="center">
  <img width="420" alt="Authdog" src="https://raw.githubusercontent.com/authdog/.github/main/profile/logo-light.svg#gh-light-mode-only" />
  <img width="420" alt="Authdog" src="https://raw.githubusercontent.com/authdog/.github/main/profile/logo-dark.svg#gh-dark-mode-only" />
</p>

<p align="center">
<b>Official CLI for Authdog — sign in, link a project, and run the console from the terminal.</b>
</p>

<p align="center">
  <a href="https://github.com/authdog/cli/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/authdog/cli/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://github.com/authdog/cli/releases"><img alt="GitHub Release" src="https://img.shields.io/github/v/release/authdog/cli?display_name=release&sort=semver" /></a>
  <a href="https://opensource.org/licenses/MIT"><img alt="License: MIT" src="https://img.shields.io/badge/License-MIT-yellow.svg" /></a>
</p>

<p align="center">
<a href="https://www.authdog.com">Website</a> ·
<a href="https://www.authdog.com/docs">Docs</a> ·
<a href="https://www.authdog.com/pricing">Pricing</a> ·
<a href="https://console.authdog.com">Console</a> ·
<a href="https://status.authdog.com">Status</a>
</p>

<p align="center">
<a href="#install">Install</a> ·
<a href="#quick-start">Quick start</a> ·
<a href="#commands">Commands</a> ·
<a href="#build-from-source">Build from source</a>
</p>

The [Authdog CLI](https://github.com/authdog/cli) is the command-line client for [Authdog](https://www.authdog.com) — Agentic Identity, Access Control, and Observability. It uses the same session as the console: browser sign-in, a linked tenant, project, and environment, then the management API.

`authdog init` detects the framework in the repo you already have, links the project, installs the SDK, and writes env vars. `--json` and `--non-interactive` keep CI and coding agents on stable output. Credentials stay in `credentials.json`. The repo stores `.authdog/project.json`, which has no secrets.

## Install

Prebuilt binaries ship from [cli.auth.dog](https://cli.auth.dog), resolved from [GitHub Releases](https://github.com/authdog/cli/releases) for your OS and architecture.

**Linux and macOS**

```bash
curl -fsSL https://cli.auth.dog/install | bash
```

The installer puts `authdog` in `$HOME/.local/bin` (override with `INSTALL_DIR`). Confirm that directory is on `PATH`. Skip the confirm prompt with `AUTHDOG_CLI_YES=1`. Pin a tag with `?version=` or `AUTHDOG_CLI_VERSION` placed before `bash`:

```bash
curl -fsSL https://cli.auth.dog/install | AUTHDOG_CLI_VERSION=0.1.0-beta.1 bash
```

Linux musl builds: set `AUTHDOG_CLI_USE_MUSL=1` before `bash`.

**Windows (PowerShell)**

```powershell
iwr -useb https://cli.auth.dog/install.ps1 | iex
```

Default install directory is `%LOCALAPPDATA%\Programs\authdog`. Installer flags, target triples, and the Cloudflare Worker that serves these scripts are documented in [`install/SPEC.md`](install/SPEC.md).

## Quick start

```bash
authdog login
authdog init
authdog whoami
```

`login` opens the hosted Account Portal and redeems the session on loopback. `init` detects the framework (pass `--framework nextjs` to force one), links tenant, project, and environment, and writes SDK env vars. `--dry-run` prints the plan without writing files.

From there, the same commands an agent would run:

```bash
authdog config pull
authdog config diff
authdog webhooks listen --forward http://127.0.0.1:3000/webhooks/authdog
authdog deploy status
authdog mcp install --client cursor
```

Add `--json` or `-o json` anywhere on the command line for machine-readable output. `--non-interactive` fails instead of prompting. Exit codes: `0` success, `2` usage, `3` auth required, `4` API error.

## Commands

| Command | What it does |
| --- | --- |
| `login` / `logout` / `status` | Browser sign-in, local sign-out, and whether a session is stored |
| `whoami` | Authenticated user from the API |
| `link` / `unlink` | Bind this directory to a tenant, application, and environment |
| `init` | Detect the framework, link, install the SDK, write env vars |
| `config pull` / `diff` / `patch` | Snapshot environment config, compare it, apply a patch (`--dry-run`) |
| `doctor` | Connectivity and auth checks (`--mcp`, `--agent-sign-in`) |
| `webhooks listen` / `verify` | Forward deliveries locally, or check `X-Authdog-Signature` offline |
| `impersonate` | Mint a short-lived sign-in URL; `revoke --grant-id` ends it |
| `deploy status` | Security posture and vanity-domain snapshot |
| `mcp install` / `list` / `uninstall` | Register the Authdog MCP server (`cursor`, `claude`, `vscode`, `codex`, or `--all`) |
| `agent doctor` / `prove` / `rp-init` / `wait` / `onboard` | Agent sign-in preflight, approval, OIDC client, poll, and onboard |
| `device authorize` | RFC 8628 device flow (`--poll`, or linked project) |
| `api METHOD PATH` | Call one `/v1` route (`--data` JSON for POST, PUT, PATCH) |

`auth login`, `auth logout`, and `auth status` are aliases. `status` reports whether you are logged in and where credentials live. Tokens stay on disk.

Full behavior, flags, and API mapping: [`SPEC.md`](SPEC.md).

### Session

Sign in once. Later commands reuse the saved bearer session and refresh it when the API requires it.

- **Browser login** — hosted OAuth, loopback callback on `127.0.0.1`, tokens stored in `credentials.json` (mode `0600`)
- **Who you are** — `whoami` calls userinfo with that session
- **Isolated config** — `AUTHDOG_CONFIG_DIR` points credentials and config at another directory for CI or a second account

### Project setup

One directory, one environment. `link` writes the manifest. `init` does that and wires the SDK.

- **Link** — `authdog link --tenant-id … --application-id … --environment-id …`
- **Init** — framework detection, package manager (`--pm`), env file (`--env-file`), `--dry-run`
- **Config** — `pull` writes `.authdog/config.json`; `diff` compares it to live; `patch --dry-run` previews an apply

### Agents and MCP

Agents drive the same binary. Give them `--json` and `--non-interactive`, and register MCP when they need live platform tools.

- **MCP** — `authdog mcp install --client cursor` registers the Authdog server; `list` and `uninstall` manage Cursor, Claude, VS Code, and Codex
- **Agent sign-in** — `agent doctor`, `agent prove --accept-disclosure`, `agent rp-init`, `agent wait --poll`, `agent onboard`
- **Device flow** — `authdog device authorize --poll` for clients that cannot open a browser

### Day-to-day operations

- **Webhooks** — `webhooks listen --forward URL` polls deliveries; optional tunnel restore with `--tunnel`, `--channel-id`, `--restore-url`, and `--yes`. `webhooks verify` checks a signature offline
- **Support** — `impersonate --user-id …` mints a sign-in URL; `impersonate revoke --grant-id …` revokes the grant
- **Go-live** — `deploy status` and `doctor` report posture before you ship
- **Any route** — `authdog api GET /v1/userinfo` calls one published `/v1` path and redacts secret fields before printing

## Configuration

| Variable | Purpose |
| --- | --- |
| `AUTHDOG_IDENTITY_ORIGIN` | Identity host (default `https://identity.authdog.com`) |
| `AUTHDOG_API_ORIGIN` | REST API base URL (default `https://api.authdog.com`) |
| `AUTHDOG_CONSOLE_ENVIRONMENT_ID` | Sign-in environment UUID |
| `AUTHDOG_CONFIG_DIR` | Config directory override; default is the OS `authdog-cli` directory |
| `AUTHDOG_CLI_VERSION` | Release tag for the installer (bare semver, no leading `v`) |
| `AUTHDOG_CLI_YES` | Skip the installer confirm prompt |
| `INSTALL_DIR` | Install location (default `$HOME/.local/bin`) |

Global flags: `--api-origin`, `--project-file` (default `.authdog/project.json`), `--non-interactive`, `--yes`, `-o json` / `--json`.

## Build from source

**Zig** 0.16.0 (`zig` on `PATH`).

Optional: [just](https://just.systems) for the recipes below, and [moon](https://moonrepo.dev) for `just moon-build` / `just moon-test`.

```bash
zig build                         # debug → zig-out/bin/authdog
zig build -Doptimize=ReleaseSafe
zig build run -- --help
zig build test
```

| Recipe | Description |
| --- | --- |
| `just` / `just build` | `zig build` |
| `just release` | `zig build -Doptimize=ReleaseSafe` |
| `just run [ARGS]…` | `zig build run` with optional arguments |
| `just test` | `zig build test` |
| `just fmt` | `zig fmt` |
| `just moon-build` | `moon run authdog-cli:build` |
| `just moon-test` | `moon run authdog-cli:test` |
| `just clean` | Remove `zig-out` and `.zig-cache` |

Tags are bare semver (`0.1.0`, `0.1.0-beta.1`) with no leading `v`. Pushing a tag runs [`.github/workflows/release.yml`](.github/workflows/release.yml), which cross-builds `authdog`, attaches `authdog-<version>-<target>` archives plus `checksums.sha256`, and publishes the GitHub Release. `just tag-push` (or Actions **Create release tag**) keeps the tag aligned with `release.toml`. The install Worker in `install/` is deployed by [`.github/workflows/cli-install-deploy.yml`](.github/workflows/cli-install-deploy.yml); see [`install/SPEC.md`](install/SPEC.md).

Repository map: `src/main.zig` (entry), `src/cli.zig` (parse and dispatch), `src/commands.zig`, `src/api/client.zig`, `src/login.zig`, `src/session.zig`, `src/project.zig`. Product spec: [`SPEC.md`](SPEC.md).

## Open source

| Repo | What it is |
| --- | --- |
| [cli](https://github.com/authdog/cli) | This repository — official CLI for management and identity services |
| [web-sdk](https://github.com/authdog/web-sdk) | Framework SDKs for web, mobile, and backends |
| [sdk](https://github.com/authdog/sdk) | Language SDKs for Authdog APIs |
| [keylab](https://github.com/authdog/keylab) | JWT create/verify without rolling your own crypto |
| [agent-skills](https://github.com/authdog/agent-skills) | Skills for agentic onboarding onto Authdog |

Point an agent at [`/llms.txt`](https://www.authdog.com/llms.txt) — every guide is also available as Markdown. Product overview: [CLI](https://www.authdog.com/product/cli).

## FAQ

**What is the Authdog CLI?**
The official terminal client for Authdog. It signs you in, links a repo to an environment, and runs console operations — init, config, webhooks, impersonation, deploy checks, MCP, and direct `/v1` calls.

**How do I install it?**
`curl -fsSL https://cli.auth.dog/install | bash` on Linux and macOS. PowerShell: `iwr -useb https://cli.auth.dog/install.ps1 | iex`.

**Where do credentials live?**
In `credentials.json` under the OS config directory for `authdog-cli`, overridable with `AUTHDOG_CONFIG_DIR`. `.authdog/project.json` in the repo holds tenant, application, and environment ids only.

**Can an agent run it?**
Yes. `authdog init` and the other commands accept `--json` and `--non-interactive`. `authdog mcp install` registers the Authdog MCP server for Cursor, Claude, VS Code, or Codex.

**Does login work offline?**
`login` needs the identity host and a reachable loopback callback. With a saved session, `status` reads local credentials without the network.

## License

[MIT](https://opensource.org/licenses/MIT)
