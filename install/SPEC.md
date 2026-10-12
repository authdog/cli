# CLI install Worker (`cli.auth.dog`)

Cloudflare Worker that serves installers for **`authdog`** and resolves GitHub Release assets named **`authdog-<tag>-<target>`** (falls back to legacy **`authdog-cli-*`** archives when needed).

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
| `BIN_NAME`     | Release archive prefix (default `authdog`) |

No secrets required for public repos.

## HTTP surface

All handlers allow **`GET`** and **`HEAD`**.

| Path               | Response |
|--------------------|----------|
| `/`                | Plain-text usage (curl examples). |
| `/health`          | `200` body `ok`. |
| `/install`         | Bash installer script (`text/plain`). Optional query **`?version=<tag>`** (or **`?version=latest`**). Resolves newest tag via **`/v1/release-tag`** when unpinned. Run with **`curl -fsSL …/install \| bash`** — put **`AUTHDOG_CLI_VERSION`** before **`bash`**, not before **`curl`**. |
| `/install.sh`      | Same body as `/install` (alias). |
| `/install.ps1`     | PowerShell installer (`text/plain`). |
| `/v1/binary-url`   | Plain-text single line: HTTPS URL of the matching release asset. |
| `/v1/release-tag`  | Plain-text newest **published** release tag (max semver, includes prereleases). |

### `GET /v1/binary-url`

**Query**

| Param      | Required | Description |
|------------|----------|-------------|
| `target`   | yes      | Release target triple; must match `RELEASE_TARGETS` in `src/index.ts`. |
| `version`  | no       | Exact release tag (bare semver, no leading `v`), or `latest`. Omit → highest semver among published GitHub Releases (includes prereleases). Tag must have Release assets (git tag alone is insufficient). |

**Success:** `200`, `Content-Type: text/plain`, body = asset URL.

**Errors:** `400` JSON if `target` missing/unknown; `502` JSON if GitHub API or asset lookup fails.

## Outbound calls

Worker uses **`fetch`** to **`https://api.github.com`** with `User-Agent: cli-install-worker`. Use hostname URLs only (not raw IPs).

## Install scripts (behaviour summary)

- **POSIX (`/install`)**: Chooses triple from `uname`; resolves **`/v1/release-tag`** when unpinned; prints release/channel/platform/dest and prompts **`Proceed with install? [y/N]`** (reads **`/dev/tty`** when stdin is a pipe). Skip prompt with **`AUTHDOG_CLI_YES=1`**. Installs **`authdog`** into **`INSTALL_DIR`**. Post-install hints mention the interactive TUI (`authdog` / `authdog tui`). Optional **`AUTHDOG_CLI_USE_MUSL=1`**, **`AUTHDOG_CLI_VERSION`**, **`INSTALL_DIR`** (default **`$HOME/.local/bin`**).
- **Windows (`/install.ps1`)**: Same summary + **`Read-Host`** confirm; **`AUTHDOG_CLI_YES=1`** skips; **`x86_64-pc-windows-msvc`** zip; default install under **`%LOCALAPPDATA%\Programs\authdog`**.
