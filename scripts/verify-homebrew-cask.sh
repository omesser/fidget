#!/usr/bin/env bash
# Download the cask's Release disk image and check the checksum, app,
# bundle id, and that livecheck's latest release is the pinned tag.
# No Homebrew required. macOS with brew also runs brew style.
# Usage: scripts/verify-homebrew-cask.sh

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

ruby scripts/homebrew-cask.rb verify

if command -v brew > /dev/null; then
  brew style packaging/homebrew/Casks/fidget.rb
  if [ "$(uname -s)" = Darwin ]; then
    brew audit --cask --online packaging/homebrew/Casks/fidget.rb
  else
    echo "brew audit --cask skipped: Homebrew installs casks on macOS"
  fi
fi
