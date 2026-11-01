#!/usr/bin/env bash
# Scenario: control-click-menu (X11)
# On screen: launches Fidget as BMO with a fixture Harness. The scenario finds
#   the sprite, right-clicks its centre, waits for the menu to appear, and
#   reads its items. Escape closes the menu. Fidget quits when the scenario
#   ends.
# Input: one real right-click on the sprite and one Escape, through xdotool.
#   Fidget treats Control-click as the menu gesture only on macOS; here the right button is the menu gesture.
# Duration: a few seconds, 10 s at most.
# Grants: an X11 session, AT-SPI (python3-pyatspi) and xdotool.
# Asserts: the right-click lands as Menu, and the open menu's AT-SPI items
#   include Chat…, Settings… and Quit.
#
# Usage: control-click-menu.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# FIDGET_SCENARIO_MENU asserts that item list, one name per line, and skips
# the GUI. That checks the assertion, not the live menu.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: control-click-menu.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: control-click-menu.x11.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

check_items() { # <item list>
  for item in "Chat…" "Settings…" "Quit"; do
    grep -qxF "$item" "$1" || fail "no '$item' in the menu; see $1"
    echo "ok: the menu holds $item"
  done
}

if [ -n "${FIDGET_SCENARIO_MENU:-}" ]; then
  check_items "$FIDGET_SCENARIO_MENU"
  echo "PASS: fixture dumps"
  exit 0
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 control-click-menu needs a session, or set FIDGET_SCENARIO_MENU." >&2
  exit 2
fi
python3 -c 'import pyatspi' > /dev/null 2>&1 || {
  echo "SKIP: python3-pyatspi is not installed." >&2
  exit 2
}
command -v xdotool > /dev/null 2>&1 || {
  echo "SKIP: xdotool is not installed." >&2
  exit 2
}

test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-control-click-menu-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home/.local/share/fidget"
log="$out/app.log" marks="$out/harness.log"
# Fidget's stderr: process.log puts a timestamp before each line.
trace=$log
: > "$marks"

cat > "$out/home/.local/share/fidget/settings.json" << 'SETTINGS_EOF'
{"first_run_tour_shown": true}
SETTINGS_EOF

# count= gives this run's Harness a unique command line for the kill on exit.
harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=nop count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

start_ms=$(($(date +%s%N) / 1000000))
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'stop_fidget; pkill -f "count=$marks" || true' EXIT INT TERM
wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"

# The overlay spans the display, so its centre is not the sprite. The newest
# frame trace line says where the sprite is drawn.
wait_for_trace 2 "$trace" '^frame: .* sprite\(' || fail "Fidget traced no frame; see $trace"
wait_for_rest "$trace" || fail "the sprite never came to rest; see $trace"
read -r w h < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$trace" | head -1) || fail "no sprite size in $trace"
read -r x y < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$trace" | tail -1) || fail "no frame trace in $trace"
cx=$((x + w / 2))
cy=$((y + h / 2))
echo "ok: sprite at ($x,$y) size ${w}x${h}, centre ($cx,$cy)"

items="$out/menu.txt"
python3 "$root/scripts/ax-window-linux.py" menu "$pid" "$cx" "$cy" > "$items" 2> "$out/menu.err" ||
  fail "no open menu in the AT-SPI tree; see $out/menu.err"

wait_for_trace 2 "$trace" '^verbs: .*\[Menu\]' || fail "the right-click did not land as Menu; see $trace"
echo "ok: the right-click landed as Menu"
check_items "$items"

elapsed_ms=$(($(date +%s%N) / 1000000 - start_ms))
echo "PASS: evidence in $out (${elapsed_ms}ms)"
