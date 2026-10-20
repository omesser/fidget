# shellcheck shell=bash
# Shared helpers for the X11 verify scripts. Callers already set
# `set -euo pipefail`, so this file does not.

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

log_info() {
  echo -e "${GREEN}[INFO]${NC} $*"
}

log_error() {
  echo -e "${RED}[ERROR]${NC} $*"
}

# Superset of callers' failure diagnostics. SETTINGS_ID / CHAT_ID / OVERLAY_ID
# are unset in scripts that never find those windows, so those blocks no-op.
fail() {
  log_error "$@"
  if [ -n "${TRACE_LOG:-}" ] && [ -f "$TRACE_LOG" ]; then
    log_error "Last 40 lines of app log:"
    tail -40 "$TRACE_LOG" >&2 || true
  fi
  if [ -n "${SETTINGS_ID:-}" ]; then
    log_error "Settings xprop:"
    xprop -id "$SETTINGS_ID" WM_NAME _NET_WM_STATE WM_CLASS 2>&1 | head -20 >&2 || true
  fi
  if [ -n "${CHAT_ID:-}" ]; then
    log_error "Chat xprop:"
    xprop -id "$CHAT_ID" WM_NAME _NET_WM_STATE WM_CLASS 2>&1 | head -20 >&2 || true
  fi
  if [ -n "${OVERLAY_ID:-}" ]; then
    log_error "Overlay xprop:"
    xprop -id "$OVERLAY_ID" WM_NAME _NET_WM_STATE WM_CLASS 2>&1 | head -20 >&2 || true
  fi
  xprop -root _NET_CLIENT_LIST_STACKING 2>&1 >&2 || true
  exit 1
}

# $1=file  $2=grep -E pattern  $3=attempts of a quarter-second each
await() {
  local file="$1" pattern="$2" attempts="$3"
  for _ in $(seq 1 "$attempts"); do
    grep -qE "$pattern" "$file" 2> /dev/null && return 0
    sleep 0.25
  done
  return 1
}

has_supporting_wm() {
  xprop -root _NET_SUPPORTING_WM_CHECK 2> /dev/null | grep -q 'window id'
}

# Superset of both callers' teardown. TEST_WINDOW_PID is unset in
# verify-settings-zorder-x11.sh, so that block no-ops there.
cleanup() {
  log_info "Cleaning up..."
  if [ -n "${APP_PID:-}" ]; then
    kill "$APP_PID" 2> /dev/null || true
  fi
  if [ -n "${TEST_WINDOW_PID:-}" ]; then
    kill "$TEST_WINDOW_PID" 2> /dev/null || true
  fi
  if [ "$WM_STARTED" -eq 1 ] && [ -n "${WM_PID:-}" ]; then
    kill "$WM_PID" 2> /dev/null || true
  fi
}

# GDK leaves 10x10 placeholders, and ~200x200 ones with a Settings webview, under
# the app's WM_CLASS. Requiring half the display holds for a display-sized
# overlay. Needs MIN_OVERLAY_W/H set first.
find_overlay_window() {
  local id w h name
  for id in $(xdotool search --class 'Fidget' 2> /dev/null || true); do
    name=$(xprop -id "$id" WM_NAME 2> /dev/null || true)
    echo "$name" | grep -q 'Settings' && continue
    w=$(xwininfo -id "$id" 2> /dev/null | awk '/^  Width:/ {print $2; exit}')
    h=$(xwininfo -id "$id" 2> /dev/null | awk '/^  Height:/ {print $2; exit}')
    if [ -n "$w" ] && [ -n "$h" ] && [ "$w" -ge "$MIN_OVERLAY_W" ] && [ "$h" -ge "$MIN_OVERLAY_H" ]; then
      echo "$id"
      return 0
    fi
  done
  return 1
}
