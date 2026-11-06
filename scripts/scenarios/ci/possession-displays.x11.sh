#!/usr/bin/env bash
# Scenario: possession-displays (X11)
# On screen: splits the X screen into two side-by-side RandR monitors, launches
#   Fidget as BMO with a fixture Harness, presses Possess, and holds an arrow
#   until BMO walks onto the other monitor. There it walks and jumps BMO and
#   presses Escape. Fidget quits and the monitors are removed at the end.
# Input: a right-click on the sprite and a click on Possess, an arrow held for
#   the walk across and again briefly, Space, and Escape. Through xdotool.
# Duration: about 15 s, 45 s at most.
# Grants: an X11 session with RandR 1.5, AT-SPI (python3-pyatspi), xdotool and
#   xrandr.
# Asserts: the held arrow walks the possessed sprite across the border and the
#   arrow's keyup lands on the new monitor's overlay and stops it; an arrow and
#   Space drive it there; Escape releases possession. Xvfb monitors share one
#   framebuffer and one window manager, so a PASS is not proof on real
#   multi-display hardware.
# Skips (exit 2) when GTK still sees one monitor after the split.
# Usage: possession-displays.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: possession-displays.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: possession-displays.x11.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}
skip() {
  echo "SKIP: $*" >&2
  exit 2
}

[ -n "${DISPLAY:-}" ] || skip "DISPLAY is unset. X11 possession-displays needs a session."
python3 -c 'import pyatspi' > /dev/null 2>&1 || skip "python3-pyatspi is not installed."
for tool in xdotool xrandr; do
  command -v "$tool" > /dev/null 2>&1 || skip "$tool is not installed."
done

test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-possession-displays-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home/.local/share/fidget"
log="$out/app.log" marks="$out/harness.log"
: > "$marks"
cat > "$out/home/.local/share/fidget/settings.json" << 'SETTINGS_EOF'
{"first_run_tour_shown": true}
SETTINGS_EOF

harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=nop count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

cleanup() {
  xdotool keyup Right Left space 2> /dev/null || true
  stop_fidget
  pkill -f "count=$marks" || true
  xrandr --delmonitor fidget-left > /dev/null 2>&1 || true
  xrandr --delmonitor fidget-right > /dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

# The left monitor takes the screen's output, so its automatic monitor goes.
output=$(xrandr | awk '/ connected/ { print $1; exit }')
[ -n "$output" ] || fail "xrandr lists no connected output"
read -r screen_w screen_h < <(xdotool getdisplaygeometry)
half=$((screen_w / 2))
if ! xrandr --setmonitor fidget-left "$half/254x$screen_h/286+0+0" "$output" > "$out/xrandr.txt" 2>&1 ||
  ! xrandr --setmonitor fidget-right "$((screen_w - half))/254x$screen_h/286+$half+0" none >> "$out/xrandr.txt" 2>&1; then
  fail "xrandr could not split the screen; see $out/xrandr.txt"
fi
xrandr --listmonitors >> "$out/xrandr.txt" 2>&1

last_frame() { grep '^frame: ' "$log" | tail -1; }
sprite_x() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),.*/\1/'; }
sprite_xy() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/'; }
frames() { line_count "$log" '^frame: '; }
walked_from() { # <x>
  [ $((($(sprite_x) - $1) * sign)) -ge 8 ]
}
# The sprite's left edge (going right) or right edge (going left) is past the border.
crossed() {
  local x
  x=$(sprite_x)
  if [ "$sign" -gt 0 ]; then [ "$x" -ge "$border" ]; else [ $((x + w)) -le "$border" ]; fi
}
# The newest <n> frames draw the sprite in one spot.
still_for() { # <n>
  grep '^frame: ' "$log" | tail -"$1" | grep -oE 'sprite\(-?[0-9]+,-?[0-9]+\)' |
    awk -v n="$1" '{ seen[$0] = 1; count++ } END { exit !(count == n && length(seen) == 1) }'
}
jumped_after() { # <frame count>
  grep '^frame: ' "$log" | tail -n +"$(($1 + 1))" | grep -qE ' Falling | jump#'
}
step_arrow() { # <what>
  local x
  x=$(sprite_x)
  xdotool keydown "$arrow"
  wait_until 3 walked_from "$x" || fail "$1 did not walk the sprite; see $log"
  xdotool keyup "$arrow"
  wait_until 3 still_for 5 || fail "the sprite kept walking after $1; see $log"
}

start_ms=$(now_ms)
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_trace 3 "$log" '^frame: .* sprite\(' || fail "Fidget traced no frame; see $log"
wait_for_rest "$log" || fail "the sprite never came to rest; see $log"
read -r w h < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r sx sy < <(sprite_xy)
cx=$((sx + w / 2)) cy=$((sy + h / 2))
echo "ok: sprite at ($sx,$sy) size ${w}x${h}"

# Each overlay line is "<label> <x> <y> <w> <h>".
displays=$(sed -nE 's/^overlay: (overlay-[0-9]+) covers ([0-9]+)x([0-9]+) at \((-?[0-9]+),(-?[0-9]+)\)$/\1 \4 \5 \2 \3/p' "$log")
[ "$(grep -c . <<< "$displays")" -ge 2 ] || skip "GTK sees one monitor after the split; see $out/xrandr.txt and $log"
read -r here hx _ hw _ < <(awk -v x="$cx" -v y="$cy" '$2 <= x && x < $2 + $4 && $3 <= y && y < $3 + $5' <<< "$displays" | head -1) ||
  fail "no overlay covers the sprite's centre; see $log"
read -r next nx _ nw _ sign < <(awk -v l="$here" -v x="$hx" -v w="$hw" -v y="$cy" '
  $1 != l && $3 <= y && y < $3 + $5 {
    if ($2 + $4 == x) { print $0, -1; exit }
    if ($2 == x + w) { print $0, 1; exit }
  }' <<< "$displays") || fail "no overlay sits beside $here; see $log"
if [ "$sign" -gt 0 ]; then arrow=Right name=right border=$((hx + hw)); else arrow=Left name=left border=$hx; fi
echo "ok: $next is to the $name of $here"

python3 "$root/scripts/ax-window-linux.py" menu "$pid" "$cx" "$cy" Possess > "$out/menu.txt" 2> "$out/menu.err" ||
  fail "could not press Possess; see $out/menu.err"
wait_for_trace 3 "$log" '^menu: possession on' || fail "the Possess row did not possess the sprite; see $log"
wait_for_trace 3 "$log" "^overlay $here: window focus" || fail "$here never took focus; see $log"
echo "ok: possessed, and $here has the keyboard"

focuses=$(line_count "$log" "^overlay $next: window focus")
if [ "$sign" -gt 0 ]; then distance=$((border - sx)); else distance=$((sx + w - border)); fi
xdotool keydown "$arrow"
# Budget the walk at 100 px/s, under the ~175 px/s BMO walks.
wait_until $((distance / 100 + 3)) crossed || fail "the held $name arrow did not carry the sprite past x=$border; see $log"
# The keyup is what proves the keyboard; the page's focus line is a note.
wait_for_count 3 "$log" "^overlay $next: window focus" "$focuses" ||
  echo "note: $next logged no page focus after the crossing" | tee -a "$out/notes.txt"
xdotool keyup "$arrow"
wait_for_trace 3 "$log" "^possession: key $next $name up" || fail "the $name keyup never reached $next, so $next does not have the keyboard; see $log"
wait_until 3 still_for 5 || fail "the sprite kept walking after the $name keyup; see $log"
read -r sx sy < <(sprite_xy)
[ $((sx + w / 2)) -ge "$nx" ] && [ $((sx + w / 2)) -lt $((nx + nw)) ] || fail "the sprite came to rest at ($sx,$sy), off $next; see $log"
echo "ok: walked onto $next, which took the keyboard, and stopped on the keyup at ($sx,$sy)"

step_arrow "the $name arrow on $next"
xdotool keydown space
wait_for_trace 3 "$log" "^possession: key $next space down" || fail "Space never reached $next; see $log"
before=$(frames)
xdotool keyup space
wait_until 3 jumped_after "$before" || fail "Space did not jump the sprite on $next; see $log"
wait_until 3 still_for 5 || fail "the sprite never landed after the jump; see $log"
echo "ok: the arrow and Space drive the sprite on $next"

xdotool key Escape
wait_for_trace 3 "$log" '^possession: escape' || fail "Escape did not release possession; see $log"
echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
