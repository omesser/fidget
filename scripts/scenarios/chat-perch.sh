#!/usr/bin/env bash
# Scenario: chat-perch (macOS), Chat as a Perch (#1502).
# On screen: launches one private-HOME Fidget/BMO instance with Chat, focuses
#   Chat, and places the Character on Chat's ledge. The waiting question shows
#   a bubble over Chat. The script clicks its chat button, then clicks and
#   types into the Chat composer. Two cropped screenshots are captured.
#   The instance quits at the end.
# Input: synthetic mouse clicks and typing inside Fidget only; no Finder or Poke.
# Duration: a few seconds, 20 s at most.
# Grants: Accessibility and Screen Recording for the terminal running it.
# Asserts: focused Chat stays at layer 0, supports a Perch, accepts composer
#   input, and its overlapping bubble button invokes the Chat command.
# Evidence: a fresh fidget-scenario-chat-perch directory under TMPDIR.
# Usage: chat-perch.sh --go <fidget binary> <fidget test binary>
# Setup: --prepare compiles helpers without touching the GUI.
# Offline: --check-click <log> <first new line> <instance> checks a saved trace.
# Without --go this prints the takeover header and exits 2.
set -euo pipefail

click_traced() {
  local trace=$1 first_line=$2 id=$3
  awk -v first="$first_line" -v marker="bubble: open-chat invoked instance=$id" \
    'NR >= first && $0 == marker { found = 1 } END { exit !found }' "$trace"
}

if [ "${1:-}" = --check-click ]; then
  [ "$#" -eq 4 ] && [[ "$3" =~ ^[1-9][0-9]*$ ]] && [ -n "$4" ] || exit 2
  click_traced "$2" "$3" "$4"
  exit $?
fi

if [ "${1:-}" != --go ] && [ "${1:-}" != --prepare ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi

root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.mac.sh"
helper_key=$(shasum "$root/scripts/scenarios/window-layer.swift" \
  "$root/scripts/scenarios/chat-perch-window.swift" "$root/scripts/ax-settings.swift" \
  "$root/scripts/click-cursor.swift" "$root/scripts/warp-cursor.swift" | shasum | cut -d ' ' -f 1)
tools="${TMPDIR:-/tmp}/fidget-chat-perch-tools/$helper_key"
mkdir -p "$tools/module-cache"
for spec in window-layer:scenarios/window-layer chat-window:scenarios/chat-perch-window ax:ax-settings click-cursor:click-cursor warp-cursor:warp-cursor; do
  tool=${spec%%:*}
  source="$root/scripts/${spec#*:}.swift"
  [ "$tools/$tool" -nt "$source" ] || swiftc -module-cache-path "$tools/module-cache" -O "$source" -o "$tools/$tool"
done
[ "$1" != --prepare ] || exit 0

bin=${2:?fidget binary required}
test_bin=${3:?fidget test binary required}
bin=$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
verify_bin="$(dirname "$bin")/fidget-verify"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-chat-perch-XXXXXX")
mkdir -p "$out/home"
seed_tour_shown "$out/home"
log="$out/app.log"
marks="$out/harness.log"
: > "$marks"
pid=""

cleanup() {
  stop_fidget
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

fail() {
  echo "FAIL: $*" | tee "$out/result.txt" >&2
  echo "Evidence: $out" >&2
  exit 1
}

for tool in window-layer chat-window ax click-cursor warp-cursor; do
  [ -x "$tools/$tool" ] || fail "missing prebuilt helper $tools/$tool"
done
if ! { [ -x "$bin" ] && [ -x "$test_bin" ] && [ -x "$verify_bin" ]; }; then
  fail "missing Fidget, fidget-verify, or fixture test binary"
fi

info() { "$tools/chat-window" info "$pid" BMO; }
layer_zero() { [ "$("$tools/window-layer" "$pid" BMO)" = 0 ]; }
chat_focused_and_normal() {
  [ "$("$tools/chat-window" focused-window)" = "$pid|BMO" ] && layer_zero
}

hit() {
  local x=$1 y=$2 expected=$3 actual
  actual=$("$tools/chat-window" element-at "$x" "$y") ||
    fail "cannot identify the topmost AX hit target at ($x,$y)"
  printf 'point=%s,%s target=%s\n' "$x" "$y" "$actual" >> "$out/target-checks.txt"
  case "$actual" in
    "$expected"*) ;;
    *) fail "topmost hit target at ($x,$y) is $actual, expected $expected" ;;
  esac
}
probe_chat() {
  local value status
  for mode in focused-window frontmost; do
    if value=$("$tools/chat-window" "$mode" 2>&1); then status=0; else status=$?; fi
    printf '%s_status=%s %s=%s\n' "$mode" "$status" "$mode" "$value"
  done
  if value=$("$tools/window-layer" "$pid" BMO 2>&1); then status=0; else status=$?; fi
  printf 'layer_status=%s layer=%s\n' "$status" "$value"
  if value=$(info 2>&1); then status=0; else status=$?; fi
  printf 'info_status=%s info=%s\n' "$status" "$value"
}
ax_dump() { "$tools/ax" dump "$pid" "$1" frames > "$2"; }
place() {
  HOME="$out/home" "$verify_bin" place --x "$1" --y "$2" >> "$out/place.txt" ||
    fail "debug place failed; see $out/place.txt"
}

composer_rect() {
  python3 - "$1" << 'PY'
import sys
for line in open(sys.argv[1]):
    cells = line.rstrip('\n').split('|')
    if cells[0].startswith('AXTextArea') and len(cells) >= 8 and cells[3].startswith('Ask '):
        print(cells[-1])
        break
PY
}

bubble_button_rect() {
  python3 - "$1" << 'PY'
import sys
for line in open(sys.argv[1]):
    cells = line.rstrip('\n').split('|')
    if cells[0] == 'AXButton' and cells[1] == 'chat' and len(cells) >= 8:
        print(cells[-1])
        break
PY
}

button_over_chat() {
  local rect
  ax_dump Fidget "$out/bubble.ax.txt" || return 1
  rect=$(bubble_button_rect "$out/bubble.ax.txt")
  [ -n "$rect" ] || return 1
  IFS=, read -r bx by bw bh <<< "$rect"
  button_x=$((bx + bw / 2))
  button_y=$((by + bh / 2))
  [ "$bx" -ge "$chat_x" ] && [ $((bx + bw)) -le $((chat_x + chat_w)) ] &&
    [ "$by" -ge "$chat_y" ] && [ $((by + bh)) -le $((chat_y + chat_h)) ]
}

perched_now() {
  python3 - "$log" "$before_place_lines" "$ledge_y" "$chat_x" "$chat_w" \
    "$out/ledge-frames.txt" << 'PY'
import re
import sys

log, before, ledge, left, width, evidence = sys.argv[1:]
before, ledge, left, width = map(int, (before, ledge, left, width))
lines = open(log).readlines()[before:]
frames = []
for line in lines:
    match = re.match(r'^frame: \d+ (\w+) pos\((-?\d+),(-?\d+)\)', line)
    if match:
        frames.append((match[1], int(match[2]), int(match[3]), line))
tail = frames[-5:]
if len(tail) == 5 and all(frame[0] == 'Perched'
        and left <= frame[1] < left + width and abs(frame[2] - ledge) <= 2
        for frame in tail):
    open(evidence, 'w').writelines(frame[3] for frame in tail)
    sys.exit(0)
sys.exit(1)
PY
}

chat_at_y() { # <y>
  local y
  IFS=, read -r _ y _ <<< "$(info)"
  [ "$y" = "$1" ]
}

last_button=
button_steady() {
  local was=$last_button
  button_over_chat || return 1
  last_button="$button_x,$button_y"
  [ "$was" = "$last_button" ]
}

composer_focused() {
  [ "$("$tools/chat-window" focused-element 2> /dev/null)" = "$pid|BMO|AXTextArea|" ]
}

point_cursor_at() {
  local x=$1 y=$2 landed
  landed=$("$tools/warp-cursor" "$x" "$y" --event) ||
    fail "could not move cursor to Fidget target ($x,$y)"
  [ "$landed" = "$x $y" ] ||
    fail "cursor landed at $landed instead of Fidget target ($x,$y)"
}

overlay_state_at() {
  local x=$1 y=$2 first_line=$3 expected=$4 latest
  latest=$(awk -v first="$first_line" -v point="point($x,$y) on overlay 0" \
    'NR >= first && index($0, point) { found = $0 } END { print found }' "$log")
  [ -n "$latest" ] && [[ "$latest" == *"$expected"* ]]
}

screen=$("$tools/chat-window" screen)
IFS=, read -r screen_x screen_y screen_w screen_h <<< "$screen"
if ! { [ "$screen_w" -gt 0 ] && [ "$screen_h" -gt 0 ]; }; then
  fail "macOS window server is unavailable to this process"
fi
ax_trust=$("$tools/chat-window" ax-trusted)
echo "ax_trusted=$ax_trust" > "$out/preflight.txt"
[ "$ax_trust" = yes ] || fail "Accessibility trust is absent; see $out/preflight.txt"
if preflight_focus=$("$tools/chat-window" focused-window 2>&1); then
  preflight_focus_status=0
else
  preflight_focus_status=$?
fi
printf 'focused_window_status=%s focused_window=%s\n' \
  "$preflight_focus_status" "$preflight_focus" >> "$out/preflight.txt"
harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=scenario-asking count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "Harness path contains spaces"

start_ms=$(now_ms)
env HOME="$out/home" FIDGET_HARNESS="$harness" FIDGET_OPEN_CHAT=1 \
  FIDGET_DEBUG_IPC=1 FIDGET_DIRECTOR_WAKE_SECS=1 FIDGET_DIRECTOR_API_KEY=x \
  FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_HITTEST=1 \
  FIDGET_TRACE_BUBBLE=1 FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
echo "pid=$pid" > "$out/run.txt"
echo "commit=$(git -C "$root" rev-parse HEAD)" >> "$out/run.txt"
echo "screen=$screen" >> "$out/run.txt"

wait_until 3 info || fail "Chat window did not open"
wait_for_marks 3 "$marks" '^asked$' || fail "fixture did not ask its question"
IFS=, read -r chat_x chat_y chat_w chat_h chat_layer <<< "$(info)"
if ! { [ "$chat_x" -ge "$screen_x" ] && [ $((chat_x + chat_w)) -le $((screen_x + screen_w)) ]; }; then
  fail "Chat is not wholly on the primary display"
fi
chat_title_x=$((chat_x + chat_w / 2))
chat_title_y=$((chat_y + 14))
before_pointer_lines=$(($(wc -l < "$log") + 1))
point_cursor_at "$chat_title_x" "$chat_title_y"
wait_until 3 overlay_state_at "$chat_title_x" "$chat_title_y" "$before_pointer_lines" "miss click-through on" ||
  fail "overlay did not release input over Chat title"
hit "$chat_title_x" "$chat_title_y" "$pid|BMO|"
"$tools/click-cursor" "$chat_title_x" "$chat_title_y" 1 \
  > "$out/chat-focus-click.txt" || fail "could not focus Chat"
if ! wait_until 3 chat_focused_and_normal; then
  probe_chat > "$out/initial-chat-state.txt"
  fail "focused Chat did not stay at layer 0; see $out/initial-chat-state.txt"
fi
probe_chat > "$out/initial-chat-state.txt"
echo 'normal_level=pass' >> "$out/result.txt"

IFS=, read -r chat_x chat_y chat_w chat_h chat_layer <<< "$(info)"
target_y=$((screen_y + 160))
[ $((target_y + chat_h)) -le $((screen_y + screen_h)) ] ||
  fail "display is too short for the bubble-over-Chat placement"
if [ "$chat_y" -ne "$target_y" ]; then
  "$tools/chat-window" move "$pid" BMO "$chat_x" "$target_y" ||
    fail "could not move Chat to a perchable ledge"
  wait_until 3 chat_at_y "$target_y" || fail "Chat never reached y=$target_y after the move"
fi
IFS=, read -r chat_x chat_y chat_w chat_h chat_layer <<< "$(info)"
echo "chat=$chat_x,$chat_y,$chat_w,$chat_h,$chat_layer" >> "$out/run.txt"
[ "$chat_layer" -eq 0 ] || fail "Chat rose to layer $chat_layer after move"

ledge_x=$((chat_x + chat_w / 2))
ledge_y=$chat_y
before_place_lines=$(wc -l < "$log")
place "$ledge_x" "$((ledge_y - 60))"
wait_until 3 perched_now || fail "Character did not settle Perched on Chat at y=$ledge_y"
echo 'perch=pass' >> "$out/result.txt"

capture_x=$chat_x
capture_y=$((chat_y - 120))
[ "$capture_y" -ge "$screen_y" ] || capture_y=$screen_y
capture_h=$((chat_h + chat_y - capture_y))
screencapture -x -R "$capture_x,$capture_y,$chat_w,$capture_h" "$out/perched.png" ||
  fail "could not capture perched Character"

wait_for_trace 3 "$log" 'showAsk instance=' ||
  fail "perching did not show the question bubble"
wait_until 3 button_over_chat || fail "question bubble button did not overlap Chat"
wait_until 2 button_steady || fail "question bubble button moved or left Chat before the click"
echo "button=$button_x,$button_y" >> "$out/run.txt"
echo 'bubble_over_chat=pass' >> "$out/result.txt"
screencapture -x -R "$capture_x,$capture_y,$chat_w,$capture_h" "$out/bubble-over-chat.png" ||
  fail "could not capture bubble over Chat"

before_pointer_lines=$(($(wc -l < "$log") + 1))
point_cursor_at "$button_x" "$button_y"
wait_until 3 overlay_state_at "$button_x" "$button_y" \
  "$before_pointer_lines" "click-through OFF" ||
  fail "overlay did not accept input at the bubble button"
hit "$button_x" "$button_y" "$pid|Fidget|AXButton|chat"
ask_id=$(sed -n 's/.*showAsk instance=\([^ ]*\) dnd=.*/\1/p' "$log" | tail -n 1)
[ -n "$ask_id" ] || fail "could not identify question owner before click"
before_click_lines=$(($(wc -l < "$log") + 1))
"$tools/click-cursor" "$button_x" "$button_y" 1 > "$out/bubble-click.txt" ||
  fail "could not click the bubble button"
wait_until 3 click_traced "$log" "$before_click_lines" "$ask_id" ||
  fail "bubble button did not invoke open Chat; see $log"
if ! wait_until 3 chat_focused_and_normal; then
  probe_chat > "$out/after-bubble-chat-state.txt"
  fail "Chat was not focused at normal level after bubble click"
fi
probe_chat > "$out/after-bubble-chat-state.txt"
echo 'bubble_button_click=pass' >> "$out/result.txt"

ax_dump BMO "$out/chat-before.ax.txt" || fail "could not inspect Chat"
rect=$(composer_rect "$out/chat-before.ax.txt")
[ -n "$rect" ] || fail "could not find enabled Chat composer"
IFS=, read -r cx cy cw ch <<< "$rect"
composer_x=$((cx + cw / 2))
composer_y=$((cy + ch / 2))
chat_focused_and_normal || fail "Chat lost focus before the composer click"
before_pointer_lines=$(($(wc -l < "$log") + 1))
point_cursor_at "$composer_x" "$composer_y"
wait_until 3 overlay_state_at "$composer_x" "$composer_y" "$before_pointer_lines" "miss click-through on" ||
  fail "overlay did not release the Chat composer point"
hit "$composer_x" "$composer_y" "$pid|BMO|AXTextArea|"
"$tools/click-cursor" "$composer_x" "$composer_y" 1 > "$out/composer-click.txt" ||
  fail "could not click Chat composer"
wait_until 2 composer_focused || fail "composer did not receive keyboard focus after the click"
chat_focused_and_normal || fail "Chat lost focus after the composer click"
"$tools/chat-window" focused-element > "$out/composer-focus.txt" ||
  fail "cannot read keyboard focus after the composer click"
grep -Fxq "$pid|BMO|AXTextArea|" "$out/composer-focus.txt" ||
  fail "composer did not receive keyboard focus after the click"
marker="macOS Chat perch"
"$tools/ax" type "$pid" "$marker" > "$out/type.txt" ||
  fail "could not type into Chat"
ax_dump BMO "$out/chat-after.ax.txt" || fail "could not inspect Chat after typing"
grep -Fq "$marker" "$out/chat-after.ax.txt" ||
  fail "typed text did not appear in Chat composer"
echo 'composer_click_and_type=pass' >> "$out/result.txt"

echo "PASS: all four macOS checks; evidence in $out ($(($(now_ms) - start_ms))ms)"
