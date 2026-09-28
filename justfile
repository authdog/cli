root := justfile_directory()
release_fetch_tags := env_var_or_default("RELEASE_FETCH_TAGS", "1")
wasm_pkg := "authdog-cli-wasm"
wasm_target := "wasm32-unknown-unknown"
wasm_out := "target/" + wasm_target + "/release/authdog_cli_wasm.wasm"

default: build

build:
    cargo build

release:
    cargo build --release

# Print the next release tag Cargo + git tags would compute.
release-tag:
    @RELEASE_FETCH_TAGS="{{ release_fetch_tags }}" python3 "{{ root }}/scripts/compute_release_tag.py"

# Create an annotated git tag from Cargo metadata.
tag:
    @RELEASE_FETCH_TAGS="{{ release_fetch_tags }}" "{{ root }}/scripts/create-local-release-tag.sh"

tag-push:
    @RELEASE_FETCH_TAGS="{{ release_fetch_tags }}" "{{ root }}/scripts/push-local-release-tag.sh"

# Usage: just run status --json
run *args:
    cargo run -- {{ args }}

check:
    cargo check

fmt:
    cargo fmt

clippy:
    cargo clippy --all-targets

test:
    cargo test

# Build the embeddable wasm-bindgen artifact.
wasm:
    rustup target add {{ wasm_target }} >/dev/null 2>&1 || true
    cargo build -p {{ wasm_pkg }} --release --target {{ wasm_target }}
    @echo "WASM artifact: {{ root }}/{{ wasm_out }}"

clean:
    cargo clean

moon-build:
    moon run authdog-cli:build

moon-test:
    moon run authdog-cli:test
