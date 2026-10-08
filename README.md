# Authdog CLI

CLI for **[Authdog](https://www.authdog.com)** identity authentication.

## Installation

Install a prebuilt binary from **[cli.auth.dog](https://cli.auth.dog)** (picked for your OS/arch from GitHub Releases):

```bash
curl https://cli.auth.dog/install -fsS | bash
```

Ensure **`$HOME/.local/bin`** (or your **`INSTALL_DIR`**) is on **`PATH`**—the installer defaults there. Options such as **`AUTHDOG_CLI_VERSION`**, **`INSTALL_DIR`**, and Linux musl are documented in **[`install/SPEC.md`](install/SPEC.md)**. Windows installers: **`install.ps1`** at the same host (see SPEC).

Maintainers deploy the Worker from **`install/`** via **[`.github/workflows/cli-install-deploy.yml`](.github/workflows/cli-install-deploy.yml)**; routing and **`wrangler.toml`** vars are covered in SPEC.

## Requirements (build from source)

- **Zig** 0.16.0 (`zig` on `PATH`).

Optional:

- **[just](https://just.systems)** for repository recipes.
- **[moon](https://moonrepo.dev)** on PATH for `just moon-build` / `just moon-test`.

## GitHub Releases

When a tag like **`0.1.0`** or **`0.1.0-beta.1`** (bare semver — **no** leading **`v`**) is pushed to **`origin`** on GitHub, the **Release** workflow (`.github/workflows/release.yml`) cross-builds **`authdog`**, attaches `authdog-cli-<version>-<target>` archives + **`checksums.sha256`**, and creates/updates that tag’s **[GitHub Release](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)**. Archives include **`authdog`** and a one-release **`authdog-cli`** compatibility copy. Archive names keep the installer target triples (including `x86_64-pc-windows-msvc`). Use **`just tag-push`** (or Actions **Create release tag**) so the tag matches `release.toml` `version` and the **`stable`** flag.

## Build & run

```bash
zig build                 # debug, installs zig-out/bin/authdog
zig build -Doptimize=ReleaseSafe
zig build run -- --help
```

## Commands

```bash
authdog login
authdog status
authdog logout
authdog whoami
authdog link --tenant-id … --application-id … --environment-id …
authdog init [--dry-run] [--framework nextjs]
authdog config pull
authdog doctor [--mcp]
authdog webhooks verify --secret … --signature … --body '…'
authdog impersonate --user-id …
authdog deploy status
authdog mcp install
```

Use **`--json`** or **`-o json`** anywhere in the command line for machine-readable output:

```bash
authdog status --json
authdog doctor --mcp -o json
```

Project linking writes **`.authdog/project.json`**; credentials stay in **`credentials.json`**. See **[`SPEC.md`](SPEC.md)** for the full command spec.

`status` never prints access or refresh tokens. Set **`AUTHDOG_CONFIG_DIR`** to override the local config directory for isolated development or CI; normal installs continue using the existing `authdog-cli` config directory.

### Environment variables

| Variable | Purpose |
|----------|---------|
| `AUTHDOG_IDENTITY_ORIGIN` | Identity host (default `https://identity.authdog.com`) |
| `AUTHDOG_CONSOLE_ENVIRONMENT_ID` | Sign-in environment UUID (hosted console default wired in sources) |
| `AUTHDOG_CONFIG_DIR` | Optional config-directory override; default remains the OS-specific `authdog-cli` directory |
| `AUTHDOG_API_ORIGIN` | REST API base URL (default `https://api.authdog.com`) |

## Just recipes

| Recipe | Description |
|--------|-------------|
| `just` / `just build` | `zig build` |
| `just release` | `zig build -Doptimize=ReleaseSafe` |
| `just run [ARGS]…` | `zig build run` with optional arguments |
| `just check` | `zig build` |
| `just test` | `zig build test` |
| `just fmt` | `zig fmt` |
| **`just moon-build`** | `moon run authdog-cli:build` (release build of the desktop CLI) |
| **`just moon-test`** | `moon run authdog-cli:test` |
| `just clean` | remove `zig-out` and `.zig-cache` |

## Layout

- **`src/main.zig`** — process entry.
- **`src/cli.zig`** — argument parsing and dispatch.
- **`src/commands.zig`** — `whoami`, `link`, `init`, `doctor`, etc.
- **`src/api/client.zig`** — `/v1` HTTP client.
- **`src/project.zig`** — `.authdog/project.json` manifest.
- **`SPEC.md`** — product spec and API mapping.
- **`src/login.zig`** — browser login, loopback callback, and token redeem.
- **`src/session.zig`** — `credentials.json` storage.
- **`assets/oauth_callback_success.html`** — page served on the loopback callback.
- **`release.toml`** — version and whether tags are stable or `beta.n`.

## Offline note

`login` succeeds only with network connectivity to Identity and a reachable loopback OAuth callback. Offline usage is limited to reading stored credentials (`status`).

## License

MIT
