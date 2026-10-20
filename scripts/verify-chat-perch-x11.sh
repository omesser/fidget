#!/usr/bin/env bash
# Prove focused Chat stays perch-eligible under the X11 overlay. Chat must not
# take _NET_WM_STATE_ABOVE (layer 1); perch_eligible keeps layer 0. Run it under
# xvfb-run; FIDGET_VERIFY_BIN overrides the binary.

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
OUT=".verify/chat-perch-x11-$STAMP"
mkdir -p "$OUT"
TRACE_LOG="$OUT/app.log"
RESULT_LOG="$OUT/result.txt"
: > "$RESULT_LOG"

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

CHAT_TITLE="${FIDGET_CHAT_TITLE:-Timber Wolf}"
CHARACTER="${FIDGET_CHARACTER:-timber-wolf}"

log_info "Starting fidget with Chat open..."
HOME_DIR="$OUT/home"
mkdir -p "$HOME_DIR"
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"
export GDK_BACKEND="${GDK_BACKEND:-x11}"
env -u FIDGET_DIRECTOR_API_KEY \
  HOME="$HOME_DIR" \
  FIDGET_OPEN_CHAT=1 \
  FIDGET_DEBUG_IPC=1 \
  FIDGET_TRACE_FRAMES=1 \
  FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER="$CHARACTER" \
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

find_chat_window() {
  local id name w h
  for id in $(xdotool search --class 'Fidget' 2> /dev/null || true); do
    name=$(xprop -id "$id" WM_NAME 2> /dev/null || true)
    echo "$name" | grep -q 'Settings' && continue
    echo "$name" | grep -qF "$CHAT_TITLE" || continue
    w=$(xwininfo -id "$id" 2> /dev/null | awk '/^  Width:/ {print $2; exit}')
    h=$(xwininfo -id "$id" 2> /dev/null | awk '/^  Height:/ {print $2; exit}')
    # Chat is ~420x560; skip the display-sized overlay and GDK placeholders.
    if [ -n "$w" ] && [ -n "$h" ] && [ "$w" -ge 300 ] && [ "$w" -lt "$MIN_OVERLAY_W" ] &&
      [ "$h" -ge 300 ] && [ "$h" -lt "$ROOT_H" ]; then
      echo "$id"
      return 0
    fi
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

log_info "Waiting for Chat window titled '$CHAT_TITLE'..."
CHAT_ID=""
for _ in $(seq 1 80); do
  CHAT_ID=$(find_chat_window || true)
  [ -n "$CHAT_ID" ] && break
  sleep 0.25
done
[ -n "$CHAT_ID" ] || fail "Could not find Chat window titled $CHAT_TITLE"
log_info "Found Chat window ID: $CHAT_ID"
xprop -id "$CHAT_ID" WM_NAME _NET_WM_STATE WM_CLASS > "$OUT/chat-xprop.txt" || true

log_info "Asserting Chat has no _NET_WM_STATE_ABOVE..."
CHAT_PROPS=$(xprop -id "$CHAT_ID" _NET_WM_STATE 2> /dev/null || true)
echo "$CHAT_PROPS" > "$OUT/chat-net-wm-state.txt"
if echo "$CHAT_PROPS" | grep -q "_NET_WM_STATE_ABOVE"; then
  fail "Chat has _NET_WM_STATE_ABOVE (${CHAT_PROPS:-empty}); not perch-eligible"
fi
log_info "Chat _NET_WM_STATE has no ABOVE: ${CHAT_PROPS:-not set}"
echo "no_above=pass" >> "$RESULT_LOG"

# X11 layer is 0/1 from is_above. No ABOVE means layer 0, which perch_eligible keeps.
echo "perch_layer_seam=pass (no ABOVE => layer 0 => perch_eligible)" >> "$RESULT_LOG"

log_info "Focusing Chat..."
xdotool windowactivate --sync "$CHAT_ID" 2> /dev/null || true
sleep 0.5
CHAT_PROPS=$(xprop -id "$CHAT_ID" _NET_WM_STATE 2> /dev/null || true)
if echo "$CHAT_PROPS" | grep -q "_NET_WM_STATE_ABOVE"; then
  fail "focused Chat gained _NET_WM_STATE_ABOVE (${CHAT_PROPS})"
fi
log_info "Focused Chat still has no ABOVE"
echo "focused_no_above=pass" >> "$RESULT_LOG"

chat_geom() {
  CHAT_X=$(xwininfo -id "$CHAT_ID" | awk '/Absolute upper-left X:/ {print $4; exit}')
  CHAT_Y=$(xwininfo -id "$CHAT_ID" | awk '/Absolute upper-left Y:/ {print $4; exit}')
  CHAT_W=$(xwininfo -id "$CHAT_ID" | awk '/^  Width:/ {print $2; exit}')
  CHAT_H=$(xwininfo -id "$CHAT_ID" | awk '/^  Height:/ {print $2; exit}')
  # Engine ledge is outer frame top: client Absolute minus _NET_FRAME_EXTENTS top.
  FRAME_TOP=$(xprop -id "$CHAT_ID" _NET_FRAME_EXTENTS 2> /dev/null |
    sed -n 's/.* = //p' | awk -F', *' '{print $3}')
  FRAME_TOP=${FRAME_TOP:-0}
  CHAT_LEDGE_Y=$((CHAT_Y - FRAME_TOP))
}

chat_geom
if [ -z "$CHAT_X" ] || [ -z "$CHAT_Y" ] || [ -z "$CHAT_W" ] || [ -z "$CHAT_H" ]; then
  fail "Could not read Chat geometry"
fi

WIN_TYPE=$(xprop -id "$CHAT_ID" _NET_WM_WINDOW_TYPE 2> /dev/null || true)
{
  echo "chat_id=$CHAT_ID"
  xprop -id "$CHAT_ID" WM_NAME _NET_WM_WINDOW_TYPE _NET_WM_STATE _NET_FRAME_EXTENTS
  xprop -root _NET_CLIENT_LIST
} > "$OUT/chat-perch-seam.txt"
echo "$WIN_TYPE" | grep -q "_NET_WM_WINDOW_TYPE_NORMAL" ||
  fail "Chat is not _NET_WM_WINDOW_TYPE_NORMAL (${WIN_TYPE:-empty})"
xprop -root _NET_CLIENT_LIST 2> /dev/null | grep -qi "$(printf '0x%x' "$CHAT_ID")" ||
  fail "Chat $CHAT_ID missing from _NET_CLIENT_LIST"
echo "client_list=pass" >> "$RESULT_LOG"
echo "window_type_normal=pass" >> "$RESULT_LOG"

# Summon opens Chat near the top. Art clearance rejects a ledge that high on a
# short Xvfb display, so slide Chat down before asking the sprite to land.
TARGET_CLIENT_Y=$((ROOT_H * 2 / 5))
if [ "$CHAT_LEDGE_Y" -lt "$TARGET_CLIENT_Y" ]; then
  log_info "Moving Chat down for clearance (ledge $CHAT_LEDGE_Y -> client y $TARGET_CLIENT_Y)..."
  xdotool windowmove "$CHAT_ID" "$CHAT_X" "$TARGET_CLIENT_Y"
  for _ in $(seq 1 20); do
    chat_geom
    [ "$CHAT_Y" -ge $((TARGET_CLIENT_Y - 2)) ] && break
    sleep 0.1
  done
fi
log_info "Chat client +${CHAT_X}+${CHAT_Y} ${CHAT_W}x${CHAT_H}; frame top=$FRAME_TOP; ledge y=$CHAT_LEDGE_Y"

# Feet on the Chat ledge (outer top edge, horizontal centre).
PLACE_X=$((CHAT_X + CHAT_W / 2))
PLACE_Y=$CHAT_LEDGE_Y
log_info "Placing Character on Chat ledge at ($PLACE_X, $PLACE_Y)..."
CMD_PATH="$HOME_DIR/.fidget-debug-cmd"
RES_PATH="$HOME_DIR/.fidget-debug-result"
rm -f "$RES_PATH"
printf 'place x=%s y=%s\n' "$PLACE_X" "$PLACE_Y" > "$CMD_PATH"
PLACE_OK=0
for _ in $(seq 1 40); do
  if [ -f "$RES_PATH" ]; then
    PLACE_REPLY=$(cat "$RES_PATH" 2> /dev/null || true)
    echo "$PLACE_REPLY" > "$OUT/place-reply.txt"
    echo "$PLACE_REPLY" | grep -q '^placed:' && PLACE_OK=1 && break
    echo "$PLACE_REPLY" | grep -q '^error:' && fail "debug place failed: $PLACE_REPLY"
  fi
  sleep 0.25
done
[ "$PLACE_OK" -eq 1 ] || fail "debug place timed out (is FIDGET_DEBUG_IPC on?)"
log_info "Placed: $(cat "$OUT/place-reply.txt")"

# stand_at leaves State::Grounded. Perched is the land-from-fall path. Holding
# the ledge y (not falling to the floor) is the Engine proof Chat is a Perch.
log_info "Waiting for feet to hold Chat ledge y=$PLACE_Y..."
HOLD_OK=0
# Frame logs omit the space after the comma: pos(109,288).
LEDGE_RE="frame: [0-9]+ (Grounded|Perched) pos\\([0-9]+, ?${PLACE_Y}\\)"
for _ in $(seq 1 80); do
  if grep -qE "$LEDGE_RE" "$TRACE_LOG" 2> /dev/null; then
    HOLD_OK=1
    break
  fi
  sleep 0.25
done
if [ "$HOLD_OK" -eq 1 ]; then
  log_info "Feet held Chat ledge y=$PLACE_Y (Chat is in the Perch list)"
  echo "ledge_hold=pass y=$PLACE_Y" >> "$RESULT_LOG"
  grep -E "$LEDGE_RE" "$TRACE_LOG" | tail -5 > "$OUT/ledge-frames.txt" || true
else
  fail "Sprite did not hold Chat ledge y=$PLACE_Y; Chat may be missing from the Perch list"
fi

BESIDE_X=$((CHAT_X - 80))
if [ "$BESIDE_X" -lt 40 ]; then
  BESIDE_X=$((CHAT_X + CHAT_W + 80))
fi
BESIDE_Y=$((CHAT_Y + CHAT_H / 2))
rm -f "$RES_PATH"
printf 'place x=%s y=%s\n' "$BESIDE_X" "$BESIDE_Y" > "$CMD_PATH"
for _ in $(seq 1 40); do
  [ -f "$RES_PATH" ] && grep -q '^placed:' "$RES_PATH" 2> /dev/null && break
  sleep 0.25
done
log_info "Character beside Chat at ($BESIDE_X, $BESIDE_Y)"

# Composer sits near the bottom of the visible Chat band. Raise nothing first:
# the click itself must deliver focus if the overlay lets it through.
chat_geom
COMPOSER_X=$((CHAT_X + CHAT_W / 2))
VISIBLE_BOTTOM=$((CHAT_Y + CHAT_H))
if [ "$VISIBLE_BOTTOM" -gt "$ROOT_H" ]; then
  VISIBLE_BOTTOM=$ROOT_H
fi
COMPOSER_Y=$((VISIBLE_BOTTOM - 40))
if [ "$COMPOSER_Y" -le "$CHAT_Y" ]; then
  COMPOSER_Y=$((CHAT_Y + 40))
fi
log_info "Clicking composer at ($COMPOSER_X, $COMPOSER_Y) with Character beside..."
# Defocus Chat so a no-op click cannot inherit prior focus.
xdotool windowactivate --sync "$OVERLAY_ID" 2> /dev/null || true
sleep 0.25
xdotool mousemove --sync "$COMPOSER_X" "$COMPOSER_Y" click 1
sleep 0.5

FOCUS_ID=$(xdotool getwindowfocus 2> /dev/null || true)
ACTIVE_RAW=$(xprop -root _NET_ACTIVE_WINDOW 2> /dev/null || true)
CHAT_HEX=$(printf '0x%x' "$CHAT_ID")
{
  echo "focus_id=$FOCUS_ID"
  echo "active=$ACTIVE_RAW"
  echo "chat_id=$CHAT_ID"
  echo "chat_hex=$CHAT_HEX"
  echo "click=$COMPOSER_X,$COMPOSER_Y"
} > "$OUT/click-focus.txt"

CLICK_OK=0
if [ -n "$FOCUS_ID" ] && [ "$FOCUS_ID" = "$CHAT_ID" ]; then
  CLICK_OK=1
fi
if echo "$ACTIVE_RAW" | grep -qi "$CHAT_HEX"; then
  CLICK_OK=1
fi

if [ "$CLICK_OK" -eq 1 ]; then
  log_info "Composer click: focus reached Chat"
  echo "composer_click=pass" >> "$RESULT_LOG"
else
  log_info "Composer click: focus is not Chat (focus=$FOCUS_ID active=$ACTIVE_RAW)"
  log_info "Recording as fail for the known under-overlay trade; Hard perch asserts still passed"
  echo "composer_click=fail focus=$FOCUS_ID active=$ACTIVE_RAW" >> "$RESULT_LOG"
fi

log_info "PASS: focused Chat has no ABOVE and is a Perch"
log_info "Result: $RESULT_LOG"
log_info "Log: $TRACE_LOG"
cat "$RESULT_LOG"
