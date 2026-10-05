#!/usr/bin/env bash
# Build if needed and start a traced fidget owned by this RUN_ID.
# Prefer drive-overlay-* when proving overlay features — those scripts own lifecycle.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
# shellcheck disable=SC1091
. "$SCRIPT_DIR/common.sh"

cd "$REPO_ROOT"

if ! fidget_bin > /dev/null; then
  echo "launch: building debug…"
  cargo build -p fidget
fi
BIN="$(fidget_bin)"

LOG="$FIDGET_VERIFY_SCRATCH/app.log"
PIDFILE="$FIDGET_VERIFY_SCRATCH/pids/app.pid"

if [ -f "$PIDFILE" ]; then
  old="$(cat "$PIDFILE")"
  if kill -0 "$old" 2> /dev/null; then
    echo "launch: already running pid $old (log $LOG)"
    export APP_PID="$old"
    exit 0
  fi
fi

export FIDGET_TRACE_FRAMES="${FIDGET_TRACE_FRAMES:-1}"
export FIDGET_TRACE_HITTEST="${FIDGET_TRACE_HITTEST:-1}"
export FIDGET_CAPTURABLE="${FIDGET_CAPTURABLE:-1}"

echo "launch: $BIN → $LOG"
"$BIN" > "$LOG" 2>&1 &
APP_PID=$!
echo "$APP_PID" > "$PIDFILE"
record_pid "$APP_PID"
export APP_PID

for _ in $(seq 1 80); do
  if grep -qE '^overlay:' "$LOG" 2> /dev/null; then
    echo "launch: ready (overlay line) pid=$APP_PID"
    append_proof "launch ready pid=$APP_PID log=$LOG"
    exit 0
  fi
  if ! kill -0 "$APP_PID" 2> /dev/null; then
    echo "launch: process exited"
    tail -40 "$LOG" || true
    append_proof "launch FAILED — process exited; see $LOG"
    exit 1
  fi
  sleep 0.25
done

echo "launch: timed out waiting for overlay: (process still up)"
append_proof "launch WARN — no overlay: yet; pid=$APP_PID log=$LOG"
exit 0
