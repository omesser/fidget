#!/usr/bin/env bash
# Point the Homebrew cask at a GitHub Release tag's Apple Silicon disk image.
# Usage: scripts/bump-homebrew-cask.sh v0.0.1-dev
#        scripts/bump-homebrew-cask.sh --check v0.0.1-dev

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if ! command -v ruby > /dev/null; then
  echo "ruby not found. Install: sudo apt-get install ruby" >&2
  exit 1
fi
if ! command -v 7z > /dev/null && ! command -v 7zz > /dev/null; then
  echo "7z not found. Install: sudo apt-get install p7zip-full" >&2
  exit 1
fi

if [ "${1:-}" = "--check" ]; then
  if [ -z "${2:-}" ]; then
    echo "usage: scripts/bump-homebrew-cask.sh --check <tag>" >&2
    exit 2
  fi
  exec ruby scripts/homebrew-cask.rb bump --check "$2"
fi

if [ -z "${1:-}" ]; then
  echo "usage: scripts/bump-homebrew-cask.sh <tag>" >&2
  exit 2
fi
exec ruby scripts/homebrew-cask.rb bump "$1"
