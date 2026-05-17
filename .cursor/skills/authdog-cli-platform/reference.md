# Authdog CLI — extended reference

## Environment variables

| Variable | Used by | Default (if unset) |
|----------|---------|----------------------|
| `AUTHDOG_API_ORIGIN` | `whoami::api_origin()`, tenants / organizations / projects fetch | `https://api.authdog.com` |
| `AUTHDOG_IDENTITY_ORIGIN` | `CliAuthConfig::from_env()`, redeem/poll/signin URLs | `https://identity.authdog.com` |
| `AUTHDOG_CONSOLE_ENVIRONMENT_ID` | Sign-in URL `/signin/{id}` | Hard-coded console env UUID in `cli_login.rs` |

## Useful probes (operator)

```bash
curl -sS -o /dev/null -w "%{http_code}\n" https://api.authdog.com/v1/userinfo    # expect 401 w/o Bearer
curl -sS -o /dev/null -w "%{http_code}\n" https://api.authdog.com/v1/tenants
tid=00000000-0000-4000-8000-000000000001
curl -sS -o /dev/null -w "%{http_code}\n" "https://api.authdog.com/v1/tenants/${tid}/projects"  # expect 401 w/o Bearer
curl -sS -o /dev/null -w "%{http_code}\n" https://api.authdog.com/favicon.ico
```

## Workspace packages

| Package | Role |
|---------|------|
| `authdog-cli` (root `Cargo.toml`) | **Library** (`src/lib.rs`) + **binary** `authdog-cli` with `required-features = ["desktop"]` |
| `authdog-cli-wasm` (`wasm/`) | `cdylib` for browser/embed via `wasm-bindgen` |

Desktop feature aggregates TUI deps (Ratatui, Crossterm, blocking `reqwest`, `open`, OAuth TCP, dirs, …).

## platform-next pointers (adjacent checkout)

| Concern | Path (under `platform-next/`) |
|---------|--------------------------------|
| REST tenant routes/handlers | `services/api/src/routes/tenants/` |
| OpenAPI mounting | `services/api/src/app.ts` |
| Management GraphQL: `userOrganizations`, `tenantsWithAccess` | `services/management/src/` (resolvers under `resolvers/queries/`) |

## Skill usage

Prefer **`SKILL.md`** for default context length. Load **this file** when debugging cross-service tenant 403/Management GraphQL failures or documenting operator curl checks.
