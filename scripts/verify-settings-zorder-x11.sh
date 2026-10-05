#!/usr/bin/env bash
# Prove the Settings webview stacks above the X11 overlay (#799).
# Run with: xvfb-run -a -s "-screen 0 1280x720x24" scripts/verify-settings-zorder-x11.sh
# FIDGET_VERIFY_BIN=path/to/fidget overrides the binary.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=scripts/lib/x11.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/x11.sh"

[ -n "${DISPLAY:-}" ] || fail "DISPLAY not set. Run under X11 or Xvfb."

if [ -z "${XAUTHORITY:-}" ] && [ -f "$HOME/.Xauthority" ]; then
  export XAUTHORITY="$HOME/.Xauthority"
fi

for tool in xdotool xprop xwininfo; do
  command -v "$tool" > /dev/null || fail "$tool not found. Install: sudo apt-get install x11-utils xdotool"
done

cd "$WORKSPACE_ROOT"

STAMP=$(date +%Y%m%d-%H%M%S)
OUT=".verify/settings-zorder-x11-$STAMP"
mkdir -p "$OUT"
TRACE_LOG="$OUT/app.log"

WM_STARTED=0
if ! has_supporting_wm; then
  command -v openbox > /dev/null || fail "openbox not found. Install: sudo apt-get install openbox"
  log_info "Starting openbox (Xvfb has no window manager)..."
  openbox --replace > /dev/null 2> "$OUT/openbox.err" &
  # shellcheck disable=SC2034  # read by the sourced cleanup()
  WM_PID=$!
  # shellcheck disable=SC2034  # read by the sourced cleanup()
  WM_STARTED=1
  for _ in $(seq 1 40); do
    has_supporting_wm && break
    sleep 0.25
  done
  has_supporting_wm || fail "openbox did not publish _NET_SUPPORTING_WM_CHECK"
fi

trap cleanup EXIT

BIN="${FIDGET_VERIFY_BIN:-}"
if [ -z "$BIN" ]; then
  if [ -x "$WORKSPACE_ROOT/target/debug/fidget" ]; then
    BIN="$WORKSPACE_ROOT/target/debug/fidget"
  else
    log_info "Building fidget (debug)..."
    cargo build -p fidget
    BIN="$WORKSPACE_ROOT/target/debug/fidget"
  fi
fi
[ -x "$BIN" ] || fail "no binary at $BIN"

log_info "Starting fidget with Settings webview..."
HOME_DIR="$OUT/home"
mkdir -p "$HOME_DIR"
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"
# This script asserts X11 EWMH stacking. Prefer the X11 GDK backend even when
# WAYLAND_DISPLAY is set, so tao's handle is an X window.
export GDK_BACKEND="${GDK_BACKEND:-x11}"
env -u FIDGET_DIRECTOR_API_KEY \
  HOME="$HOME_DIR" \
  FIDGET_OPEN_SETTINGS=1 \
  FIDGET_TRACE_FRAMES=1 \
  FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=timber-wolf \
  FIDGET_CHARACTERS="${FIDGET_CHARACTERS:-$WORKSPACE_ROOT/characters}" \
  GDK_BACKEND="$GDK_BACKEND" \
  LIBGL_ALWAYS_SOFTWARE="$LIBGL_ALWAYS_SOFTWARE" \
  "$BIN" > "$TRACE_LOG" 2>&1 &
APP_PID=$!

await "$TRACE_LOG" '^overlay:' 80 || fail "App never published an overlay line"
kill -0 "$APP_PID" 2> /dev/null || fail "App exited during startup"

ROOT_W=$(xwininfo -root | awk '/Width:/ {print $2}')
ROOT_H=$(xwininfo -root | awk '/Height:/ {print $2}')
# shellcheck disable=SC2034  # read by the sourced find_overlay_window()
MIN_OVERLAY_W=$((ROOT_W / 2))
# shellcheck disable=SC2034  # read by the sourced find_overlay_window()
MIN_OVERLAY_H=$((ROOT_H / 2))

find_settings_window() {
  local id name
  for id in $(xdotool search --name 'Settings' 2> /dev/null || true); do
    name=$(xprop -id "$id" WM_NAME 2> /dev/null || true)
    echo "$name" | grep -q 'Settings' && echo "$id" && return 0
  done
  return 1
}

log_info "Waiting for overlay window..."
OVERLAY_ID=""
for _ in $(seq 1 60); do
  OVERLAY_ID=$(find_overlay_window || true)
  [ -n "$OVERLAY_ID" ] && break
  sleep 0.25
done
[ -n "$OVERLAY_ID" ] || fail "Could not find fidget overlay window"
log_info "Found overlay window ID: $OVERLAY_ID"

log_info "Waiting for overlay EWMH ABOVE..."
OVERLAY_PROPS=""
for _ in $(seq 1 80); do
  OVERLAY_PROPS=$(xprop -id "$OVERLAY_ID" _NET_WM_STATE 2> /dev/null || true)
  echo "$OVERLAY_PROPS" | grep -q "_NET_WM_STATE_ABOVE" && break
  sleep 0.25
done
echo "$OVERLAY_PROPS" | grep -q "_NET_WM_STATE_ABOVE" ||
  fail "overlay missing _NET_WM_STATE_ABOVE (${OVERLAY_PROPS:-empty})"

log_info "Waiting for Settings window..."
SETTINGS_ID=""
for _ in $(seq 1 60); do
  SETTINGS_ID=$(find_settings_window || true)
  [ -n "$SETTINGS_ID" ] && break
  sleep 0.25
done
[ -n "$SETTINGS_ID" ] || fail "Could not find Settings window"
log_info "Found Settings window ID: $SETTINGS_ID"

log_info "Waiting for Settings _NET_WM_STATE_ABOVE..."
SETTINGS_PROPS=""
for _ in $(seq 1 40); do
  SETTINGS_PROPS=$(xprop -id "$SETTINGS_ID" _NET_WM_STATE 2> /dev/null || true)
  echo "$SETTINGS_PROPS" | grep -q "_NET_WM_STATE_ABOVE" && break
  sleep 0.25
done
echo "$SETTINGS_PROPS" | grep -q "_NET_WM_STATE_ABOVE" ||
  fail "Settings missing _NET_WM_STATE_ABOVE (${SETTINGS_PROPS:-empty})"
log_info "Settings is in the ABOVE band: $SETTINGS_PROPS"

stacking_ids() {
  xprop -root _NET_CLIENT_LIST_STACKING 2> /dev/null |
    sed 's/.*#//' |
    tr ',' '\n' |
    sed 's/^ *//;s/ *$//' |
    sed '/^$/d' |
    while read -r hex; do
      printf '%d\n' "$hex" 2> /dev/null || true
    done
}

log_info "Checking stacking order..."
STACK_OK=0
for _ in $(seq 1 40); do
  STACK=$(stacking_ids)
  OVERLAY_POS=$(echo "$STACK" | grep -n "^${OVERLAY_ID}$" | head -1 | cut -d: -f1 || true)
  SETTINGS_POS=$(echo "$STACK" | grep -n "^${SETTINGS_ID}$" | head -1 | cut -d: -f1 || true)
  if [ -n "$OVERLAY_POS" ] && [ -n "$SETTINGS_POS" ] && [ "$SETTINGS_POS" -gt "$OVERLAY_POS" ]; then
    STACK_OK=1
    break
  fi
  sleep 0.25
done

if [ "$STACK_OK" -ne 1 ]; then
  log_error "stacking overlay=$OVERLAY_ID pos=${OVERLAY_POS:-missing} settings=$SETTINGS_ID pos=${SETTINGS_POS:-missing}"
  log_error "stack:"
  stacking_ids >&2 || true
  xprop -root _NET_CLIENT_LIST_STACKING >&2 || true
  fail "Settings is not above the overlay in _NET_CLIENT_LIST_STACKING"
fi

log_info "Settings stacks above overlay (positions $OVERLAY_POS < $SETTINGS_POS)"
log_info "PASS: Settings webview is above the ABOVE overlay"
log_info "Log: $TRACE_LOG"
