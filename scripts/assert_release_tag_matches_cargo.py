#!/usr/bin/env python3
"""Ensure the pushed/dispatched git tag matches [package].version in Cargo.toml (stable or -beta.<n>)."""

from __future__ import annotations

import os
import re
import sys
import tomllib


def main() -> None:
    raw = (
        os.environ.get("RELEASE_TAG")
        or os.environ.get("TAG")
        or os.environ.get("GITHUB_REF_NAME")
        or ""
    ).strip()
    if not raw.startswith("v"):
        print(f"error: release tag must start with 'v' (got {raw!r})", file=sys.stderr)
        sys.exit(2)

    body = raw.removeprefix("v")

    cargo_path = os.path.join(os.path.dirname(__file__), "..", "Cargo.toml")
    cargo_path = os.path.normpath(cargo_path)
    with open(cargo_path, "rb") as f:
        package = tomllib.load(f).get("package")

    if not isinstance(package, dict):
        print("error: missing [package] table in Cargo.toml", file=sys.stderr)
        sys.exit(2)

    version = str(package.get("version") or "").strip()
    if not version:
        print("error: missing [package].version in Cargo.toml", file=sys.stderr)
        sys.exit(2)

    beta = re.fullmatch(re.escape(version) + r"-beta\.\d+", body)
    if beta:
        print(f"ok: beta tag {raw} matches Cargo.toml version {version}")
        return

    if body == version:
        print(f"ok: release tag {raw} matches Cargo.toml version {version}")
        return

    print(
        "error: tag does not match Cargo.toml "
        f"[package].version={version!r}: got derived payload {body!r} "
        f"(expect v{version} or v{version}-beta.<n>)",
        file=sys.stderr,
    )
    sys.exit(1)


if __name__ == "__main__":
    main()
