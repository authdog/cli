# Authdog CLI — extended reference

## Environment variables

| Variable | Used by | Default (if unset) |
|----------|---------|----------------------|
| `AUTHDOG_IDENTITY_ORIGIN` | `AuthConfig.fromEnv`, redeem/signin URLs | `https://identity.authdog.com` |
| `AUTHDOG_CONSOLE_ENVIRONMENT_ID` | Sign-in URL `/signin/{id}` | Hard-coded console env UUID in `src/login.zig` |
| `AUTHDOG_CONFIG_DIR` | Credential directory override | OS config dir for `authdog-cli` |

## Useful probes (operator)

```bash
curl -sS -o /dev/null -w "%{http_code}\n" https://api.authdog.com/v1/userinfo    # expect 401 w/o Bearer
curl -sS -o /dev/null -w "%{http_code}\n" https://api.authdog.com/v1/tenants
tid=00000000-0000-4000-8000-000000000001
curl -sS -o /dev/null -w "%{http_code}\n" "https://api.authdog.com/v1/tenants/${tid}/projects"  # expect 401 w/o Bearer
curl -sS -o /dev/null -w "%{http_code}\n" https://api.authdog.com/favicon.ico
```

## Layout

| Piece | Role |
|-------|------|
| `src/main.zig` | `authdog` process entry |
| `src/cli.zig` | `login`, `logout`, `status`, text/JSON output |
| `src/login.zig` | Loopback OAuth and redeem |
| `src/session.zig` | `credentials.json` |
| `release.toml` | Version and `stable` flag for git tags |

## platform-next pointers (adjacent checkout)

| Concern | Path (under `platform-next/`) |
|---------|--------------------------------|
| REST tenant routes/handlers | `services/api/src/routes/tenants/` |
| OpenAPI mounting | `services/api/src/app.ts` |
| Management GraphQL: `userOrganizations`, `tenantsWithAccess` | `services/management/src/` (resolvers under `resolvers/queries/`) |

## Skill usage

Prefer **`SKILL.md`** for default context length. Load **this file** when debugging cross-service tenant 403/Management GraphQL failures or documenting operator curl checks.
