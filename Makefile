.PHONY: all build release release-tag tag tag-push run clean check fmt clippy test wasm tenants projects bazel-build bazel-test

MKROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
# When tagging locally, refresh remote tags before computing the next `-beta.{n}` (set to 0 to stay offline-only).
RELEASE_FETCH_TAGS ?= 1

all: build

build:
	cargo build

release:
	cargo build --release

## Print the next release tag Cargo + git tags would compute (respects RELEASE_FETCH_TAGS).
release-tag:
	@RELEASE_FETCH_TAGS="$(RELEASE_FETCH_TAGS)" python3 "$(MKROOT)/scripts/compute_release_tag.py"

## Annotated git tag derived from `./Cargo.toml` (`[package].version`, `[package.metadata.authdog-release].stable`).
tag:
	@RELEASE_FETCH_TAGS="$(RELEASE_FETCH_TAGS)" "$(MKROOT)/scripts/create-local-release-tag.sh"

tag-push:
	@RELEASE_FETCH_TAGS="$(RELEASE_FETCH_TAGS)" "$(MKROOT)/scripts/push-local-release-tag.sh"

# Usage: make run ARGS='--whatever'
ARGS ?=
run:
	cargo run -- $(ARGS)

check:
	cargo check

fmt:
	cargo fmt

clippy:
	cargo clippy --all-targets

test:
	cargo test

WASM_PKG := authdog-cli-wasm
WASM_TARGET := wasm32-unknown-unknown
WASM_OUT := target/$(WASM_TARGET)/release/authdog_cli_wasm.wasm

## Build embeddable `wasm-bindgen` artifact (JWT helpers only — no Ratatui / OAuth).
wasm:
	rustup target add $(WASM_TARGET) >/dev/null 2>&1 || true
	cargo build -p $(WASM_PKG) --release --target $(WASM_TARGET)
	@echo "WASM artifact: $$(pwd)/$(WASM_OUT)"

## Run tests tied to tenants (substring match: TUI separators + tenants REST error shaping).
tenants:
	cargo test -p authdog-cli tenants

## Run tests tied to projects (substring match: lib projects REST + TUI JSON banner).
projects:
	cargo test -p authdog-cli projects

clean:
	cargo clean

## Hermetic build via Bazel (requires Bazelisk or `bazel` on PATH; see `.bazelversion`).
bazel-build:
	bazel build //:authdog-cli

## Library unit tests under Bazel (`src/tui_output.rs` tests stay on `cargo test` only).
bazel-test:
	bazel test //:authdog_cli_lib_test
