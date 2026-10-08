# Authdog CLI — product specification (refactor target)

Canonical API contracts: **`GET https://api.authdog.com/v1/openapi`** (implemented in `platform-next/services/api`). Identity CLI OAuth: **`platform-next/apps/identity`** (`/api/v1/cli/oauth/*`, `/api/v1/identity/:environmentId/impersonate`).

This document defines the terminal-first CLI we are building on top of the existing Zig binary (`authdog`). It is the source of truth for command behavior, API mapping, and delivery phases.

---

## 1. Goals

1. **From repo to working auth in one flow** — detect the app stack, link a tenant/project/environment, install the right SDK, write env vars, and apply minimal integration files without manual copy-paste from the dashboard.
2. **Operate Authdog from the shell** — same bearer session as today’s `login`, calling `/v1/*` for management tasks agents and CI can automate.
3. **Local developer ergonomics** — webhook testing and signature verification without requiring a public tunnel when possible; user impersonation for support/debug with full audit trail.
4. **Production readiness** — guided checks for domains, OAuth connections, and security posture before go-live.
5. **Agent-native** — stable `--json`, non-interactive flags, exit codes, and optional MCP registration so coding agents can drive setup safely.

## 2. Non-goals (initial phases)

- Replacing the hosted console for all configuration.
- Embedding a JavaScript runtime in the Zig binary (package install and file patches may shell out to `npm`/`bun`/`pnpm`/`yarn` when present).
- Implementing every `/v1` route as a subcommand (expose high-value workflows first; add `authdog api …` later if needed).
- Hosting a proprietary webhook relay SaaS (prefer replay/verify locally; optional tunnel documented as bring-your-own).

## 3. Current baseline (keep and extend)

| Today | Keep |
|-------|------|
| `login` / `logout` / `status` | Yes; optionally alias under `authdog auth login` |
| Loopback OAuth via Identity | Yes |
| `credentials.json` (0600) | Yes; add optional **API profile** fields (see §4) |
| `-o json` / `--json` | Yes; extend to all new commands |
| `AUTHDOG_IDENTITY_ORIGIN`, `AUTHDOG_CONFIG_DIR`, `AUTHDOG_CONSOLE_ENVIRONMENT_ID` | Yes |

New global flags (all commands):

| Flag | Purpose |
|------|---------|
| `--api-origin` | Default `https://api.authdog.com`; env `AUTHDOG_API_ORIGIN` |
| `--project-file` | Path to link manifest (default `.authdog/project.json`) |
| `--non-interactive` | Fail instead of prompting (required for CI/agents) |
| `--yes` | Accept defaults in wizards |

Exit codes: `0` success, `2` usage/validation, `3` auth required, `4` API error (print stable `error` JSON field when `-o json`).

## 4. Project link manifest

Written by `init` / `link`; read by all project-scoped commands.

**Path:** `.authdog/project.json` (git-trackable; no secrets).

```json
{
  "schemaVersion": 1,
  "tenantId": "uuid",
  "applicationId": "uuid",
  "environmentId": "uuid",
  "apiOrigin": "https://api.authdog.com",
  "identityOrigin": "https://identity.authdog.com",
  "linkedAt": "2026-10-09T00:00:00Z"
}
```

**Credentials** (`credentials.json`) remain separate and hold only OAuth tokens. Optional extension:

```json
{
  "access_token": "…",
  "refresh_token": "…",
  "api_origin": "https://api.authdog.com"
}
```

## 5. Internal architecture (Zig)

| Module | Responsibility |
|--------|----------------|
| `src/cli.zig` | Top-level routing, global flags, help |
| `src/login.zig`, `src/session.zig` | Unchanged responsibilities |
| `src/api/client.zig` | HTTPS client, bearer injection, token refresh hook, JSON decode helpers |
| `src/api/openapi.zig` | Optional: pinned path constants generated from `/v1/openapi` in CI |
| `src/project.zig` | Load/save `.authdog/project.json`, validate UUIDs |
| `src/detect.zig` | Framework + package manager detection from `package.json`, `pyproject.toml`, etc. |
| `src/commands/*.zig` | One file per command group |
| `src/ui.zig` | Progress steps (◇/✓), spinners, stderr vs stdout discipline |

HTTP: use Zig std HTTP client; respect `AUTHDOG_API_ORIGIN`; never log tokens.

## 6. Command tree (target)

```
authdog
├── auth
│   ├── login
│   ├── logout
│   └── status
├── init
├── link
├── unlink
├── whoami
├── init-config | config
│   ├── pull
│   ├── patch
│   └── diff
├── webhooks
│   ├── listen
│   └── verify
├── impersonate (alias: imp)
│   ├── create-grant   (optional helper)
│   └── revoke
├── deploy
│   └── status
├── mcp
│   ├── install
│   ├── list
│   └── uninstall
├── doctor
└── (future) api …
```

Backward compatibility: bare `authdog login|logout|status` remain indefinitely.

---

## 7. Command specifications

### 7.1 `authdog init`

**Purpose:** Bootstrap auth in an existing repository (or `--starter` template in a later phase).

**Steps (interactive default):**

1. **Detect framework** — inspect lockfiles and dependencies:
   - `next` → `nextjs`
   - `@authdog/react`, `@authdog/javascript` → `javascript` / `react`
   - `@authdog/express` → `express`
   - FastAPI / `authdog` Python package → `fastapi`
   - Unknown → prompt or `--framework`
2. **Authenticate** — run login flow if no valid session.
3. **Link project** — if no manifest:
   - `GET /v1/tenants`
   - `GET /v1/tenants/{tenantId}/projects`
   - User picks tenant + project + environment (or flags `--tenant-id`, `--application-id`, `--environment-id`).
4. **Resolve SDK wiring** — obtain publishable key, OIDC `clientId`, endpoints, env-var names (see §8.1).
5. **Install SDK** — exec detected PM: e.g. `npm install @authdog/nextjs` (matrix in §9).
6. **Write env file** — merge into `.env.local` / `.env` (detect existing; never overwrite unrelated keys):
   - `PK_AUTHDOG` / `NEXT_PUBLIC_PK_AUTHDOG` / `VITE_AUTHDOG_PK` (framework-specific)
   - `AUTHDOG_CLIENT_ID` (OIDC hex client id, **not** the publishable key)
7. **Apply integration snippets** — copy from embedded catalog mirroring `platform-next/packages/sdk-snippets` (provider, middleware, callback route).
8. **Write** `.authdog/project.json`.
9. Print next steps (run dev server, claim production, link docs).

**Flags:** `--framework`, `--pm`, `--env-file`, `--dry-run`, `--non-interactive`, `--skip-install`.

**API:** tenants list, projects list, project details, environment list, SDK config aggregate (§8.1).

---

### 7.2 `authdog link` / `unlink`

**link:** Bind cwd to an existing tenant/project/environment without installing packages.

**unlink:** Remove `.authdog/project.json` only (does not logout).

---

### 7.3 `authdog whoami`

**Purpose:** Human/agent sanity check of the management session.

**API:** `GET /v1/userinfo` with bearer token.

**Output (text):** display name, email, environment id from payload; **never** print raw JWT.

**Output (json):** subset of userinfo response + `credentialsPath`.

---

### 7.4 `authdog config pull|patch|diff`

**Purpose:** Manage a **narrow, version-controlled** slice of environment settings as JSON on disk.

**Default file:** `.authdog/config.json`

**pull:** Download and normalize:

| Config key | API source (initial scope) |
|------------|----------------------------|
| `redirectUris` | `GET …/environments/{environmentId}/redirect-uris` (see environments routes) |
| `session` | `GET …/session-config` (env-settings) |
| `oidcClients` | `GET …/oidc-clients` (read-only snapshot) |
| `webhooks` | `GET …/webhooks` (secrets redacted) |

**patch:** Apply JSON merge patch; call corresponding PUT/POST routes. Support `--dry-run` (validate + show diff only).

**diff:** Compare disk vs live pull without writing.

**Out of scope for v1 patch:** full parity with every env-settings route; expand incrementally.

---

### 7.5 `authdog webhooks listen`

**Purpose:** Forward webhook deliveries to a local HTTP handler for development.

**Invocation:**

```text
authdog webhooks listen --forward http://127.0.0.1:3000/webhooks/authdog [--environment-id …]
```

**Behavior (phased):**

- **Phase A (no new infra):** Poll `GET …/webhooks/deliveries?limit=…` on an interval; for each new delivery id, fetch payload from delivery record (or Management-backed fields returned by API), POST to `--forward`, print summary. Optionally call `POST …/deliveries/{deliveryId}/redeliver` when the platform supports targeting a alternate URL (if not, replay body client-side).
- **Phase B (optional):** CLI temporarily updates one webhook endpoint URL to a user-supplied tunnel URL, with restore on exit (requires `--channel-id` and explicit consent).

**Requirements:** Linked project; bearer auth; respects signing secret from endpoint creation/rotation.

---

### 7.6 `authdog webhooks verify`

**Purpose:** Offline HMAC verification of a captured delivery (stdin or file).

**Input:** Raw body + headers (or `--secret` + `--timestamp` + `--signature` flags matching Authdog’s webhook signing scheme used by notification channels).

**Output:** `valid: true|false` (+ json).

**Note:** Implement verifier to match Management’s webhook signing format (document scheme in this repo once confirmed from `platform-next` notification dispatch). Does **not** require login when secret is passed explicitly.

---

### 7.7 `authdog impersonate` (`imp`)

**Purpose:** Mint a short-lived impersonation session for debugging, tied to the operator’s account and grant policy.

**Flow:**

1. Require linked `environmentId` + `tenantId` (manifest or flags).
2. Resolve target user: `--user-id` or `authdog users search` (future); initial phase: required `--user-id`.
3. Ensure grant exists:
   - If none: `POST /v1/tenants/{tenantId}/environments/{environmentId}/impersonation-grants` with `actorUserId` from userinfo, `targetUserId`, `durationMinutes`, optional `reason`.
   - Or require pre-approved grant (stricter orgs): `--grant-id` only.
4. Call Identity (not REST API worker):
   - `POST {identity}/api/v1/identity/{environmentId}/impersonate`
   - Body: `{ "userId", "tenantId", "callbackUrl"? }`
   - Header: `Authorization: Bearer <cli access token>`
5. Response: `{ "token", "redirectTo"? }` — print a **sign-in URL**:
   - If `redirectTo` present: `{redirectTo}?token={token}` (only when platform allows; respect `sanitizePlatformRedirectUri` rules).
   - Else: print token + instructions to paste into app callback handler (`?token=` pattern used by JS SDK).
6. Audit: rely on platform logging (`user_impersonation_started`).

**Subcommand `impersonate revoke`:** `POST …/impersonation-grants/{grantId}/revoke`.

**Safety:** Refuse `--non-interactive` without `--reason` when tenant policy requires it (future); always log grant id in json output.

---

### 7.8 `authdog deploy` / `deploy status`

**Purpose:** Guided production readiness for the linked environment.

**status checks (read-only API calls):**

| Check | API |
|-------|-----|
| Security posture score & findings | `GET …/security/posture` |
| Vanity domains + DNS state | `GET …/vanity-domains`, `POST …/vanity-domains/{id}/check` |
| OIDC / redirect configuration | environments + oidc-clients routes |
| Webhook endpoints configured | `GET …/webhooks` |

**deploy (interactive wizard):** Sequential prompts with doc links — register vanity hostname, show CNAME target from API, list connected OAuth grants (`connected-apps` routes), remind to rotate secrets off disk.

**Does not:** auto-DNS; may shell out to `dig`/`nslookup` for hints only.

---

### 7.9 `authdog mcp install|list|uninstall`

**Purpose:** Register the **Authdog MCP server** (hosted on the API worker) in local AI clients.

**Server URL:** `https://api.authdog.com/mcp` (streamable HTTP; see `platform-next/services/api/src/mcp/index.ts`).

**install:**

- Detect installed clients: Cursor, Claude Code, VS Code, Codex, etc. (same strategy as common MCP CLIs: config file paths per OS).
- Write user-global config (not per-repo): OAuth to Authdog for MCP scopes.
- Flags: `--all`, `--client cursor`, `--non-interactive`.

**list / uninstall:** Manage entries tagged `authdog` only.

**doctor:** Ping `/.well-known/oauth-protected-resource/mcp` and report reachability.

---

### 7.10 `authdog doctor`

**Purpose:** Pre-flight diagnostics.

| Check | Method |
|-------|--------|
| CLI version | local |
| Logged in | `credentials.json` |
| Identity reachable | `GET {identity}/…` or redeem endpoint HEAD |
| API reachable | `GET /v1/health` |
| Token valid | `GET /v1/userinfo` |
| Linked project | manifest parse + optional `GET …/applications/{id}` |
| MCP endpoint | optional `--mcp` |

Output table (text) or structured json array `{ name, ok, detail }`.

---

## 8. API mapping summary

Base: `{apiOrigin}/v1` with `Authorization: Bearer {access_token}`.

| CLI need | REST (existing in `services/api`) |
|----------|-----------------------------------|
| Session identity | `GET /userinfo` |
| Tenants | `GET /tenants`, create/update when needed |
| Projects | `GET /tenants/{id}/projects`, `GET /tenants/{tenantId}/applications/{applicationId}` |
| Environments | `…/applications/{applicationId}/environments` |
| OIDC clients / redirects | `oidc-clients`, `environments` redirect URI routes |
| Webhooks | `…/webhooks`, deliveries, rotate-secret, redeliver |
| Impersonation grants | `…/impersonation-grants` |
| Impersonation token | Identity `POST /api/v1/identity/{environmentId}/impersonate` |
| Security / go-live | `…/security/posture`, `…/vanity-domains` |
| OpenAPI | `GET /openapi` |

### 8.1 Platform gap — SDK config REST endpoint

MCP already exposes **`getSdkConfig`** (`platform-next/services/api/src/mcp/tools/oidc/getSdkConfig.ts`) combining public key, recommended OIDC client, endpoints, and env-var hints.

**Requirement:** Add REST parity, e.g.

`GET /v1/tenants/{tenantId}/environments/{environmentId}/sdk-config`

…returning the same shape as MCP (no secrets). CLI **must not** depend on MCP for `init` (simpler auth, no MCP scope coupling).

**Alternative (interim):** CLI composes via multiple REST calls + `GET /v1/graphql` public GraphQL — only if documented; prefer dedicated REST.

---

## 9. Framework / SDK matrix (init)

Aligned with `@authdog/sdk-snippets` catalog (`express`, `fastapi`, `javascript`, `nextjs`):

| Framework | Package | Env vars |
|-----------|---------|----------|
| nextjs | `@authdog/nextjs` | `NEXT_PUBLIC_PK_AUTHDOG`, `NEXT_PUBLIC_AUTHDOG_CLIENT_ID` |
| javascript / react | `@authdog/javascript` or `@authdog/react` | `PK_AUTHDOG`, `AUTHDOG_CLIENT_ID` |
| express | `@authdog/express` | `PK_AUTHDOG` |
| fastapi | `authdog` (Python) | `PK_AUTHDOG` |

Snippets ship embedded in CLI release artifact (generated in CI from `sdk-snippets` to avoid drift).

---

## 10. Agent / CI mode

- All commands accept `--json` and `--non-interactive`.
- `init --non-interactive` requires tenant/project/environment ids + `--framework` + `--pm`.
- Stable json schema version field on structured output: `{ "schemaVersion": 1, … }`.
- Document prompt recipes in `.cursor/skills/authdog-cli-platform/` (operator flows: init, webhooks, impersonate, deploy).

---

## 11. Phased delivery

| Phase | Scope | Outcome |
|-------|--------|---------|
| **P0** | `api` client, refresh token, `whoami`, `link`, `doctor`, manifest | CLI talks to production API reliably |
| **P1** | `init` (nextjs + express first), SDK REST endpoint in API repo | One-command app bootstrap |
| **P2** | `config pull`, `webhooks verify`, `webhooks listen` phase A | Day-2 dev workflows |
| **P3** | `impersonate`, `deploy status` | Support & go-live |
| **P4** | `mcp install`, `config patch`, starter templates | Agent-first UX parity |

---

## 12. Testing

- Unit: parse tests per command (existing `cli.zig` pattern).
- HTTP: mock stub server for API client (Zig tests).
- Integration (optional CI job): `authdog doctor` against staging with secret credentials; no token echo.
- Contract: CI diff CLI pinned paths vs `/v1/openapi` export from `services/api`.

---

## 13. Documentation & install

- User-facing: `README.md` command reference links to this spec.
- Install worker (`install/SPEC.md`) unchanged; optional docs link on `cli.auth.dog`.
- Versioning: continue `release.toml` semver; breaking CLI json schema bumps minor with changelog.

---

## 14. Open questions

1. Token refresh: does Identity expose refresh for CLI grants today? If not, add refresh to redeem response handling before long-running commands.
2. Webhook signature: exact header names and HMAC algorithm for `webhooks verify`.
3. `config patch` authorization: which Management scopes are required per sub-resource?
4. Should `init` create a dev environment automatically (`POST …/environments`) or only link existing?

Resolve in API/Identity specs before P1/P2 implementation.
