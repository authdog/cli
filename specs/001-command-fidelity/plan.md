# Implementation Plan: Command fidelity

**Branch**: `001-command-fidelity` | **Date**: 2026-10-17 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-command-fidelity/spec.md`

## Summary

025 waves 1–5 shipped the command tree. Ten further waves make those
commands match `SPEC.md` where they still clobber, mislead, or stop
early: merge MCP config, honest session expiry, section config diff,
redacted pull and safe patch, init snippet files, React and FastAPI
install, honest webhook forward, safe impersonation plus revoke,
deploy checklist, and offline contract tests.

## Technical Context

**Language/Version**: Zig 0.16.0 (repo requirement)

**Primary Dependencies**: Zig std HTTP client (`src/api/client.zig`);
Identity CLI OAuth; `api.authdog.com` `/v1`; snippet catalog in
`platform-next/packages/sdk-snippets` (vendored snapshot only)

**Storage**: `~/.config/authdog-cli/credentials.json` (mode 0600);
`.authdog/project.json`; `.authdog/config.json`; Cursor
`~/.cursor/mcp.json`

**Testing**: `zig build test` (unit tests in `src/*.zig` plus
`src/cli_checks.zig`)

**Target Platform**: The `authdog` binary shipped from `cli.auth.dog`
(Linux, macOS, Windows)

**Project Type**: CLI

**Performance Goals**: Config compare and MCP merge stay in-process on
files under 1 MiB, the existing read limit. No new network retries.

**Constraints**: No embedded JS runtime. No new unpublished HTTP
paths. No tokens in text or JSON. `--json` and exit codes 0/2/3/4
stay stable.

**Scale/Scope**: Ten waves, one command family each, all in this repo.
Cursor is the only MCP client. Starters, webhook Phase B, and
`authdog api` stay out.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Result |
| --- | --- | --- |
| I. Single Zig binary | Snippets are embedded assets, not a JS runtime. Package install still shells out to a manager on `PATH`. | Pass |
| II. Secrets off output | Wave 2, 4, and 8 remove token and secret fields from stdout and from `.authdog/config.json`. | Pass |
| III. Agent-stable interface | New JSON fields are additive. Exit 3 is used for 401. Dry-run and `--non-interactive` keep their meanings. | Pass |
| IV. Specs name a purpose | Folder and branch are `001-command-fidelity`. `SPEC.md` stays the behavior source. | Pass |
| V. Tests for parsers and file writes | Wave 10 tests MCP merge, redaction, diff, detection, verify-adjacent listen envelope, and impersonate shaping. | Pass |

Post-design re-check: research decisions 2 and 7 refuse invented HTTP
routes. No Complexity Tracking row is required.

## Project Structure

### Documentation (this feature)

```text
specs/001-command-fidelity/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   └── command-output.md
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks — not part of this plan
```

### Source Code (repository root)

```text
src/
├── cli.zig              # help text, --pm, impersonate revoke routing
├── commands.zig         # MCP, config, init, listen, impersonate, deploy
├── detect.zig           # React vs JavaScript, FastAPI vs unknown
├── auth_session.zig     # session load (unchanged token storage)
├── api/client.zig       # 401 → exit 3
└── sdk_config.zig       # existing OIDC list path reused by pull
assets/snippets/         # vendored nextjs + express files (Wave 5)
```

**Structure Decision**: Keep one module per area already in `src/`.
Do not add `src/commands/*.zig` in this feature. Snippet bytes live
under `assets/snippets/` and are embedded the same way as the OAuth
callback page in `build.zig`.

## Waves

Each wave is one user story from the spec and can merge alone.
Later waves may assume earlier user-visible behavior.

| Wave | Story | Outcome | Primary code |
| --- | --- | --- | --- |
| 1 | US1 | MCP install merges; uninstall removes only `authdog` | `upsertCursorMcpConfig`, `removeCursorMcpConfig` |
| 2 | US2 | HTTP 401 exits 3 with `session expired; run authdog login` | `src/api/client.zig` `reportApiFailure` |
| 3 | US3 | `config diff` compares normalized sections | `runConfigDiff` |
| 4 | US4 | Pull adds redacted webhooks + read-only `oidcClients`; patch dry-run uses Wave 3; apply refuses clients and secrets | `fetchEnvironmentConfigBundle`, `runConfigPatch` |
| 5 | US5 | Init writes missing Next.js and Express snippet files; `--pm` selects the installer | `runInit`, `assets/snippets/`, `src/cli.zig` |
| 6 | US6 | React installs `@authdog/react`; FastAPI installs `authdog` via uv or pip; other Python stays unknown | `src/detect.zig`, `runPackageInstall` |
| 7 | US7 | Listen forwards a body when present, otherwise `payloadAvailable: false` | `runWebhooksListen` |
| 8 | US8 | Impersonate prints a sign-in URL; `impersonate revoke --grant-id`; non-interactive create requires `--reason` | `runImpersonate`, `src/cli.zig` |
| 9 | US9 | `deploy status` prints four `ok`/`FAIL` checks | `runDeployStatus` |
| 10 | US10 | Offline tests plus help text match `contracts/command-output.md` | `src/*` tests, `help_text` |

### Wave dependencies

- Wave 4 dry-run output uses the section compare from Wave 3.
- Wave 6 install dispatch uses the `--pm` override from Wave 5 when
  the operator passed it; lockfile detection still works if Wave 5
  has not shipped, so Wave 6 can land first if needed.
- Wave 9 uses the 401 mapping from Wave 2 when a readiness call is
  unauthorized. Without Wave 2 it still prints the four rows for
  HTTP 200 fixtures.
- Wave 10 is last. It asserts the contracts for whichever earlier
  waves have merged, and it updates help text that those waves change.

### Out of scope (do not pull into a wave)

- MCP clients other than Cursor.
- Identity refresh route, starter templates, `init` creating an
  environment, webhook Phase B tunnel rewrite, `authdog api`.
- OpenAPI CI in this repo (stays in platform-next).
- Non-zero exit when a deploy check fails.

## Complexity Tracking

No constitution violations.
