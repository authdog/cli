---
name: authdog-cli-platform
description: >-
  Authdog CLI and hosted API context (REST origins, OAuth, env vars, code maps).
  Use when changing authdog-cli, Identity sign-in or redeem URLs, just recipes,
  or when the user mentions Authdog API location, AUTHDOG_* env vars, Management
  GraphQL backing the API worker, or oauth callback.
disable-model-invocation: true
---

# Authdog CLI and platform surface

The CLI is a Zig program (`build.zig`, `src/`). A sibling **`platform-next/`** checkout may exist **one directory up** (`../platform-next`): API/Management implementations live there, not inside this repository.

## Hosted origins (production defaults)

| Role | Origin | Typical paths |
|------|--------|----------------|
| **REST API** | `https://api.authdog.com` | `/v1/userinfo`, `/v1/tenants`, `/v1/openapi`, favicon asset |
| **Identity** | `https://identity.authdog.com` | `/signin/{environmentId}?cli_sess=…&cli_redirect=…`, `POST /api/v1/cli/oauth/redeem`, `GET /api/v1/cli/oauth/poll` |
| JWKS reference (management token verify) | `https://id.authdog.com/.well-known/jwks.json` | (server-side Management; not CLI) |

Override: **`AUTHDOG_IDENTITY_ORIGIN`**.

## CLI defaults (source of truth)

- Identity: **`default_identity_origin`** (`AUTHDOG_IDENTITY_ORIGIN`), **`default_console_environment_id`** (`AUTHDOG_CONSOLE_ENVIRONMENT_ID`) in `src/login.zig`.
- OAuth loopback path: **`loopback_oauth_redirect_path`** (`/oauth/callback`).

## OAuth (browser ↔ CLI)

1. CLI listens **`127.0.0.1:0`** HTTP; redirect URL **`http://127.0.0.1:{port}/oauth/callback`**.
2. Browser returns **`GET /oauth/callback?grant=<64 hex>`**; CLI replies with **`assets/oauth_callback_success.html`** (logo from **`https://api.authdog.com/favicon.ico`** unless changed).
3. Tokens: **`POST {identity}/api/v1/cli/oauth/redeem`**.
4. Session file: **`~/.config/authdog-cli/credentials.json`** on Linux (`src/session.zig`), mode `0600`. macOS uses `~/Library/Application Support/com.Authdog.authdog-cli`. Windows uses `%APPDATA%\Authdog\authdog-cli`.

## CLI commands

| Command | Behaviour |
|---------|-----------|
| `login` | Opens Identity sign-in in browser; receives loopback callback and exchanges tokens. |
| `logout` | Deletes saved credentials locally. |
| `status` | Reports whether user is logged in and path to credentials. Never prints tokens. |

Extend **reference.md** only when more tables or troubleshooting steps are needed.

## justfile (repo root)

- **`just build`** / **`just test`**: `zig build` and `zig build test`
- Version and beta tagging: **`release.toml`**

## Quick edit map

| Area | Path |
|------|------|
| OAuth / redeem | `src/login.zig` |
| CLI routing | `src/cli.zig` |
| Session store | `src/session.zig` |
| Process entry | `src/main.zig` |

Do not print access or refresh tokens from `status`. Never paste production tokens into chats.
