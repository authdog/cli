.PHONY: all build release run clean check fmt clippy test

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

clean:
	cargo clean
