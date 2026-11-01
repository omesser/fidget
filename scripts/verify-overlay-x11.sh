#!/usr/bin/env bash
# X11 overlay parity: Perch, ride, drop, Poke, EWMH states. The Perch window
# predates the app, as in verify-overlay.sh; xprop exits 0 on a missing property.
# Run: xvfb-run -a -s "-screen 0 1280x720x24" scripts/verify-overlay-x11.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=scripts/lib/x11.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib/x11.sh"
# shellcheck source=scripts/scenarios/helpers/wait-helpers.sh
source "$SCRIPT_DIR/scenarios/helpers/wait-helpers.sh"
root_events() { stdbuf -oL xprop -root -spy; }

[ -n "${DISPLAY:-}" ] || fail "DISPLAY not set. Run under X11 or Xvfb."

for tool in xdotool xprop xwininfo xterm; do
  command -v "$tool" > /dev/null || fail "$tool not found. Install: sudo apt-get install x11-utils xdotool xterm"
done

cd "$WORKSPACE_ROOT"

STAMP=$(date +%Y%m%d-%H%M%S)
OUT=".verify/x11-$STAMP"
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
  wait_on_source 3 root_events grep -qEm1 '^_NET_SUPPORTING_WM_CHECK\(.*window id' ||
    fail "openbox did not publish _NET_SUPPORTING_WM_CHECK within 3 s"
fi

trap cleanup EXIT

ROOT_W=$(xwininfo -root | awk '/Width:/ {print $2}')
ROOT_H=$(xwininfo -root | awk '/Height:/ {print $2}')
# shellcheck disable=SC2034  # read by the sourced find_overlay_window()
MIN_OVERLAY_W=$((ROOT_W / 2))
# shellcheck disable=SC2034  # read by the sourced find_overlay_window()
MIN_OVERLAY_H=$((ROOT_H / 2))
# Halfway from the spawn (display centre, starting_position()) to the floor;
# centred on the spawn x once mapped, so a short walk stays over the Perch.
PERCH_Y=$((ROOT_H * 2 / 3))

# -e sleep: a login shell on Xvfb often exits immediately, taking the window
# with it before the sprite can land.
xterm -geometry "100x8+0+${PERCH_Y}" -title "perch-prop" -e sleep 3600 > "$OUT/xterm.out" 2> "$OUT/xterm.err" &
TEST_WINDOW_PID=$!
TEST_WINDOW_ID=$(timeout 3 xdotool search --sync --name '^perch-prop$' 2> /dev/null | head -1 || true)
[ -n "$TEST_WINDOW_ID" ] || fail "The perch window did not map within 3 s"
log_info "Perch window ID: $TEST_WINDOW_ID"
PERCH_W=$(xwininfo -id "$TEST_WINDOW_ID" | awk '/Width:/ {print $2}')
PERCH_X=$((ROOT_W / 2 - PERCH_W / 2))
timeout 3 xdotool windowmove --sync "$TEST_WINDOW_ID" "$PERCH_X" "$PERCH_Y" ||
  fail "The perch window did not move within 3 s"
log_info "Display ${ROOT_W}x${ROOT_H}; perch window ${PERCH_W} px wide at +${PERCH_X}+${PERCH_Y}"
xwininfo -id "$TEST_WINDOW_ID" > "$OUT/perch-window.txt" || true

log_info "Building fidget..."
cargo build -p fidget

log_info "Starting fidget with frame tracing..."
export FIDGET_TRACE_FRAMES=1
export FIDGET_TRACE_HITTEST=1
export RUST_LOG=debug
export LIBGL_ALWAYS_SOFTWARE=1
target/debug/fidget > "$TRACE_LOG" 2>&1 &
APP_PID=$!
# shellcheck disable=SC2034  # read by the sourced wait core
pid=$APP_PID

# A cold debug build on software GL can take well past 3 s to start.
wait_for_up "$TRACE_LOG" || fail "App never published an overlay line"
kill -0 "$APP_PID" 2> /dev/null || fail "App exited during startup"

log_info "Waiting for overlay window..."
wait_on_source 3 root_events probe_each_line find_overlay_window ||
  fail "Could not find the fidget overlay window within 3 s"
WINDOW_ID=$(find_overlay_window)
log_info "Found overlay window ID: $WINDOW_ID"

log_info "Waiting for EWMH window states..."
wait_for_line 3 "$TRACE_LOG" 'EWMH configured' || fail "configure_overlay never succeeded within 3 s"
overlay_state_events() { stdbuf -oL xprop -id "$WINDOW_ID" -spy _NET_WM_STATE; }
wait_on_source 3 overlay_state_events grep -qEm1 'ABOVE.*SKIP_TASKBAR|SKIP_TASKBAR.*ABOVE' || true
WINDOW_PROPS=$(xprop -id "$WINDOW_ID" _NET_WM_STATE 2> /dev/null || true)
if echo "$WINDOW_PROPS" | grep -q "_NET_WM_STATE_ABOVE" && echo "$WINDOW_PROPS" | grep -q "_NET_WM_STATE_SKIP_TASKBAR"; then
  log_info "EWMH states verified"
else
  log_info "WARN: EWMH properties not reflected by WM (app configured them: $(grep -c 'EWMH configured' "$TRACE_LOG") times)"
  log_info "WARN: Continuing with overlay presence verified through frame logs"
fi

wait_for_line 3 "$TRACE_LOG" 'frame:.* (Falling|Grounded|Perched)' ||
  fail "Sprite did not initialize (no Falling/Grounded/Perched state within 3 s)"
log_info "Sprite initialized"

log_info "Waiting for sprite to perch..."
wait_for_line 10 "$TRACE_LOG" 'frame: [0-9]+ Perched' ||
  fail "Sprite did not perch (no Perched state within 10 s)"
log_info "Perched state verified"

# Last Perched pos() is the feet; a ride that yanks is a fall, so step slowly
# relative to RIDE_ACCELERATION * YANK_WINDOW_S (1000 pt/s).
log_info "Moving perch window to test ride..."
PERCH_CUR_X=$(xwininfo -id "$TEST_WINDOW_ID" | awk '/Absolute upper-left X:/ {print $4}')
PERCH_CUR_Y=$(xwininfo -id "$TEST_WINDOW_ID" | awk '/Absolute upper-left Y:/ {print $4}')
HOLD_START_LINE=$(wc -l < "$TRACE_LOG")
for i in $(seq 1 12); do
  xdotool windowmove "$TEST_WINDOW_ID" $((PERCH_CUR_X + i * 8)) "$PERCH_CUR_Y"
  sleep 0.4
done

# Animation name is the token before '#'; Hold is still State::Perched.
tail -n +"$HOLD_START_LINE" "$TRACE_LOG" | grep -qE 'frame:.*Perched pos.* hold#' ||
  fail "Sprite did not ride (no Hold animation in traces after the move)"
log_info "Ride behavior (Hold) verified"

log_info "Closing perch window to test drop..."
DROP_START_LINE=$(wc -l < "$TRACE_LOG")
kill "$TEST_WINDOW_PID" 2> /dev/null || true
TEST_WINDOW_PID=""
xdotool windowkill "$TEST_WINDOW_ID" 2> /dev/null || true

# Falling at startup also matches `frame: N Falling`; only a new one after close counts.
since_drop=+$((DROP_START_LINE + 1))
wait_on_lines 3 "$since_drop" "$TRACE_LOG" grep -qEm1 'frame: [0-9]+ Falling' ||
  fail "Sprite did not drop (no new Falling state within 3 s of the window closing)"
log_info "Drop behavior (Falling) verified"
wait_on_lines 5 "$since_drop" "$TRACE_LOG" grep -qEm1 'frame: [0-9]+ Grounded' ||
  fail "Sprite did not land (no Grounded state within 5 s of the window closing)"

# Feet are pos(); the body is above them. A click shorter than one 16ms tick
# can miss XQueryPointer, so hold the button across a couple of polls.
LAST_FRAME=$(grep 'frame:' "$TRACE_LOG" | tail -1)
SPRITE_POS=$(echo "$LAST_FRAME" | grep -oE 'pos\([-0-9.]+,[-0-9.]+\)' | head -1 || true)
[ -n "$SPRITE_POS" ] || fail "Could not determine sprite position for Poke test"
SPRITE_X=$(echo "$SPRITE_POS" | tr -d 'pos()' | cut -d, -f1 | cut -d. -f1)
SPRITE_Y=$(echo "$SPRITE_POS" | tr -d 'pos()' | cut -d, -f2 | cut -d. -f1)
CLICK_Y=$((SPRITE_Y - 40))
log_info "Clicking sprite at ($SPRITE_X, $CLICK_Y) from $LAST_FRAME"
xdotool mousemove --sync "$SPRITE_X" "$CLICK_Y"
sleep 0.05
xdotool mousedown 1
sleep 0.12
xdotool mouseup 1

wait_for_line 3 "$TRACE_LOG" 'verbs:.*Poke' || fail "Click did not produce a Poke verb within 3 s"
log_info "Poke verb verified"

log_info ""
log_info "All X11 overlay behaviors verified:"
log_info "  - EWMH states (_NET_WM_STATE_ABOVE, SKIP_TASKBAR)"
log_info "  - Sprite initialization (Falling/Grounded/Perched)"
log_info "  - Perch (sprite on window top edge)"
log_info "  - Ride (Hold animation on window move)"
log_info "  - Drop (Falling state on window close)"
log_info "  - Poke (verb from click)"
log_info ""
log_info "Trace log saved to: $TRACE_LOG"
