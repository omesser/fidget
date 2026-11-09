#!/usr/bin/env bash
# Set the workspace's crate version to a tag's, and the lockfile with it. Run
# from the repository root.

set -euo pipefail

tag="${1:-}"
[[ $tag =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]] || {
  echo "usage: scripts/bump-crate-versions.sh <tag, such as v1.2.3 or v1.2.3-dev.gabc1234>" >&2
  exit 2
}
version="${tag#v}"

# A Windows checkout can be CRLF, which the line matches below would miss.
tr -d '\r' < Cargo.toml > Cargo.toml.lf
mv Cargo.toml.lf Cargo.toml

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
