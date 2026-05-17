# Authdog CLI

Interactive terminal CLI for **[Authdog](https://www.authdog.com)** session flows: OAuth sign-in in the browser with a localhost callback, session storage on disk, and quick calls to Identity / API helpers (`whoami`, tenants, organizations, tenant-scoped projects, JWT claim preview).

## Requirements

- **Rust toolchain** stable (Edition 2021), `cargo`.
- Unix-like environment with a usable terminal (**Ratatui** over **crossterm**).

Optional:

- **`wasm32-unknown-unknown`** target for `make wasm` (installed automatically via `rustup` in the Makefile step).

## Build & run

```bash
cargo build              # debug
cargo build --release
cargo run               # launches the fullscreen TUI
```

### Slash commands

| Command     | Behaviour |
|------------|-----------|
| `/help`    | Lists commands |
| `/login`   | Opens Identity sign-in; CLI listens on `http://127.0.0.1:<port>/oauth/callback` |
| `/logout`  | Deletes saved credentials locally |
| `/whoami`  | Loads identity from **`GET …/v1/userinfo`** (API host below) plus optional JWT claim formatting |
| `/tenants` | **`GET …/v1/tenants`** listing (JSON) |
| `/tenant` | Show, set (`/tenant <uuid>`), or clear (`/tenant clear`) the **current tenant** stored in `credentials.json` (validated against `/tenants` when that request succeeds) |
| `/projects` | **`GET …/v1/tenants/{tenantId}/projects`** (JSON); requires a current tenant from **`/tenant`** |
| `/organizations` | **`GET …/v1/organizations`** listing (JSON); alias **`/orgs`** |
| `/status`  | Credentials path, **current tenant** (if any), opaque token previews |
| `/quit`    | Exit |

### Environment variables

| Variable | Purpose |
|----------|---------|
| `AUTHDOG_IDENTITY_ORIGIN` | Identity host (default `https://identity.authdog.com`) |
| `AUTHDOG_CONSOLE_ENVIRONMENT_ID` | Sign-in environment UUID (hosted console default wired in sources) |
| `AUTHDOG_API_ORIGIN` | REST API origin for `/v1/userinfo`, `/v1/tenants`, `/v1/tenants/{id}/projects`, `/v1/organizations` (default **`https://api.authdog.com`**) |

## Makefile targets

| Target | Description |
|--------|--------------|
| `make` / `make build` | `cargo build` |
| `make release` | `cargo build --release` |
| `make run` | `cargo run` (optional `ARGS=…`) |
| `make check` | `cargo check` |
| `make test` | `cargo test` |
| `make clippy` | `cargo clippy --all-targets` |
| `make fmt` | `cargo fmt` |
| **`make wasm`** | Release build of **`authdog-cli-wasm`** → `target/wasm32-unknown-unknown/release/authdog_cli_wasm.wasm` |
| **`make tenants`** | Runs `cargo test` filtered on names containing **`tenants`** (TUI + tenants REST helpers) |
| **`make projects`** | Runs `cargo test` filtered on names containing **`projects`** |

## Workspace layout

- **`authdog-cli`** (`Cargo.toml`, `src/`) — library + **`authdog-cli`** binary (`required-features = ["desktop"]`). The Ratatui UI lives under **`src/app.rs`**, **`src/tui_output.rs`**, and slash routing under **`src/commands/`** (`registry`, `dispatch`).
- **`wasm/`** — minimal **`wasm-bindgen`** **`cdylib`** built on JWT helpers from the core crate (no terminal / OAuth).

The desktop feature pulls Ratatui, Crossterm, blocking `reqwest`, OAuth loopback TCP, filesystem session store, etc. The WASM package depends on **`authdog-cli` with `default-features = false`**.

## Wasm (`make wasm`)

The WASM artefact exposes **JWT payload inspection helpers** (**signatures not verified**, same caveat as CLI claim previews). Use **`wasm-pack build`** inside `wasm/` if you want generated JS bindings for the browser.

## `/tenants`, `/organizations`, `/projects`, and the REST API

The CLI calls:

- **`{AUTHDOG_API_ORIGIN}/v1/tenants`**
- **`{AUTHDOG_API_ORIGIN}/v1/tenants/{tenantId}/projects`** (`/projects`, tenant id from **`/tenant`**)
- **`{AUTHDOG_API_ORIGIN}/v1/organizations`** (`/organizations` or `/orgs`)

Each request uses `Authorization: Bearer <access_token>`.

Upstream, these routes are backed by Management GraphQL; error responses may include **`error`** plus **`detail`**. When present, those fields are surfaced in the CLI output for easier debugging without raw JSON truncation.

Deployed API behaviour lives in **`platform-next/services/api`** (handlers under the respective `/v1/…` paths); ensure that stack matches your CLI version if listing fails (`403`, `401`, empty list, etc.).

If the error **`detail`** includes Cloudflare **`error code: 1003`**, the API Worker cannot reach Management by hostname (often `MANAGEMENT_ENDPOINT` is missing, wrong, or set to an **IP** instead of **`https://mgt.authdog.com/graphql`**—see **`platform-next/services/api/wrangler.prod.toml`** and Workers secrets for **`authdog-api-prod-v2`**).

## Offline note

`/login` succeeds only with network connectivity to Identity and reachable loopback OAuth callback. Offline usage is mostly limited to reading stored credentials (`/status`) and inspecting JWT payloads (no server round-trip).

## License

MIT
