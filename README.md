# Authdog CLI

CLI for **[Authdog](https://www.authdog.com)** identity authentication.

## Installation

Install a prebuilt binary from **[cli.auth.dog](https://cli.auth.dog)** (picked for your OS/arch from GitHub Releases):

```bash
curl https://cli.auth.dog/install -fsS | bash
```

Ensure **`$HOME/.local/bin`** (or your **`INSTALL_DIR`**) is on **`PATH`**—the installer defaults there. Options such as **`AUTHDOG_CLI_VERSION`**, **`INSTALL_DIR`**, and Linux musl are documented in **[`install/SPEC.md`](install/SPEC.md)**. Windows installers: **`install.ps1`** at the same host (see SPEC).

Maintainers deploy the Worker from **`install/`** via **[`.github/workflows/cli-install-deploy.yml`](.github/workflows/cli-install-deploy.yml)**; routing and **`wrangler.toml`** vars are covered in SPEC.

## Requirements (build from source)

- **Rust toolchain** stable (Edition 2021), `cargo`.

Optional:

- **[just](https://just.systems)** for repository recipes.
- **`wasm32-unknown-unknown`** target for `just wasm` (installed automatically via `rustup` by the recipe).
- **[moon](https://moonrepo.dev)** on PATH for `just moon-build` / `just moon-test`.

## GitHub Releases

When a tag like **`0.1.0`** or **`0.1.0-beta.1`** (bare semver — **no** leading **`v`**) is pushed to **`origin`** on GitHub, the **Release** workflow (`.github/workflows/release.yml`) cross-builds **`authdog`**, attaches `authdog-cli-<version>-<target>` archives + **`checksums.sha256`**, and creates/updates that tag’s **[GitHub Release](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)**. Archives include **`authdog`** and a one-release **`authdog-cli`** compatibility copy. Use **`just tag-push`** (or Actions **Create release tag**) so the tag matches `./Cargo.toml` `[package].version` and the **`[package.metadata.authdog-release]`** rules.

## Build & run

```bash
cargo build              # debug
cargo build --release
cargo run -- --help
```

## Commands

```bash
authdog login
authdog status
authdog logout
```

Use global **`-o json`** or **`--json`** for machine-readable output:

```bash
authdog status -o json
```

`status` never prints access or refresh tokens. Set **`AUTHDOG_CONFIG_DIR`** to override the local config directory for isolated development or CI; normal installs continue using the existing `authdog-cli` config directory.

### Environment variables

| Variable | Purpose |
|----------|---------|
| `AUTHDOG_IDENTITY_ORIGIN` | Identity host (default `https://identity.authdog.com`) |
| `AUTHDOG_CONSOLE_ENVIRONMENT_ID` | Sign-in environment UUID (hosted console default wired in sources) |
| `AUTHDOG_CONFIG_DIR` | Optional config-directory override; default remains the OS-specific `authdog-cli` directory |

## Just recipes

| Recipe | Description |
|--------|-------------|
| `just` / `just build` | `cargo build` |
| `just release` | `cargo build --release` |
| `just run [ARGS]…` | `cargo run` with optional arguments |
| `just check` | `cargo check` |
| `just test` | `cargo test` |
| `just clippy` | `cargo clippy --all-targets` |
| `just fmt` | `cargo fmt` |
| **`just wasm`** | Release build of **`authdog-cli-wasm`** → `target/wasm32-unknown-unknown/release/authdog_cli_wasm.wasm` |
| **`just moon-build`** | `moon run authdog-cli:build` (release build of the desktop CLI) |
| **`just moon-test`** | `moon run authdog-cli:test` (library unit tests) |
| `just clean` | `cargo clean` |

## Workspace layout

- **`authdog-cli`** (`Cargo.toml`, `src/`) — library package + **`authdog`** binary (`required-features = ["desktop"]`). CLI routing lives in **`src/cli.rs`**, browser login flow in **`src/cli_login.rs`**, and session storage in **`src/session_store.rs`**.
- **`wasm/`** — minimal **`wasm-bindgen`** **`cdylib`** built on JWT helpers from the core crate (no terminal / OAuth).

The desktop feature pulls blocking `reqwest`, OAuth loopback TCP, filesystem session store, etc. The WASM package depends on **`authdog-cli` with `default-features = false`**.

## Wasm (`just wasm`)

The WASM artefact exposes **JWT payload inspection helpers** (**signatures not verified**, same caveat as claim previews). Use **`wasm-pack build`** inside `wasm/` if you want generated JS bindings for the browser.

## Offline note

`/login` succeeds only with network connectivity to Identity and a reachable loopback OAuth callback. Offline usage is limited to reading stored credentials (`/status`).

## License

MIT
