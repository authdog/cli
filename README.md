# Authdog CLI

Scriptable CLI for **[Authdog](https://www.authdog.com)** identity inspection and resource context. Conventional commands provide stable text or JSON output; **`authdog ui`** keeps the fullscreen browser for interactive navigation.

## Installation

Install a prebuilt binary from **[cli.auth.dog](https://cli.auth.dog)** (picked for your OS/arch from GitHub Releases):

```bash
curl https://cli.auth.dog/install -fsS | bash
```

Ensure **`$HOME/.local/bin`** (or your **`INSTALL_DIR`**) is on **`PATH`**—the installer defaults there. Options such as **`AUTHDOG_CLI_VERSION`**, **`INSTALL_DIR`**, and Linux musl are documented in **[`install/SPEC.md`](install/SPEC.md)**. Windows installers: **`install.ps1`** at the same host (see SPEC).

Maintainers deploy the Worker from **`install/`** via **[`.github/workflows/cli-install-deploy.yml`](.github/workflows/cli-install-deploy.yml)**; routing and **`wrangler.toml`** vars are covered in SPEC.

## Requirements (build from source)

- **Rust toolchain** stable (Edition 2021), `cargo`.
- Unix-like environment with a usable terminal (**Ratatui** over **crossterm**).

Optional:

- **`wasm32-unknown-unknown`** target for `make wasm` (installed automatically via `rustup` in the Makefile step).
- **[moon](https://moonrepo.dev)** on PATH for `make moon-build` / `make moon-test`.

## GitHub Releases

When a tag like **`0.1.0`** or **`0.1.0-beta.1`** (bare semver — **no** leading **`v`**) is pushed to **`origin`** on GitHub, the **Release** workflow (`.github/workflows/release.yml`) cross-builds **`authdog`**, attaches `authdog-cli-<version>-<target>` archives + **`checksums.sha256`**, and creates/updates that tag’s **[GitHub Release](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)**. Archives include **`authdog`** and a one-release **`authdog-cli`** compatibility copy. Use **`make tag-push`** (or Actions **Create release tag**) so the tag matches `./Cargo.toml` `[package].version` and the **`[package.metadata.authdog-release]`** rules.

## Build & run

```bash
cargo build              # debug
cargo build --release
cargo run -- --help
cargo run -- ui          # launches the fullscreen TUI
```

## Commands

```bash
authdog login
authdog whoami
authdog organizations list
authdog tenants list [--organization ID]
authdog projects list [--tenant ID]
authdog environments list [--tenant ID] [--project ID]
authdog context show
authdog context set tenant TENANT_ID
authdog context clear project
authdog status
authdog logout
authdog ui
```

Use global **`-o json`** or **`--json`** for machine-readable output:

```bash
authdog organizations list --json | jq .
authdog status -o json
```

Explicit resource flags override saved context. Projects fall back to the selected tenant; environments fall back to selected tenant and project. Context changes cascade safely: changing an organization clears tenant/project/environment, changing a tenant clears project/environment, and changing a project clears environment.

`status` and `context show` never print access or refresh tokens. Set **`AUTHDOG_CONFIG_DIR`** to override the local config directory for isolated development or CI; normal installs continue using the existing `authdog-cli` config directory.

## Fullscreen UI

Run **`authdog ui`** to use the original Ratatui interface. Its slash commands remain available:

| Command | Behaviour |
|--------|-----------|
| `/help` | Lists commands |
| `/login` | Opens Identity sign-in; CLI listens on `http://127.0.0.1:<port>/oauth/callback` |
| `/logout` | Deletes saved credentials locally |
| `/whoami` | Identity from **`GET …/v1/userinfo`** (API host below) plus optional JWT claim formatting |
| `/tenants` | **`GET …/v1/tenants`** listing (JSON) |
| `/tenant` | Show, set (`/tenant <uuid>`), or clear (`/tenant clear`) the **current tenant** stored in `credentials.json` (validated against `/tenants` when that request succeeds) |
| `/projects` | **`GET …/v1/tenants/{tenantId}/projects`** (JSON); requires a current tenant from **`/tenant`** |
| `/browse` | Interactive flow: organizations → tenants → projects → environments; **↑ / ↓** to move, **Enter** to confirm, **Esc** to step back. |
| `/organizations` | **`GET …/v1/organizations`** listing (JSON); alias **`/orgs`** |
| `/status` | Credentials path and active organization/tenant/project/environment context |
| `/quit` | Exit |

### Environment variables

| Variable | Purpose |
|----------|---------|
| `AUTHDOG_IDENTITY_ORIGIN` | Identity host (default `https://identity.authdog.com`) |
| `AUTHDOG_CONSOLE_ENVIRONMENT_ID` | Sign-in environment UUID (hosted console default wired in sources) |
| `AUTHDOG_API_ORIGIN` | REST API origin for `/v1/userinfo`, `/v1/tenants`, `/v1/tenants/{id}/projects`, `/v1/organizations` (default **`https://api.authdog.com`**) |
| `AUTHDOG_CONFIG_DIR` | Optional config-directory override; default remains the OS-specific `authdog-cli` directory |

## Makefile targets

| Target | Description |
|--------|-------------|
| `make` / `make build` | `cargo build` |
| `make release` | `cargo build --release` |
| `make run` | `cargo run` (optional `ARGS=…`) |
| `make check` | `cargo check` |
| `make test` | `cargo test` |
| `make clippy` | `cargo clippy --all-targets` |
| `make fmt` | `cargo fmt` |
| **`make wasm`** | Release build of **`authdog-cli-wasm`** → `target/wasm32-unknown-unknown/release/authdog_cli_wasm.wasm` |
| **`make tenants`** | `cargo test -p authdog-cli tenants` (substring filter: tenants-focused tests) |
| **`make projects`** | `cargo test -p authdog-cli projects` (substring filter: projects-focused tests) |
| **`make moon-build`** | `moon run authdog-cli:build` (release build of the desktop CLI) |
| **`make moon-test`** | `moon run authdog-cli:test` (library unit tests) |
| `make clean` | `cargo clean` |

## Workspace layout

- **`authdog-cli`** (`Cargo.toml`, `src/`) — library package + **`authdog`** binary (`required-features = ["desktop"]`). Conventional routing lives in **`src/cli.rs`**. Ratatui UI lives under **`src/app.rs`**, **`src/tui_output.rs`**, **`src/browse.rs`**, and **`src/commands/`**.
- **`wasm/`** — minimal **`wasm-bindgen`** **`cdylib`** built on JWT helpers from the core crate (no terminal / OAuth).

The desktop feature pulls Ratatui, Crossterm, blocking `reqwest`, OAuth loopback TCP, filesystem session store, etc. The WASM package depends on **`authdog-cli` with `default-features = false`**.

## Wasm (`make wasm`)

The WASM artefact exposes **JWT payload inspection helpers** (**signatures not verified**, same caveat as CLI claim previews). Use **`wasm-pack build`** inside `wasm/` if you want generated JS bindings for the browser.

## `/tenants`, `/organizations`, `/projects`, `/browse`, and the REST API

The CLI calls:

- **`{AUTHDOG_API_ORIGIN}/v1/userinfo`** (`/whoami`)
- **`{AUTHDOG_API_ORIGIN}/v1/tenants`**
- **`{AUTHDOG_API_ORIGIN}/v1/tenants/{tenantId}/projects`** (`/projects` and **`/browse`** after a tenant is chosen)
- **`{AUTHDOG_API_ORIGIN}/v1/organizations`** (`/organizations`, **`/orgs`**, first step of **`/browse`**)

Each request uses `Authorization: Bearer <access_token>`.

If **`/organizations`** returns an empty list, **`/browse`** skips straight to **`/tenants`** (labelled as having no organizations).

Upstream, these routes are backed by Management GraphQL; error responses may include **`error`** plus **`detail`**. When present, those fields are surfaced in the CLI output for easier debugging without raw JSON truncation.

Deployed API behaviour lives in **`platform-next/services/api`** (handlers under the respective `/v1/…` paths); ensure that stack matches your CLI version if listing fails (`403`, `401`, empty list, etc.).

If the error **`detail`** includes Cloudflare **`error code: 1003`**, the API Worker cannot reach Management by hostname (often `MANAGEMENT_ENDPOINT` is missing, wrong, or set to an **IP** instead of **`https://mgt.authdog.com/graphql`**—see **`platform-next/services/api/wrangler.prod.toml`** and Workers secrets for **`authdog-api-prod-v2`**).

## Offline note

`/login` succeeds only with network connectivity to Identity and a reachable loopback OAuth callback. Offline usage is mostly limited to reading stored credentials (`/status`) and inspecting JWT payloads (no server round-trip).

## License

MIT
