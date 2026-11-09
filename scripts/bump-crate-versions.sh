#!/usr/bin/env bash
# Set the workspace's crate version to a stable Release tag's, and the lockfile
# with it. Run from the repository root.

set -euo pipefail

tag="${1:-}"
[[ $tag =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || {
  echo "usage: scripts/bump-crate-versions.sh <stable tag, such as v1.2.3>" >&2
  exit 2
}
version="${tag#v}"

current="$(awk -F'"' '
  /^\[/ { in_package = ($0 == "[workspace.package]") }
  in_package && /^version = / { print $2; exit }
' Cargo.toml)"
[ -n "$current" ] || {
  echo "Cargo.toml has no [workspace.package] version" >&2
  exit 1
}

awk -v version="$version" '
  /^\[/ { in_package = ($0 == "[workspace.package]") }
  in_package && /^version = / { $0 = "version = \"" version "\"" }
  { print }
' Cargo.toml > Cargo.toml.bumped
mv Cargo.toml.bumped Cargo.toml
cargo update --workspace --quiet
echo "crates set to ${version}, from ${current}"
