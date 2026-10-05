#!/usr/bin/env bash
# Scenario: control-click-menu (X11)
# On screen: launches Fidget as BMO with a fixture Harness. The scenario finds
#   the sprite, right-clicks its centre, waits for the menu to appear, and
#   reads its items. Escape closes the menu. Fidget quits when the scenario
#   ends.
# Input: one real right-click on the sprite and one Escape, through xdotool.
#   X11 has no Control-click; the right button is the menu gesture there.
# Duration: about 20 s, 1 min at most.
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
root=$(cd "$(dirname "$0")/../.." && pwd)

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
mkdir -p "$out/home"
log="$out/app.log"

harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=nop"
[ "$(wc -w <<< "$harness")" -eq 3 ] || fail "a path in the Harness line holds a space: $harness"

env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2> /dev/null || true; pkill -f "script=nop" || true' EXIT

wait_for() { # <seconds> <command...>
  local n=$(($1 * 4))
  shift
  until "$@" > /dev/null 2>&1; do
    kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
    n=$((n - 1))
    [ "$n" -gt 0 ] || return 1
    sleep 0.25
  done
}

# The overlay spans the display, so its centre is not the sprite. The newest
# frame trace line says where the sprite is drawn.
wait_for 15 grep -q '^frame: .* sprite(' "$log" || fail "Fidget traced no frame; see $log"
sleep 2
read -r w h < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r x y < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$log" | tail -1) || fail "no frame trace in $log"
cx=$((x + w / 2))
cy=$((y + h / 2))
echo "ok: sprite at ($x,$y) size ${w}x${h}, centre ($cx,$cy)"

items="$out/menu.txt"
python3 "$root/scripts/ax-window-linux.py" menu "$pid" "$cx" "$cy" > "$items" 2> "$out/menu.err" ||
  fail "no open menu in the AT-SPI tree; see $out/menu.err"

wait_for 3 grep -q '^verbs: .*\[Menu\]' "$log" || fail "the right-click did not land as Menu; see $log"
echo "ok: the right-click landed as Menu"
check_items "$items"

echo "PASS: evidence in $out"
