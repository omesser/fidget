#!/usr/bin/env bash
# Tear down helper-owned processes and scratch. Never delete evidence.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
# shellcheck disable=SC1091
. "$SCRIPT_DIR/common.sh"
# shellcheck source=../../../../scripts/scenarios/helpers/wait-helpers.sh
# shellcheck disable=SC1091
. "$REPO_ROOT/scripts/scenarios/helpers/wait-helpers.sh"

# True when any of the pids still runs; a zombie has already exited.
any_running() { # <pid...>
  local p state
  for p in "$@"; do
    state=$(ps -o stat= -p "$p" 2> /dev/null) || continue
    [[ $state == Z* ]] || return 0
  done
  return 1
}

# TERM to Fidget's whole tree (its children hold the lock too), up to 3 s for
# all of them to exit, then kill -9 on what is left of that tree. A child that
# outlives Fidget gets it too, even when the wait could only watch Fidget's pid.
stop_pid() { # <pid>
  local tree
  tree=$(process_tree "$1")
  # shellcheck disable=SC2086
  kill $tree 2> /dev/null || true
  # shellcheck disable=SC2086
  wait_for_exit 3 "$1" "$FIDGET_VERIFY_LOCK" && ! any_running $tree && return 0
  echo "cleanup: kill -9 $1"
  # shellcheck disable=SC2086
  kill -9 $tree 2> /dev/null || true
  # SIGKILL lands asynchronously, and a launch right after takes the lock with -n.
  wait_for_exit 1 "$1" "$FIDGET_VERIFY_LOCK" ||
    echo "cleanup: $FIDGET_VERIFY_LOCK is still held 1 s after kill -9 $1; the next launch will fail"
}

PIDLIST="$FIDGET_VERIFY_SCRATCH/pids/owned.pids"
if [ -f "$PIDLIST" ]; then
  while read -r pid; do
    [ -n "$pid" ] || continue
    if kill -0 "$pid" 2> /dev/null; then
      echo "cleanup: kill $pid"
      stop_pid "$pid"
    fi
  done < "$PIDLIST"
fi

# Also clear single app.pid if present
if [ -f "$FIDGET_VERIFY_SCRATCH/pids/app.pid" ]; then
  pid="$(cat "$FIDGET_VERIFY_SCRATCH/pids/app.pid")"
  if kill -0 "$pid" 2> /dev/null; then
    echo "cleanup: kill app.pid $pid"
    stop_pid "$pid"
  fi
fi

rm -rf "$FIDGET_VERIFY_SCRATCH"
mkdir -p "$FIDGET_VERIFY_EVIDENCE"

echo "cleanup: scratch removed; evidence preserved at $FIDGET_VERIFY_EVIDENCE"
append_proof "cleanup done; evidence still at $FIDGET_VERIFY_EVIDENCE"
ls -la "$FIDGET_VERIFY_EVIDENCE"
test -d "$FIDGET_VERIFY_EVIDENCE"
