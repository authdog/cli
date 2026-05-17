.PHONY: all build release run clean check fmt clippy test wasm tenants projects bazel-build bazel-test

all: build

build:
	cargo build

release:
	cargo build --release

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
