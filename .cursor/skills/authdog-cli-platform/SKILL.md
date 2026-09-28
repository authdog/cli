---
name: authdog-cli-platform
description: >-
  Authdog CLI and hosted API context (REST origins, OAuth, env vars, code maps).
  Use when changing authdog-cli, /v1/userinfo or /v1/tenants, Identity sign-in or
  redeem URLs, WASM build, just recipes, or when the user mentions Authdog API
  location, AUTHDOG_* env vars, Management GraphQL backing the API worker,
  oauth callback, tenants list, or whoami.
disable-model-invocation: true
---

# Authdog CLI and platform surface

Assume the Rust workspace lives at **`authdog-cli/`** (`Cargo.toml`). A sibling **`platform-next/`** checkout may exist **one directory up** (`../platform-next`): API/Management implementations live there, not inside this crate.

## Hosted origins (production defaults)

| Role | Origin | Typical paths |
|------|--------|----------------|
| **REST API** | `https://api.authdog.com` | `/v1/userinfo`, `/v1/tenants`, `/v1/openapi`, favicon asset |
| **Identity** | `https://identity.authdog.com` | `/signin/{environmentId}?cli_sess=…&cli_redirect=…`, `POST /api/v1/cli/oauth/redeem`, `GET /api/v1/cli/oauth/poll` |
| JWKS reference (management token verify) | `https://id.authdog.com/.well-known/jwks.json` | (server-side Management; not CLI) |

Overrides: **`AUTHDOG_API_ORIGIN`**, **`AUTHDOG_IDENTITY_ORIGIN`**.

## Rust CLI defaults (source of truth)

- API: **`whoami::DEFAULT_API_ORIGIN`** = `https://api.authdog.com` (`AUTHDOG_API_ORIGIN`).
- Identity: **`DEFAULT_IDENTITY_ORIGIN`** (`AUTHDOG_IDENTITY_ORIGIN`), **`DEFAULT_CONSOLE_ENVIRONMENT_ID`** (`AUTHDOG_CONSOLE_ENVIRONMENT_ID`).
- OAuth loopback path: **`cli_login::LOOPBACK_OAUTH_REDIRECT_PATH`** (`/oauth/callback`).

## OAuth (browser ↔ CLI)

1. CLI listens **`127.0.0.1:0`** HTTP; redirect URL **`http://127.0.0.1:{port}/oauth/callback`**.
2. Browser returns **`GET /oauth/callback?grant=<64 hex>`**; CLI replies with **`assets/oauth_callback_success.html`** (logo from **`https://api.authdog.com/favicon.ico`** unless changed).
3. Tokens: **`POST {identity}/api/v1/cli/oauth/redeem`**; fallback **poll** at **`cli_poll_url`**.
4. Session file: **`~/.config/authdog-cli/credentials.json`** (crate **`session_store`**), mode `0600`.

## CLI commands

| Command | Behaviour |
|---------|-----------|
| `login` | Opens Identity sign-in in browser; receives loopback callback and exchanges tokens. |
| `logout` | Deletes saved credentials locally. |
| `status` | Reports whether user is logged in and path to credentials. |

Extend **reference.md** only when more tables or troubleshooting steps are needed.

## WASM

- Crate **`wasm/`** (**`authdog-cli-wasm`**) **`cdylib`** + **`wasm-bindgen`**; depends on **`authdog-cli`** with **`default-features = false`** (JWT/helpers only—no OAuth/TUI).

## justfile (repo root)

- **`just wasm`**: WASM release artefact **`target/wasm32-unknown-unknown/release/authdog_cli_wasm.wasm`**

## Quick edit map

| Area | Path |
|------|------|
| Userinfo REST + JWT prettify | `src/whoami.rs` |
| OAuth / redeem | `src/cli_login.rs` |
| CLI routing | `src/cli.rs` |
| Session store | `src/session_store.rs` |
| Wasm exports | `wasm/src/lib.rs` |

Do not confuse **CLI token preview** (`/status`) with full tokens; never paste production tokens into chats.
