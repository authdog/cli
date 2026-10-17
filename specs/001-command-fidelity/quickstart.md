# Quickstart: Command fidelity

Offline checks prove each wave. Live tenant steps are optional and
need `authdog login` plus a linked `.authdog/project.json`.

Run from the repo root:

```bash
zig build test
```

## Wave 1 — MCP merge

1. Write a temp `mcp.json` with a server named `other`.
2. Point the test at that file and run the install helper.
3. Expect both `other` and `authdog`.
4. Run uninstall. Expect only `other`.
5. Repeat install with `{` as the file. Expect usage failure and an
   unchanged file.

## Wave 2 — Expired session

1. Stub an HTTP 401 body.
2. Expect exit code 3, `error` = `session_expired`, and no
   `access_token` or `refresh_token` substring in stdout.

## Wave 3 — Config diff

1. Build two documents with the same `sessionConfig` and different
   `redirectUris`, one pretty-printed and one compact.
2. Expect `sessionConfig: match`, `redirectUris: differ`,
   `matches: false`.

## Wave 4 — Redaction and dry-run

1. Pull fixture JSON that includes `signingSecret`.
2. Expect the written file to contain no `signingSecret` value.
3. Dry-run a session change. Expect `dryRun: true` and zero PUT
   attempts in the stub client.

## Wave 5 — Init files

1. In a temp dir with `package.json` containing `next` and an existing
   `app/layout.tsx`, run init `--dry-run --pm bun`.
2. Expect `packageManager` `bun` and one `create` integration path.
3. Apply init with `--skip-install`. Expect `app/layout.tsx` bytes
   unchanged and the new snippet path present.
4. Run again. Expect `skip-exists`.

## Wave 6 — Frameworks

1. `package.json` with `react` and without `next` → React package
   `@authdog/react`.
2. `pyproject.toml` containing `fastapi` → Python package `authdog`.
3. `pyproject.toml` without `fastapi` → framework `unknown`.

## Wave 7 — Listen forward

1. Delivery object without `body` → POST body has
   `payloadAvailable: false` and the delivery id.
2. Delivery object with `body` → POST body equals that field.
3. Summary `forwardedWithBody` counts only the second case.

## Wave 8 — Impersonate

1. Fixture response `{ "token": "sekret", "redirectTo": "https://app.example/cb" }`.
2. Expect `signInUrl` to start with `https://app.example/cb?token=`.
3. Expect stdout not to contain a JSON key named `token`.
4. `--non-interactive` grant creation without `--reason` → exit 2.
5. `impersonate revoke --grant-id` hits the revoke path and prints
   `revoked: true`.

## Wave 9 — Deploy checklist

1. Stub four GETs, one of them HTTP 500.
2. Expect exactly the names `security-posture`, `vanity-domains`,
   `redirects`, `webhooks`, one `ok: false`, exit 0.

## Wave 10 — Contract

1. `zig build test` covers the cases above.
2. Help text contains neither `Compare local config (stub)` nor
   `Reserved (does not delete shared config)`.
3. Field names in the tests match [command-output.md](./contracts/command-output.md).
