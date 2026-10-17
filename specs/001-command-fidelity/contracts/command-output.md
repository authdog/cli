# Command output contract

Structured output is JSON with `schemaVersion: 1`. Text mode may
paraphrase the same facts and MUST NOT add secrets. Exit codes: `0`
success, `2` usage, `3` authentication required, `4` API error.

## `mcp install` / `mcp uninstall`

```json
{
  "schemaVersion": 1,
  "ok": true,
  "configPath": "/home/me/.cursor/mcp.json",
  "serverUrl": "https://api.authdog.com/mcp",
  "preservedServers": ["other"]
}
```

`preservedServers` lists every `mcpServers` key other than `authdog`
after the write. Uninstall omits `serverUrl` and adds
`removed: true|false`.

## Session expired (any management command)

Exit `3`.

```json
{
  "schemaVersion": 1,
  "ok": false,
  "error": "session_expired"
}
```

Text: `session expired; run authdog login`.

## `config diff`

```json
{
  "schemaVersion": 1,
  "ok": true,
  "matches": false,
  "sections": {
    "sessionConfig": "match",
    "redirectUris": "differ",
    "webhooks": "match",
    "oidcClients": "match"
  }
}
```

`matches` is true only when every section is `match`.

## `config patch --dry-run`

```json
{
  "schemaVersion": 1,
  "ok": true,
  "dryRun": true,
  "sections": {
    "sessionConfig": "differ",
    "redirectUris": "match",
    "webhooks": "match",
    "oidcClients": "match"
  }
}
```

No PUT is performed. A differing `oidcClients` or a secret key is exit
`2` with `error: "patch_refused"`.

## `init --dry-run`

Existing env fields stay. Add:

```json
{
  "packageManager": "bun",
  "integrationFiles": [
    { "path": "app/authdog-provider.tsx", "action": "create" }
  ]
}
```

`action` is `create` or `skip-exists`.

## `webhooks listen`

```json
{
  "schemaVersion": 1,
  "ok": true,
  "polls": 1,
  "trackedDeliveries": 2,
  "forwarded": 1,
  "forwardedWithBody": 0
}
```

## `impersonate` / `impersonate revoke`

```json
{
  "schemaVersion": 1,
  "ok": true,
  "grantId": "uuid",
  "signInUrl": "https://identity.example/callback?token=redacted-in-docs"
}
```

The live URL may contain the token because that is how the app
consumes it. The JSON MUST NOT also include a separate `token` field
or the raw response body. Revoke returns `grantId` and `revoked: true`.

## `deploy status`

```json
{
  "schemaVersion": 1,
  "ok": true,
  "checks": [
    { "name": "security-posture", "ok": true, "detail": "score available" },
    { "name": "vanity-domains", "ok": true, "detail": "1 domain" },
    { "name": "redirects", "ok": false, "detail": "request failed" },
    { "name": "webhooks", "ok": true, "detail": "1 endpoint" }
  ]
}
```

`checks` length is always 4 when the report completes.
