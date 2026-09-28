#!/usr/bin/env python3
"""Compute the next release git tag from release.toml (version + stable)."""

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
    """Largest `-beta.{n}` for this base across bare and legacy `v`-prefixed tags."""
    legacy_prefix = f"v{base}-beta."
    prefix = f"{base}-beta."

    tags: list[str] = []
    for pat in (f"{prefix}*", f"{legacy_prefix}*"):
        proc = subprocess.run(
            ["git", "-C", root, "tag", "-l", pat],
            text=True,
            stdout=subprocess.PIPE,
            check=True,
        )
        tags.extend(t for t in proc.stdout.splitlines() if t)

    def max_for(p: str) -> int:
        rx = re.compile(re.escape(p) + r"(\d+)$")
        n = 0
        for tag in tags:
            m = rx.fullmatch(tag)
            if m:
                n = max(n, int(m.group(1)))
        return n

    return max(max_for(prefix), max_for(legacy_prefix))


def main() -> None:
    root = _repo_root()
    release_path = os.path.join(root, "release.toml")
    with open(release_path, "rb") as f:
        cfg = tomllib.load(f)

    version = str(cfg.get("version") or "").strip()
    if not version:
        print("error: missing version in release.toml", file=sys.stderr)
        sys.exit(2)

    stable = bool(cfg.get("stable", False))

    _fetch_tags(root)

    if stable:
        tag = version
        print(tag)
        return

    n = _max_beta_suffix(root, version) + 1
    tag = f"{version}-beta.{n}"
    print(tag)


if __name__ == "__main__":
    main()
