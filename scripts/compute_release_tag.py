#!/usr/bin/env python3
"""Compute the next release git tag from Cargo.toml ([package].version + metadata)."""

from __future__ import annotations

import os
import re
import subprocess
import sys
import tomllib


def _repo_root() -> str:
    proc = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if proc.returncode != 0 or not proc.stdout.strip():
        print("error: not inside a git repository", file=sys.stderr)
        sys.exit(2)
    return proc.stdout.strip()


def _fetch_tags(root: str) -> None:
    if os.environ.get("RELEASE_FETCH_TAGS", "1") == "0":
        return
    subprocess.run(
        ["git", "-C", root, "fetch", "origin", "--tags"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
        env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
    )


def _max_beta_suffix(root: str, base: str) -> int:
    prefix = f"v{base}-beta."
    proc = subprocess.run(
        ["git", "-C", root, "tag", "-l", f"{prefix}*"],
        text=True,
        stdout=subprocess.PIPE,
        check=True,
    )
    tags = [t for t in proc.stdout.splitlines() if t]
    rx = re.compile(re.escape(prefix) + r"(\d+)$")
    n = 0
    for tag in tags:
        m = rx.fullmatch(tag)
        if m:
            n = max(n, int(m.group(1)))
    return n


def main() -> None:
    root = _repo_root()
    cargo_path = os.path.join(root, "Cargo.toml")
    with open(cargo_path, "rb") as f:
        cfg = tomllib.load(f)

    package = cfg.get("package")
    if not isinstance(package, dict):
        print("error: missing [package] table in Cargo.toml", file=sys.stderr)
        sys.exit(2)

    version = str(package.get("version") or "").strip()
    if not version:
        print("error: missing [package].version in Cargo.toml", file=sys.stderr)
        sys.exit(2)

    md = package.get("metadata")
    release_md: dict = {}
    if isinstance(md, dict):
        entry = md.get("authdog-release")
        if isinstance(entry, dict):
            release_md = entry

    stable = bool(release_md.get("stable", False))

    _fetch_tags(root)

    if stable:
        tag = f"v{version}"
        print(tag)
        return

    n = _max_beta_suffix(root, version) + 1
    tag = f"v{version}-beta.{n}"
    print(tag)


if __name__ == "__main__":
    main()
