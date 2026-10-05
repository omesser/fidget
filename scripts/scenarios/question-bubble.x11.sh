#!/usr/bin/env bash
# Scenario: question-bubble (X11)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and shows a permission question. A Poke is sent to the sprite while
#   the question waits. Two AT-SPI dumps of the overlay: one before the Poke,
#   one after it. Fidget quits when the scenario ends.
# Input: one real click on the sprite (the Poke) through xdotool.
# Duration: about 20 s, 1 min at most.
# Grants: an X11 session, AT-SPI (python3-pyatspi) and xdotool.
# Asserts: no "Question for you" bubble before the Poke; the click lands as a
#   Poke; after it, the bubble reads "Question for you in the" with a "chat"
#   link button.
#
# Usage: question-bubble.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# FIDGET_SCENARIO_AX_BEFORE and FIDGET_SCENARIO_AX_AFTER assert those dumps and
# skip the GUI. That checks the assertion, not the live window.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: question-bubble.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: question-bubble.x11.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../.." && pwd)

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

check_before() { # <dump>
  ! grep -qF "Question for you" "$1" || fail "the question cue showed before the Poke; see $1"
  echo "ok: no question cue before the Poke"
}

# The trailing space of "in the " may not survive the accessibility name, and
# "in the chat" as one label is the variant with no link to click.
check_after() { # <dump>
  grep -qE '^(label\|Question for you in the ?|section\|Question for you in the chat)$' "$1" || fail "the bubble does not read 'Question for you in the'; see $1"
  grep -qx 'button|chat' "$1" || fail "the bubble has no 'chat' link button; see $1"
  echo "ok: bubble shows 'Question for you in the' and a 'chat' link button"
}

if [ -n "${FIDGET_SCENARIO_AX_BEFORE:-}" ] || [ -n "${FIDGET_SCENARIO_AX_AFTER:-}" ]; then
  if [ -z "${FIDGET_SCENARIO_AX_BEFORE:-}" ] || [ -z "${FIDGET_SCENARIO_AX_AFTER:-}" ]; then
    fail "set both FIDGET_SCENARIO_AX_BEFORE and FIDGET_SCENARIO_AX_AFTER"
  fi
  check_before "$FIDGET_SCENARIO_AX_BEFORE"
  check_after "$FIDGET_SCENARIO_AX_AFTER"
  echo "PASS: fixture dumps"
  exit 0
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 question-bubble needs a session, or set FIDGET_SCENARIO_AX_BEFORE and FIDGET_SCENARIO_AX_AFTER." >&2
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
out="${TMPDIR:-/tmp}/fidget-scenario-question-bubble-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home"
log="$out/app.log" marks="$out/harness.log"
: > "$marks"

harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=scenario-asking count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=6 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2> /dev/null || true; pkill -f "count=$marks" || true' EXIT

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

# The overlay window is titled Fidget; Chat takes the Character's name.
dump() { python3 "$root/scripts/ax-window-linux.py" dump "$pid" Fidget > "$out/$1.ax.txt" 2> "$out/$1.err" || fail "AT-SPI dump failed; see $out/$1.err"; }

wait_for 30 grep -qx asked "$marks" || fail "no wake reached the Harness; see $log"
sleep 1

# The overlay spans the display, so its centre is not the sprite. The newest
# frame trace line says where the sprite is drawn.
read -r sprite_w sprite_h < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r sx sy < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$log" | tail -1) || fail "no frame trace in $log"
dump before-poke
check_before "$out/before-poke.ax.txt"

{ xdotool mousemove --sync "$((sx + sprite_w / 2))" "$((sy + sprite_h / 2))" && sleep 0.05 && xdotool mousedown 1 && sleep 0.12 && xdotool mouseup 1; } > "$out/poke.txt" 2>&1 ||
  fail "could not click the sprite; see $out/poke.txt"
wait_for 3 grep -q '^verbs: .*Poke' "$log" || fail "the click did not land as a Poke; see $log"
sleep 1
dump after-poke
check_after "$out/after-poke.ax.txt"

echo "PASS: evidence in $out"
