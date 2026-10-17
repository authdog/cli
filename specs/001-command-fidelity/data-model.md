# Data model: Command fidelity

Field names below are the on-disk or stdout shapes operators can rely
on. Validation rules come from the functional requirements.

## Client MCP config

Local JSON file (Cursor: `~/.cursor/mcp.json`).

| Field | Rule |
| --- | --- |
| `mcpServers` | Object. Required after a successful install. |
| `mcpServers.authdog.url` | Authdog MCP URL for the active API origin. |
| Other `mcpServers.*` keys | Preserved on install and uninstall. |

Invalid JSON is a usage failure. The file is not rewritten.

## Management session

Existing `credentials.json`. This feature does not add fields.

| Field | Rule |
| --- | --- |
| `access_token` | Sent as the bearer token. Never printed. |
| `refresh_token` | Stored. Not sent. No refresh route exists. Never printed. |

A 401 on a management call produces exit 3 and
`error: "session_expired"` in JSON. It does not modify the file.

## Environment settings file

`.authdog/config.json`, schemaVersion 1.

| Field | Rule |
| --- | --- |
| `schemaVersion` | `1` |
| `sessionConfig` | Live session document. Patch may PUT this. |
| `redirectUris` | Live redirect document. Patch may PUT this. |
| `webhooks` | Live webhook list with `signingSecret`, `secret`, `clientSecret`, and `token` removed at any depth. |
| `oidcClients` | Read-only snapshot. Patch MUST refuse the command if this section differs from live. |

Compare classifies each of the four sections as `match` or `differ`
after JSON value equality (key order ignored).

## Integration file

A path from the vendored snippet catalog.

| Field | Rule |
| --- | --- |
| Destination path | Written only when absent. |
| Contents | Catalog bytes for that framework. No secret substitution beyond env vars already written to the env file. |

## Webhook forward body

POST body to `--forward`.

| Case | Body |
| --- | --- |
| Delivery has `body` or `payload` | Those bytes, unchanged. |
| Delivery has neither | `{"deliveryId","eventType","payloadAvailable":false}` |

The summary object includes `polls`, `trackedDeliveries`,
`forwarded`, and `forwardedWithBody`.

## Impersonation result

| Field | Rule |
| --- | --- |
| `signInUrl` | Present when Identity returns `redirectTo`. Token is placed in the query string of that URL only. |
| `instructions` | Present when `redirectTo` is absent. Tells the operator to paste into the app callback. Does not include the token. |
| `grantId` | Present when a grant was created or revoked. |

`impersonate revoke` input is `--grant-id`. Non-interactive grant
creation requires `--reason`.

## Readiness check

Four rows, fixed names: `security-posture`, `vanity-domains`,
`redirects`, `webhooks`.

| Field | Rule |
| --- | --- |
| `name` | One of the four names. |
| `ok` | True when that call returns HTTP 2xx. |
| `detail` | One short sentence. No raw response body. |

A finished report exits 0. Missing auth still exits 3.
