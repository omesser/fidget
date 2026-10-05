#!/usr/bin/env bash
# Shared paths for verify-fidget helpers. Source from repo-relative helpers.
set -euo pipefail

_helpers_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIDGET_SKILL_ROOT="$(cd "$_helpers_dir/.." && pwd)"
REPO_ROOT="$(cd "$_helpers_dir/../../../.." && pwd)"
# helpers -> verify-fidget -> skills -> .agents -> repo
# From helpers: .. = skill, ../.. = skills, ../../.. = .agents, ../../../.. = repo
if [ ! -f "$REPO_ROOT/Cargo.toml" ] || [ ! -d "$REPO_ROOT/src-tauri" ]; then
  # Fallback: walk up looking for workspace
  REPO_ROOT="$(cd "$_helpers_dir" && while [ ! -f Cargo.toml ] || [ ! -d src-tauri ]; do
    cd .. || exit 1
    [ "$PWD" = "/" ] && exit 1
  done && pwd)"
fi

export REPO_ROOT
export FIDGET_SKILL_ROOT

export RUN_ID="${RUN_ID:-$(date +%Y%m%d-%H%M%S)-$$}"
export FIDGET_VERIFY_ROOT="${FIDGET_VERIFY_ROOT:-/tmp/fidget-verify-$RUN_ID}"
export FIDGET_VERIFY_EVIDENCE="${FIDGET_VERIFY_EVIDENCE:-$FIDGET_VERIFY_ROOT/evidence}"
export FIDGET_VERIFY_SCRATCH="${FIDGET_VERIFY_SCRATCH:-$FIDGET_VERIFY_ROOT/scratch}"

mkdir -p "$FIDGET_VERIFY_EVIDENCE" "$FIDGET_VERIFY_SCRATCH/pids"

# Debug only: a release build sends stderr only to <data dir>/fidget/process.log
# (#1325), so its terminal log never shows the `overlay:` or `frame:` lines.
fidget_bin() {
  if [ -x "$REPO_ROOT/target/debug/fidget" ]; then
    echo "$REPO_ROOT/target/debug/fidget"
  else
    return 1
  fi
}

record_pid() {
  local pid="$1"
  echo "$pid" >> "$FIDGET_VERIFY_SCRATCH/pids/owned.pids"
}

append_proof() {
  local line="$1"
  mkdir -p "$FIDGET_VERIFY_EVIDENCE"
  {
    echo "## $(date -u +%Y-%m-%dT%H:%M:%SZ) UTC"
    echo "$line"
    echo
  } >> "$FIDGET_VERIFY_EVIDENCE/PROOF.md"
}
