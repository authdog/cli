#!/usr/bin/env python3
"""Ensure the pushed/dispatched git tag matches version in release.toml (stable or -beta.<n>)."""

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
    if raw.startswith("v"):
        print(
            "error: release tag must not use a leading 'v' "
            f"(got {raw!r}; expected bare semver)",
            file=sys.stderr,
        )
        sys.exit(2)

    body = raw

    release_path = os.path.join(os.path.dirname(__file__), "..", "release.toml")
    release_path = os.path.normpath(release_path)
    with open(release_path, "rb") as f:
        cfg = tomllib.load(f)

    version = str(cfg.get("version") or "").strip()
    if not version:
        print("error: missing version in release.toml", file=sys.stderr)
        sys.exit(2)

    beta = re.fullmatch(re.escape(version) + r"-beta\.\d+", body)
    if beta:
        print(f"ok: beta tag {raw} matches release.toml version {version}")
        return

    if body == version:
        print(f"ok: release tag {raw} matches release.toml version {version}")
        return

    print(
        "error: tag does not match release.toml "
        f"version={version!r}: got {body!r} "
        f"(expect {version} or {version}-beta.<n>)",
        file=sys.stderr,
    )
    sys.exit(1)


if __name__ == "__main__":
    main()
