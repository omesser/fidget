#!/usr/bin/env bash
set -euo pipefail

# Offline coverage only: every `uses:` has a lockfile entry. The Actions
# Lockfile workflow runs `--verify`, which also checks each pin upstream.
if ! command -v gh > /dev/null 2>&1; then
  echo "skipped - gh not installed" >&2
  exit 0
fi
if ! gh actions-lock --version > /dev/null 2>&1; then
  echo "skipped - gh-actions-lock not installed (gh extension install github/gh-actions-lock)" >&2
  exit 0
fi

if ! gh actions-lock --verify-local; then
  echo "Run \`gh actions-lock\` and commit .github/workflows/actions.lock." >&2
  exit 1
fi
