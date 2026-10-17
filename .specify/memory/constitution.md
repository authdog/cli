<!--
Sync Impact Report
- Version change: template (unversioned) → 1.0.0
- Modified principles: placeholders replaced with Authdog CLI principles I–V
- Added sections: Additional Constraints, Development Workflow
- Removed sections: none (template slots filled)
- Follow-up TODOs: none
-->

# Authdog CLI Constitution

## Core Principles

### I. Single Zig binary

The published CLI MUST remain one Zig executable. It MUST NOT embed a
JavaScript runtime. Installing packages and applying project files MAY
invoke a package manager already present on `PATH`.

Rationale: operators install one binary from `cli.auth.dog`. A second
runtime would split release, support, and agent behavior.

### II. Secrets stay off output and out of the repo

Commands MUST NOT print access tokens, refresh tokens, client secrets,
private keys, or webhook signing secrets in text or JSON. Credential
files MUST remain mode `0600` and MUST live outside the project tree.
Project files such as `.authdog/project.json` MUST stay free of secrets.

Rationale: agents and CI capture stdout. A leaked token in `--json` is a
production incident.

### III. Agent-stable interface

Every command MUST honor `--json` / `-o json` and `--non-interactive`.
Exit codes MUST stay `0` success, `2` usage, `3` authentication required,
`4` API error. Structured success and error payloads MUST include
`schemaVersion`. `--non-interactive` MUST fail instead of prompting.

Rationale: coding agents and CI cannot answer prompts, and they need a
stable machine contract.

### IV. Specs name a purpose

`SPEC.md` is the command-behavior source of truth. Delivery work lives
in `specs/NNN-purpose/` using Spec Kit files (`spec.md`, `plan.md`,
`tasks.md`, and `research.md` when needed). The folder name MUST state
what the work accomplishes. A sequence label (`next-waves`,
`follow-ups`, `more`) is not a purpose. The feature branch MUST use the
same name.

Rationale: numbered waves are readable only when the directory says why
they exist.

### V. Tests cover parsers, crypto, and file writes

Changes to argument parsing, signature checks, credential handling, and
writes to operator config files MUST ship with Zig tests. Tests MUST
NOT print token material. A command that mutates a shared config file
MUST have a test that an unrelated entry survives.

Rationale: the highest-risk bugs in this CLI are silent file clobber and
signature drift, both of which unit tests catch without a live tenant.

## Additional Constraints

- Version and stability come from `release.toml`. A breaking JSON
  `schemaVersion` bump MUST be called out in the changelog.
- This repo MUST NOT invent unpublished HTTP paths. A missing platform
  route is a recorded dependency, not a private protocol.
- HTTP behavior changes that require Management, Identity, or OpenAPI
  edits belong in `platform-next`. This repo consumes those contracts.
- Public docs MUST NOT include secrets, session material, or incident
  detail.

## Development Workflow

- Format with `zig fmt`. Gate with `zig build test`.
- Implementation plans MUST include a Constitution Check before research
  and again after design.
- Waves are ordered so a later wave can assume the previous wave's
  user-visible outcome. A wave MUST be demonstrable on its own.
- Plans MUST NOT add scope that is absent from the feature spec or from
  research grounded in the current tree.

## Governance

This constitution supersedes conflicting local habits for Spec Kit work.
Amendments require an edit to this file, a semantic version bump, and an
updated last-amended date. MAJOR removes or redefines a principle.
MINOR adds a principle or expands a rule. PATCH clarifies wording.

Reviews MUST confirm the Constitution Check passed. Complexity that
breaks a principle MUST be listed in the plan's Complexity Tracking
table with the rejected simpler alternative.

**Version**: 1.0.0 | **Ratified**: 2026-10-17 | **Last Amended**: 2026-10-17
