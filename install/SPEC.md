# CLI install Worker (`cli.auth.dog`)

Cloudflare Worker that serves installers and resolves GitHub Release download URLs for **`authdog-cli`** (see `.github/workflows/release.yml` archive naming).

## Deploy

### Locally

```bash
cd install
npm install
npm run deploy
```

`wrangler.toml` includes **`account_id`** for the Authdog Cloudflare account (aligned with **platform-next**). Override locally with **`CLOUDFLARE_ACCOUNT_ID`** if needed.

### CI (GitHub Actions)

Workflow **`.github/workflows/cli-install-deploy.yml`** runs on **`workflow_dispatch`** and on pushes to **`main`** that touch **`install/**`**. Configure repo secret **`CLOUDFLARE_API_TOKEN`** (Workers Scripts Edit — same token pattern as platform-next).

Steps mirror platform-next MCP deploy: **`tokens/verify`** check, then **`npm ci`** + **`npm run deploy`** from **`install/`**.

Attach routes so **`https://cli.auth.dog`** (or another hostname) invokes this Worker.

## Environment (`wrangler.toml` `[vars]`)

| Variable       | Purpose                                      |
|----------------|----------------------------------------------|
| `GITHUB_REPO`  | `owner/name` on GitHub (default `authdog/cli`) |
| `BIN_NAME`     | Binary/archive prefix (default `authdog-cli`) |

No secrets required for public repos.

## HTTP surface

All handlers allow **`GET`** and **`HEAD`**.

| Path               | Response |
|--------------------|----------|
| `/`                | Plain-text usage (curl examples). |
| `/health`          | `200` body `ok`. |
| `/install`         | Bash installer script (`text/plain`). Run with **`curl -fsSL …/install`** and pipe to **`bash`** (see **`/`** on a deployed Worker for the exact URL). |
| `/install.sh`      | Same body as `/install` (alias). |
| `/install.ps1`     | PowerShell installer (`text/plain`). |
| `/v1/binary-url`   | Plain-text single line: HTTPS URL of the matching release asset. |

### `GET /v1/binary-url`

**Query**

| Param      | Required | Description |
|------------|----------|-------------|
| `target`   | yes      | Rust triple; must match a CI matrix target (see `RELEASE_TARGETS` in `src/index.ts`). |
| `version`  | no       | Exact release tag (bare semver, no leading `v`). Omit → GitHub **`releases/latest`** (stable only); if none, newest release **`releases?per_page=1`** (includes prereleases). |

**Success:** `200`, `Content-Type: text/plain`, body = asset URL.

**Errors:** `400` JSON if `target` missing/unknown; `502` JSON if GitHub API or asset lookup fails.

## Outbound calls

Worker uses **`fetch`** to **`https://api.github.com`** with `User-Agent: cli-install-worker`. Use hostname URLs only (not raw IPs).

## Install scripts (behaviour summary)

- **POSIX (`/install`)**: Chooses triple from `uname`; optional **`AUTHDOG_CLI_USE_MUSL=1`** on Linux x86_64/aarch64; optional **`AUTHDOG_CLI_VERSION`**; installs to **`INSTALL_DIR`** or **`$HOME/.local/bin`**.
- **Windows (`/install.ps1`)**: **`x86_64-pc-windows-msvc`** zip; optional **`AUTHDOG_CLI_VERSION`**, **`INSTALL_DIR`** (default under `%LOCALAPPDATA%\Programs\authdog-cli`).
