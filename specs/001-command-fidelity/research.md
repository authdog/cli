# Research: Command fidelity

## 1. MCP install overwrites the client config

**Decision**: Wave 1 parses the existing Cursor config, sets only
`mcpServers.authdog`, and writes the merged document. Uninstall deletes
that one key and rewrites the rest. Invalid JSON fails usage and does
not touch the file.

**Rationale**: `upsertCursorMcpConfig` in `src/commands.zig` replaces
the whole file with a two-server template. `removeCursorMcpConfig` is
an empty function, and help text says uninstall is reserved. A merge
fixes the data-loss bug without supporting more clients.

**Alternatives considered**: Multi-client detection (Cursor, Claude,
VS Code, Codex) as specified in `SPEC.md` §7.9. Deferred. Cursor is the
client this binary already targets, and the other paths are a later
feature.

## 2. CLI sessions cannot be refreshed

**Decision**: Wave 2 maps HTTP 401 from the management API to exit code
`3` and the message `session expired; run authdog login`. The stored
`refresh_token` stays on disk and is still never printed. No refresh
HTTP call is added.

**Rationale**: Identity CLI OAuth in
`platform-next/apps/identity/src/handlers/cli/oauthCliBridge.ts`
exposes `POST /api/v1/cli/oauth/start`, `GET /api/v1/cli/oauth/poll`,
and `POST /api/v1/cli/oauth/redeem`. Redeem returns `access_token` and
`refresh_token` and then deletes the bridge row. There is no refresh
route. `src/auth_session.zig` loads only the access token.
`SPEC.md` §14 item 1 recorded this as open; it is now resolved.

**Alternatives considered**: Call the OIDC token endpoint with
`grant_type=refresh_token`. Rejected. That endpoint belongs to
registered OIDC clients, not the CLI management bridge, and inventing
the call would violate the constitution.

## 3. Config diff is a byte compare

**Decision**: Wave 3 parses local and live documents and compares the
sections `sessionConfig`, `redirectUris`, `webhooks`, and
`oidcClients`. Objects compare by value, not key order or whitespace.
Output names each section as `match` or `differ`.

**Rationale**: `runConfigDiff` uses `std.mem.eql` on the raw strings,
and help text calls the command a stub. Pull writes pretty-printed
concatenation of upstream bodies, so a harmless re-pull can look like
a diff.

**Alternatives considered**: Unified line diff of the raw files.
Rejected. Operators need section names, and formatting noise would
keep the command a stub.

## 4. Pull omits clients and can persist secrets

**Decision**: Wave 4 adds the existing OIDC client list
(`sdk_config.fetchOidcClientsPath`) as `oidcClients`, recursively
redacts keys named `signingSecret`, `secret`, `clientSecret`, and
`token` before writing `.authdog/config.json`, and makes
`config patch --dry-run` print the Wave 3 section diff without a PUT.
Apply keeps session and redirect writes only and refuses a document
that changes `oidcClients` or any redacted key.

**Rationale**: `formatConfigDocument` writes session, redirects, and
webhooks only. `SPEC.md` §7.4 also requires a read-only OIDC snapshot
and redacted webhooks. Dry-run today lists action names and does not
show the diff. Patch scopes (open question 3) stay limited to the two
PUT routes already called.

**Alternatives considered**: Full env-settings parity. Rejected for
this feature. `SPEC.md` already says patch grows incrementally.

## 5. Init stops before integration files

**Decision**: Wave 5 vendors a small snippet catalog under
`assets/snippets/` for Next.js provider wrap and Express client setup,
copied from `platform-next/packages/sdk-snippets` (manifest ids
`nextjs-provider` and `express-configure`). Init writes a destination
only when that path is absent. `--pm bun|pnpm|yarn|npm` overrides
lockfile detection. `--dry-run` lists destinations and the package
manager and writes nothing.

**Rationale**: `runInit` merges env lines and may spawn a package
manager. It never copies snippets. The snippet package points at
marketing markdown fences, so the CLI needs a generated snapshot it
can embed, not a runtime dependency on platform-next. Principle I
forbids a JS runtime; embed-at-build is the same pattern as
`assets/oauth_callback_success.html`.

**Alternatives considered**: Shell out to a snippet renderer at init
time. Rejected. The installed binary must work without the monorepo.

## 6. Framework detection is coarse

**Decision**: Wave 6 treats a `react` or `@authdog/react` dependency,
when `next` is absent, as React and installs `@authdog/react`. FastAPI
requires `fastapi` or the `authdog` Python package in `pyproject.toml`.
Any other Python project stays `unknown` unless `--framework fastapi`
is passed. FastAPI install uses `uv add` when `uv.lock` exists, else
`pip install`.

**Rationale**: `detectFramework` returns `.fastapi` for every
`pyproject.toml`, and `npmPackage` returns null for FastAPI, so init
installs nothing. React shares the JavaScript package
`@authdog/javascript`.

**Alternatives considered**: Prompt for framework when detection is
ambiguous. Rejected in this wave. `--non-interactive` already fails
closed, and interactive prompts are a separate UX change.

## 7. Listen cannot replay a signed body

**Decision**: Wave 7 forwards `body` or `payload` when the delivery
object contains one. Otherwise it POSTs
`{"deliveryId","eventType","payloadAvailable":false}` and counts those
forwards separately. It does not attach `X-Authdog-Signature`.

**Rationale**: `listWebhookDeliveries` in
`platform-next/services/api/src/routes/webhooks/handlers.ts` returns
id, channel, event type, status, and attempt metadata. It does not
return the stored body. The 025 checkpoint already notes that
`--forward` posts a metadata summary. Pretending that summary is the
signed event breaks local verifiers. Phase B (rewrite a channel URL
to a tunnel) stays out of scope.

**Alternatives considered**: Call redeliver and hope the platform
targets `--forward`. Rejected. Redeliver re-signs to the saved
endpoint URL, not an arbitrary local URL.

## 8. Impersonate prints the raw response

**Decision**: Wave 8 parses `token` and optional `redirectTo`, prints
`{redirectTo}?token={token}` when `redirectTo` is present, and
otherwise prints paste instructions. JSON output includes `grantId`
and `signInUrl` and omits the token. `impersonate revoke --grant-id`
calls the revoke route from `SPEC.md` §7.7. `--non-interactive` grant
creation requires `--reason`.

**Rationale**: `runImpersonate` puts `imp_resp.body` in the structured
payload. That body is the Identity impersonation response and can
carry the token. Revoke is specified and not routed.

**Alternatives considered**: Keep printing the token in JSON for
agents. Rejected. Principle II forbids token material on stdout.
Agents get a URL they can open, which is the operator outcome in
`SPEC.md` §7.7.

## 9. Deploy status dumps raw documents

**Decision**: Wave 9 issues the posture, vanity, redirect, and webhook
GETs already used elsewhere and emits four checks `{name, ok, detail}`.
A completed report exits 0 even when a row fails. A 401 still exits 3
via Wave 2. Text mode prints `ok` / `FAIL` rows the way `doctor` does.

**Rationale**: `runDeployStatus` embeds raw posture and vanity bodies
and skips redirects and webhooks, which `SPEC.md` §7.8 lists as status
checks. Interactive DNS wizard and `dig` stay out of this wave.

**Alternatives considered**: Non-zero exit when any check fails.
Rejected. The spec says agents must be able to read every row from a
completed report. Auth and usage failures keep their exit codes.

## 10. Contract lock

**Decision**: Wave 10 adds Zig tests beside the touched commands and
checks them against `contracts/command-output.md`. Help lines for
`config diff` and `mcp uninstall` are updated in that wave so they
match behavior. No new CI job is required beyond `zig build test`.

**Rationale**: Parser, signature, and file-mutation tests are
constitution V. An OpenAPI pin job already lives in platform-next for
025. This repo does not have the OpenAPI export. Duplicating that CI
here would couple release to a sibling checkout.

**Alternatives considered**: A CLI CI diff against `/v1/openapi`.
Deferred to platform-next, where `services/api/test/openapi.test.ts`
already pins paths.
