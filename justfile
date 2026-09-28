root := justfile_directory()
release_fetch_tags := env_var_or_default("RELEASE_FETCH_TAGS", "1")

default: build

build:
    zig build

release:
    zig build -Doptimize=ReleaseSafe

# Print the next release tag release.toml + git tags would compute.
release-tag:
    @RELEASE_FETCH_TAGS="{{ release_fetch_tags }}" python3 "{{ root }}/scripts/compute_release_tag.py"

# Create an annotated git tag from release.toml.
tag:
    @RELEASE_FETCH_TAGS="{{ release_fetch_tags }}" "{{ root }}/scripts/create-local-release-tag.sh"

tag-push:
    @RELEASE_FETCH_TAGS="{{ release_fetch_tags }}" "{{ root }}/scripts/push-local-release-tag.sh"

# Usage: just run status --json
run *args:
    zig build run -- {{ args }}

check:
    zig build

fmt:
    zig fmt build.zig src

test:
    zig build test

clean:
    rm -rf zig-out .zig-cache

moon-build:
    moon run authdog-cli:build

moon-test:
    moon run authdog-cli:test
