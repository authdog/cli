#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO=$(cd "$SCRIPT_DIR/.." && pwd)
cd "$REPO"

export RELEASE_FETCH_TAGS="${RELEASE_FETCH_TAGS:-1}"

TAG=$(python3 "$SCRIPT_DIR/compute_release_tag.py")

if ! git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  printf '%s\n' "Missing local tag ${TAG}; run \"make tag\" first." >&2
  exit 1
fi

git push origin "${TAG}"
