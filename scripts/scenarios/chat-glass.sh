#!/usr/bin/env bash
# Scenario: chat-glass (macOS)
# On screen: covers the main display with a window of coloured stripes, then
#   launches Fidget as BMO with the Glass Chat design and a fixture Harness.
#   Chat opens by itself over the stripes. One capture of the Chat window and
#   one of its rectangle on screen, which shows only Chat and the stripes. Then
#   Fidget and the stripes quit.
# Input: none.
# Duration: a few seconds, 25 s at most.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: the panel is as see-through as the Glass design says, and what shows
#   through it is blurred. The window capture keeps the window's alpha: the
#   design's tint runs from .62 to .83 under the log, so the median alpha must
#   lie in 140 to 219. The screen capture must show no hard stripe edge in the
#   rows that hold no text.
#
# Usage: chat-glass.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: chat-glass.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: chat-glass.sh --go <fidget binary> <fidget test binary>}
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-chat-glass-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home/Library/Application Support/fidget" "$tools"
echo '{"first_run_tour_shown": true, "chat_ui": "glass"}' \
  > "$out/home/Library/Application Support/fidget/settings.json"
log="$out/app.log" marks="$out/harness.log"
: > "$marks"

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

for tool in window-id pixel-stats stripes-window; do
  [ "$tools/$tool" -nt "$root/scripts/scenarios/$tool.swift" ] ||
    swiftc -O "$root/scripts/scenarios/$tool.swift" -o "$tools/$tool"
done
[ "$tools/ax" -nt "$root/scripts/ax-settings.swift" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"
build_ax_events

# FIDGET_HARNESS splits on whitespace, so no path in it may hold a space.
harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=scenario-thinking count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

start_ms=$(now_ms)
"$tools/stripes-window" &
stripes=$!
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=1 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$stripes" 2> /dev/null; stop_fidget; pkill -f "count=$marks" || true' EXIT INT TERM

wait_for_marks 10 "$marks" '^asked$' || fail "no wake reached the Harness; see $log"
wait_for_ax 10 "$pid" "$tools/window-id" "$pid" BMO || fail "Chat did not open"
wait_for_ax_dump 10 "$pid" BMO '^AXButton\|Allow\|' || fail "Chat never drew the ask; see $log"
screencapture -x -o -l "$("$tools/window-id" "$pid" BMO)" "$out/chat.png"
screencapture -x -R "$("$tools/window-id" "$pid" BMO bounds)" "$out/screen.png"
read -r alpha _ < <("$tools/pixel-stats" "$out/chat.png")
read -r _ step < <("$tools/pixel-stats" "$out/screen.png")
[ "$alpha" -ge 140 ] || fail "Glass Chat is barely painted: median alpha $alpha of 255; see $out/chat.png"
[ "$alpha" -lt 220 ] || fail "Glass Chat is too opaque: median alpha $alpha of 255; see $out/chat.png"
echo "ok: median alpha $alpha of 255"
[ "$step" -lt 6 ] || fail "Glass Chat does not blur the stripes behind it: edge step $step; see $out/screen.png"
echo "ok: edge step $step behind the panel"
echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
