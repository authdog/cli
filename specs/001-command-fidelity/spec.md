# Feature Specification: Command fidelity

**Feature Branch**: `001-command-fidelity`

**Created**: 2026-10-17

**Status**: Draft

**Input**: User description: "Configure spec kit and plan the next 10 logical waves"

## User Scenarios & Testing *(mandatory)*

The command tree from the product specification is already present.
These ten journeys close the gaps where a shipped command still
misleads, clobbers, or stops short of the outcome an operator expects.
Each journey is one wave and can ship on its own.

### User Story 1 - Keep other AI tools when registering Authdog (Priority: P1)

An operator who already uses several MCP servers runs install for
Authdog. Their other servers stay in the same config. Uninstall removes
only the Authdog entry and leaves the rest untouched.

**Why this priority**: Install today replaces the whole client config.
That destroys unrelated servers and is unsafe to run.

**Independent Test**: Start from a config that contains a non-Authdog
server, install Authdog, confirm both servers remain, uninstall
Authdog, confirm only the other server remains.

**Acceptance Scenarios**:

1. **Given** a client config with another server, **When** the operator installs Authdog, **Then** both servers are present and Authdog points at the Authdog server URL.
2. **Given** Authdog is one of several servers, **When** the operator uninstalls Authdog, **Then** only the Authdog entry is removed.
3. **Given** no config file yet, **When** the operator installs Authdog, **Then** a config is created that contains only Authdog.

---

### User Story 2 - Expired sign-in fails clearly (Priority: P1)

A long-running or later command hits an expired management session.
The operator sees a clear request to sign in again, with exit status
for authentication required. The command does not dump the raw error
body and does not pretend a refresh succeeded.

**Why this priority**: Refresh material is stored, but the sign-in
service has no refresh route for this CLI session. Commands currently
surface a generic API failure.

**Independent Test**: Use a stored session whose access token is
rejected and confirm the command exits as authentication required and
prints no token.

**Acceptance Scenarios**:

1. **Given** a saved session the service rejects as unauthorized, **When** the operator runs a project command, **Then** the result says to sign in again and uses the authentication-required exit status.
2. **Given** that failure, **When** output is machine-readable, **Then** the payload contains a stable error field and no token material.
3. **Given** a valid session, **When** the same command runs, **Then** behavior is unchanged.

---

### User Story 3 - See what config actually changed (Priority: P1)

An operator who pulled environment settings compares the local file to
the live environment and sees which sections differ, not only that the
byte length changed.

**Why this priority**: The compare command is documented as a stub and
reports only whether the raw files match.

**Independent Test**: Change one setting in the local file, run
compare, and confirm the result names that section and reports the
unchanged sections as matching.

**Acceptance Scenarios**:

1. **Given** a local settings file and a live environment that differ in one section, **When** the operator compares them, **Then** the result names the differing section.
2. **Given** identical normalized content with harmless formatting differences, **When** the operator compares them, **Then** the result reports a match.
3. **Given** no local file, **When** the operator compares, **Then** the command says to pull first and exits as usage.

---

### User Story 4 - Pull a safe settings snapshot and preview writes (Priority: P2)

Pull includes the read-only client snapshot and webhook endpoints with
secrets removed. A dry-run of patch shows the same differences as
compare and writes nothing. Patch refuses to send client records or
secret fields.

**Why this priority**: Operators cannot review clients from the local
file, and a dry-run does not show the real change.

**Independent Test**: Pull a fixture that contains a webhook secret,
confirm the secret is absent on disk, then dry-run a patch and confirm
no remote write occurs.

**Acceptance Scenarios**:

1. **Given** a linked environment, **When** the operator pulls settings, **Then** the file includes session, redirects, redacted webhooks, and a read-only client snapshot.
2. **Given** a local change to session or redirects, **When** the operator previews a patch, **Then** the preview lists those changes and does not call the service.
3. **Given** a local file that tries to change clients or a signing secret, **When** the operator applies a patch, **Then** the command refuses and leaves the service unchanged.

---

### User Story 5 - Init writes integration files without clobber (Priority: P1)

In a Next.js or Express repository, init still links the project and
writes environment variables, and it also adds the missing integration
files from the published snippet catalog. Existing application files
are left in place. The operator can choose the package manager.

**Why this priority**: Init installs a package and writes env vars, then
stops. The product goal is a working integration, not only a dependency.

**Independent Test**: Run init in a sample Next.js app that already has
a layout file and confirm a new integration file appears while the
existing layout is unchanged.

**Acceptance Scenarios**:

1. **Given** a Next.js or Express project and a linked environment, **When** the operator runs init, **Then** missing integration files are created and existing project files are unchanged.
2. **Given** `--pm` naming bun, pnpm, yarn, or npm, **When** install is not skipped, **Then** that package manager is the one invoked.
3. **Given** `--dry-run`, **When** init runs, **Then** the planned files and env lines are printed and nothing is written or installed.

---

### User Story 6 - React and FastAPI get the right install (Priority: P2)

A React project installs the React package. A FastAPI project installs
the Python package with the detected Python tool. A Python project that
is not FastAPI is not treated as FastAPI.

**Why this priority**: Detection treats any Python project as FastAPI
and does not install a Python package. React is folded into the generic
JavaScript package.

**Independent Test**: Point detection at a React `package.json` and a
FastAPI `pyproject.toml`, and at a Python project without FastAPI, and
confirm the three classifications differ.

**Acceptance Scenarios**:

1. **Given** a project that depends on React, **When** the operator runs init, **Then** the React package is the one selected.
2. **Given** a FastAPI project, **When** the operator runs init without skipping install, **Then** the Python Authdog package is installed with the detected Python tool.
3. **Given** a Python project that does not use FastAPI, **When** framework detection runs, **Then** the result is unknown unless the operator passes an explicit framework.

---

### User Story 7 - Webhook listen tells the truth about the payload (Priority: P2)

Listen still polls recent deliveries and can POST them to a local URL.
When the delivery record has no body, the forward states that the body
is unavailable instead of presenting metadata as the signed event.
When a body is present, that body is what gets forwarded.

**Why this priority**: Forward currently posts a summary object. A local
handler that verifies signatures will reject it, and the operator will
think the integration is broken.

**Independent Test**: Poll a delivery fixture with no body and confirm
the forward payload is marked body-unavailable and is not labeled as
the original signed event.

**Acceptance Scenarios**:

1. **Given** a delivery record without a body, **When** listen forwards it, **Then** the local request says the body is unavailable and includes the delivery id and event type.
2. **Given** a delivery record that includes a body, **When** listen forwards it, **Then** the local request body is that recorded body.
3. **Given** machine-readable output, **When** listen finishes, **Then** the summary counts polls, tracked deliveries, and how many forwards included a body.

---

### User Story 8 - Impersonation shows a sign-in URL and can be revoked (Priority: P2)

Creating an impersonation session prints a sign-in URL or explicit
paste instructions. It does not print the raw service response.
Revoke ends a grant by id. Creating a grant without an interactive
prompt requires a reason.

**Why this priority**: The current command echoes the service body,
which can contain the impersonation token, and there is no revoke
command.

**Independent Test**: Mint against a fixture response that contains a
token and confirm stdout has a URL and does not contain the raw token
field. Revoke a grant id and confirm the revoke call is made.

**Acceptance Scenarios**:

1. **Given** a linked environment and a target user, **When** the operator impersonates that user, **Then** the output is a sign-in URL or paste instructions and omits the raw token field.
2. **Given** `--non-interactive` and grant creation, **When** no reason is provided, **Then** the command fails usage.
3. **Given** a grant id, **When** the operator revokes it, **Then** the grant is revoked and the output names the grant id.

---

### User Story 9 - Deploy status is a checklist (Priority: P2)

Deploy status prints one row per readiness check: security posture,
vanity domains, redirect configuration, and webhook endpoints. Each row
is pass or fail with a short detail. Machine-readable output is the
same list.

**Why this priority**: Status currently dumps two raw service documents
and skips redirects and webhooks, so an agent cannot tell whether the
environment is ready.

**Independent Test**: Run status against fixtures and confirm four named
checks with pass or fail, and no raw secret material.

**Acceptance Scenarios**:

1. **Given** a linked environment, **When** the operator runs deploy status, **Then** the result lists posture, vanity domains, redirects, and webhooks, each with pass or fail.
2. **Given** machine-readable output, **When** status runs, **Then** each check has a name, an ok flag, and a detail string.
3. **Given** a failing check, **When** status finishes, **Then** the process still exits success for a completed report, and the failing row is visible.

---

### User Story 10 - The ten outcomes stay locked (Priority: P3)

Each outcome above has an automated check that fails if the command
regresses. A published command contract lists the machine-readable
fields operators and agents may rely on.

**Why this priority**: These fixes are easy to undo in a later edit.
The contract is the last wave because it records the earlier waves.

**Independent Test**: Run the project test command and confirm the new
checks pass, and confirm the contract document matches the fields those
checks assert.

**Acceptance Scenarios**:

1. **Given** the test suite, **When** it runs with no network, **Then** the fidelity checks for stories 1–9 pass.
2. **Given** the command contract, **When** a structured field changes, **Then** the contract and the tests change in the same wave.
3. **Given** a help listing, **When** an operator reads compare and uninstall, **Then** the text matches the shipped behavior.

---

### Edge Cases

- Install when the existing client config is not valid JSON: fail usage
  and leave the file unchanged.
- Uninstall when Authdog is not present: succeed and report that there
  was nothing to remove.
- Compare when live and local section order differs but values match:
  report a match.
- Init when the integration file already exists: skip that file and say
  so.
- Init when the chosen package manager is not on `PATH`: fail with the
  missing tool name and leave env and project files as far as the
  command got, reported in the error.
- Listen when the forward URL rejects the POST: count it as not
  forwarded and continue polling.
- Impersonate when the service returns no redirect: print paste
  instructions and still omit the raw response body.
- Deploy status when one readiness call fails: that row fails and the
  other checks still appear.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: MCP install MUST merge Authdog into an existing client
  config and MUST preserve every other server entry.
- **FR-002**: MCP uninstall MUST remove only the Authdog server entry.
- **FR-003**: A management call rejected as unauthorized MUST exit as
  authentication required and MUST tell the operator to sign in again.
- **FR-004**: The CLI MUST NOT claim a session refresh succeeded.
  Identity exposes no refresh route for this CLI session.
- **FR-005**: Config compare MUST report per-section differences after
  normalizing formatting.
- **FR-006**: Config pull MUST include session, redirects, redacted
  webhooks, and a read-only client snapshot.
- **FR-007**: Config patch preview MUST list the same section changes
  as compare and MUST NOT write remotely. Apply MUST refuse client
  records and secret fields.
- **FR-008**: Init MUST add missing Next.js and Express integration
  files from the published snippet catalog and MUST leave existing
  project files unchanged.
- **FR-009**: Init MUST honor an explicit package-manager choice.
- **FR-010**: Framework detection MUST select the React package for a
  React app, install the Python package for FastAPI, and MUST classify
  a non-FastAPI Python project as unknown unless a framework is passed.
- **FR-011**: Webhook listen MUST forward a recorded body when one
  exists, and MUST label a forward as body-unavailable when the
  delivery record has no body.
- **FR-012**: Impersonate MUST present a sign-in URL or paste
  instructions and MUST NOT print the raw service body. Revoke MUST
  accept a grant id. Non-interactive grant creation MUST require a
  reason.
- **FR-013**: Deploy status MUST emit four named checks (posture,
  vanity domains, redirects, webhooks), each with pass or fail and a
  short detail.
- **FR-014**: Automated checks MUST cover FR-001 through FR-013 without
  network access, and a command contract MUST list the structured
  fields those checks assert.
- **FR-015**: Help text for compare and uninstall MUST describe the
  behavior those commands actually perform.

### Key Entities

- **Client MCP config**: The operator's local AI-client server list.
  Authdog is one named entry among others.
- **Management session**: Stored sign-in used for project commands.
  Contains an access token and unused refresh material until Identity
  grows a refresh route.
- **Environment settings file**: Local snapshot of session, redirects,
  redacted webhooks, and read-only clients.
- **Integration file**: A snippet-catalog file init may create only
  when that path is absent.
- **Webhook delivery**: A recorded attempt. It may or may not include
  the original body.
- **Impersonation grant**: A time-bounded approval that can be created
  and revoked. The minted sign-in handle is shown only as a URL or
  paste instruction.
- **Readiness check**: One named pass/fail row in deploy status.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An operator with two existing AI servers can install and
  then uninstall Authdog and still have exactly those two servers.
- **SC-002**: An expired session produces an authentication-required
  result in one step, with no token text in the output.
- **SC-003**: A one-section settings change is named in compare output
  in a single run, and a formatting-only change is reported as a match.
- **SC-004**: A pulled settings file contains zero signing-secret
  values from the fixture, and a patch preview performs zero remote
  writes.
- **SC-005**: Init in a sample Next.js app creates the missing
  integration file and leaves every pre-existing project file
  byte-for-byte unchanged.
- **SC-006**: React, FastAPI, and non-FastAPI Python samples receive
  three different framework results.
- **SC-007**: A delivery without a body is forwarded as
  body-unavailable; a delivery with a body is forwarded as that body.
- **SC-008**: Impersonation output contains a sign-in URL or paste
  instructions and zero copies of the raw token field.
- **SC-009**: Deploy status always returns exactly four named checks.
- **SC-010**: The offline test run covers stories 1–9 and matches the
  published command contract.

## Assumptions

- Cursor is the first MCP client. Other clients stay out of these ten
  waves.
- Identity will not add a CLI refresh route inside this feature.
  Expired sessions ask the operator to sign in again.
- The public delivery list does not include webhook bodies today.
  Listen stays honest until a later platform change adds them.
- Snippet text comes from the existing snippet catalog. Init does not
  invent new integration samples.
- A completed deploy report exits success even when a check fails, so
  agents can read the rows. Usage and auth failures still use their
  existing exit statuses.
- Starter templates, temporary webhook tunnel rewrites, and a generic
  API client remain outside this feature.
- Platform-next 025 waves 1–5 stay closed. This feature continues from
  that baseline in the CLI repo.
