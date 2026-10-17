# Tasks: Command fidelity

**Input**: Design documents from `/specs/001-command-fidelity/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/command-output.md, quickstart.md

**Tests**: Included. The spec (FR-014, US10) requires offline checks for the command contract.

**Organization**: Phases follow user-story priority. US4 depends on US3. US10 is last.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [x] T001 Confirm `.gitignore` already ignores `/.zig-cache/` and `/zig-out/`

## Phase 2: Foundational

- [x] T002 Map HTTP 401 to exit 3 and `error: "session_expired"` in `src/api/client.zig` and `src/cli.zig` (text: `session expired; run authdog login`)

## Phase 3: User Story 1 — MCP merge (P1)

**Independent Test**: A config with server `other` still has `other` after install and after uninstall.

- [x] T003 [US1] Merge and remove only `mcpServers.authdog` in `src/commands.zig` (preserve every other key; invalid JSON is exit 2 and does not write)
- [x] T004 [US1] Cover merge, uninstall, and invalid JSON in `src/commands.zig` tests

## Phase 4: User Story 2 — Expired session (P1)

**Independent Test**: Status 401 yields `session_expired` and does not echo the response body.

- [x] T005 [US2] Add a `src/api/client.zig` test that 401 selects `session_expired` and the code does not include a sample token

## Phase 5: User Story 3 — Config diff (P1)

**Independent Test**: One changed section is `differ`; formatting-only input is `match`.

- [x] T006 [US3] Compare `sessionConfig`, `redirectUris`, `webhooks`, and `oidcClients` by JSON value in `src/commands.zig`
- [x] T007 [US3] Test section diff in `src/commands.zig`

## Phase 6: User Story 5 — Init files (P1)

**Independent Test**: `--dry-run --pm bun` names `bun` and a create action; an existing layout file is unchanged.

- [x] T008 [US5] Vendor Next.js and Express snippets in `assets/snippets/` and write them only when the destination is absent from `src/commands.zig`
- [x] T009 [US5] Parse `--pm` in `src/cli.zig` and honor `bun`, `pnpm`, `yarn`, `npm`, `uv`, and `pip` in `src/commands.zig`

## Phase 7: User Story 4 — Pull and patch (P2)

**Independent Test**: `signingSecret` is absent after pull shaping; dry-run performs no PUT; client or secret changes return `patch_refused`.

- [x] T010 [US4] Add redacted `webhooks` and read-only `oidcClients` to the settings document in `src/commands.zig`
- [x] T011 [US4] Make `config patch` refuse secret keys and a differing `oidcClients` section, and show the Wave 3 diff on `--dry-run`, in `src/commands.zig`

## Phase 8: User Story 6 — Framework install (P2)

**Independent Test**: React, FastAPI, and other Python projects classify differently.

- [x] T012 [US6] Detect React as `@authdog/react` and FastAPI only when `fastapi` or `authdog` is present in `src/detect.zig`
- [x] T013 [US6] Install the Python package with `uv` when `uv.lock` exists, otherwise `pip`, in `src/commands.zig`

## Phase 9: User Story 7 — Webhook listen (P2)

**Independent Test**: A delivery without a body forwards `payloadAvailable: false`; a delivery with `body` forwards that body.

- [x] T014 [US7] Shape listen forwards in `src/commands.zig` and count `forwardedWithBody`

## Phase 10: User Story 8 — Impersonate (P2)

**Independent Test**: A fixture with `redirectTo` yields `signInUrl` and no JSON key `token`. Revoke accepts `--grant-id`. Non-interactive grant creation requires `--reason`.

- [x] T015 [US8] Shape impersonation output and add `impersonate revoke` in `src/commands.zig` and `src/cli.zig`

## Phase 11: User Story 9 — Deploy checklist (P2)

**Independent Test**: Four named checks, exit success when a row fails.

- [x] T016 [US9] Emit `security-posture`, `vanity-domains`, `redirects`, and `webhooks` from `src/commands.zig`

## Phase 12: User Story 10 — Contract lock (P3)

**Independent Test**: `zig build test` passes and help text no longer calls diff a stub or uninstall reserved.

- [x] T017 [US10] Update help text in `src/cli.zig` and assert the contract fields from `specs/001-command-fidelity/contracts/command-output.md`
- [x] T018 Run `zig build test` and `zig fmt`

## Dependencies

- US4 (T010–T011) requires US3 section compare (T006).
- US10 (T017) requires the help strings changed by earlier stories.
- US2 (T002) is required for 401 behavior on deploy and patch.

## Parallel execution

- T012 (`src/detect.zig`) can proceed beside T003 (`src/commands.zig`) only before both edit `src/commands.zig`. After T003, keep `src/commands.zig` sequential.

## Implementation strategy

MVP is User Story 1 (MCP merge). Each later story is an increment on the same binary. US10 locks the contract after the command behavior exists.
