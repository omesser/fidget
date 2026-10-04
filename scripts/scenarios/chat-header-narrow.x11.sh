#!/usr/bin/env bash
# Scenario: chat-header-narrow (X11)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and takes focus, then is resized to 420, 360 and 320 wide. One
#   AT-SPI dump of the Chat window per width. Fidget quits when it ends.
# Input: none. The resizes go through AT-SPI, not the mouse.
# Duration: about 30 s, 2 min at most.
# Grants: a running X11 session. AT-SPI (python3-pyatspi) for the terminal that runs it.
# Asserts: at each width the Instance name, the Character chip and the mind
#   line share one row, all three end inside the window, and the page is no
#   wider than its scroll area, so Chat never scrolls sideways. The mind line
#   names the fixture's long unbroken path, the shape #1090 reports.
#
# Usage: chat-header-narrow.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_AX_420, FIDGET_SCENARIO_AX_360 and FIDGET_SCENARIO_AX_320
# to assert those dumps and skip the GUI. That checks the assertion, not the live window.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: chat-header-narrow.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: chat-header-narrow.x11.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../.." && pwd)

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

mind='^label\|[^|]* · session [^|]+\|'
rect() { # <dump line>: its frame as "x y w h"
  tr , ' ' <<< "${1##*|}"
}

check_dump() { # <width> <file>
  local w=$1 f=$2 x y ww h wx wr sw aw rows label top=-1 bottom=-1
  [ -f "$f" ] || fail "$w: missing dump $f"
  rows=$(grep -E '^label\|' "$f" | grep -EB2 -m1 "$mind") ||
    fail "$w: no mind line naming a session in $f"
  [ "$(wc -l <<< "$rows")" -eq 3 ] || fail "$w: no name and chip before the mind line in $f"

  frame_line=$(grep -m1 '^frame|' "$f" || true)
  [ -n "$frame_line" ] || fail "$w: no frame line in $f"
  read -r wx _ ww _ <<< "$(rect "$frame_line")"
  [ "$ww" -eq "$w" ] || fail "$w: frame is $ww wide in $f"
  wr=$((wx + ww))

  while IFS= read -r row; do
    read -r x y ww h <<< "$(rect "$row")"
    label=$(cut -d'|' -f2 <<< "$row")
    [ $((x + ww)) -le $((wr + 1)) ] || fail "$w: '$label' ends at $((x + ww)), past the window edge at $wr"
    if [ "$top" -lt 0 ]; then
      top=$y bottom=$((y + h))
    elif [ "$y" -ge "$bottom" ] || [ $((y + h)) -le "$top" ]; then
      fail "$w: '$label' at y $y..$((y + h)) left the name's row $top..$bottom"
    fi
  done <<< "$rows"

  read -r _ _ sw _ <<< "$(rect "$(grep -m1 '^scroll-area|' "$f")")"
  read -r _ _ aw _ <<< "$(rect "$(grep -m1 '^web-area|' "$f")")"
  [ "$aw" -le $((sw + 1)) ] || fail "$w: the page is $aw wide in a $sw scroll area, so Chat scrolls sideways"
  echo "ok: $w: one row inside the window, page $aw of $sw"
}

if [ -n "${FIDGET_SCENARIO_AX_420:-}" ] || [ -n "${FIDGET_SCENARIO_AX_360:-}" ] || [ -n "${FIDGET_SCENARIO_AX_320:-}" ]; then
  if [ -z "${FIDGET_SCENARIO_AX_420:-}" ] || [ -z "${FIDGET_SCENARIO_AX_360:-}" ] || [ -z "${FIDGET_SCENARIO_AX_320:-}" ]; then
    fail "set FIDGET_SCENARIO_AX_420, FIDGET_SCENARIO_AX_360 and FIDGET_SCENARIO_AX_320"
  fi
  for w in 420 360 320; do
    eval "f=\$FIDGET_SCENARIO_AX_$w"
    check_dump "$w" "$f"
  done
  echo "PASS: fixture dumps"
  exit 0
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 chat-header-narrow needs a session, or set FIDGET_SCENARIO_AX_420, FIDGET_SCENARIO_AX_360 and FIDGET_SCENARIO_AX_320." >&2
  exit 2
fi
python3 -c 'import pyatspi' > /dev/null 2>&1 || {
  echo "SKIP: python3-pyatspi is not installed." >&2
  exit 2
}

test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-chat-header-narrow-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home"
log="$out/app.log" marks="$out/harness.log"
: > "$marks"

harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=scenario-thinking count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=6 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2> /dev/null || true; pkill -f "count=$marks" || true' EXIT

wait_for() {
  local n=$(($1 * 4))
  shift
  until "$@" > /dev/null 2>&1; do
    kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
    n=$((n - 1))
    [ "$n" -gt 0 ] || return 1
    sleep 0.25
  done
}

dump() { # <name>
  python3 "$root/scripts/ax-window-linux.py" dump "$pid" BMO frames > "$out/$1.ax.txt" 2> "$out/$1.err" ||
    fail "$1: AT-SPI dump failed; see $out/$1.err"
}

shows_session() {
  dump session && grep -qE "$mind" "$out/session.ax.txt"
}

check() { # <width>
  local w=$1
  local f=$out/$w.ax.txt got
  python3 "$root/scripts/ax-window-linux.py" size "$pid" BMO "$w" 560 > "$out/$w.frame.txt" 2> "$out/$w.frame.err" ||
    fail "$w: resize failed; see $out/$w.frame.err"
  read -r _ _ got _ < <(tr , ' ' < "$out/$w.frame.txt")
  [ "$got" -eq "$w" ] || fail "$w: Chat is $got wide after the resize"
  sleep 1
  dump "$w" || fail "$w: AX dump failed; see $f"
  check_dump "$w" "$f"
}

wait_for 30 grep -qx asked "$marks" || fail "no wake reached the Harness; see $log"
wait_for 40 shows_session || fail "the mind line names no session; see $out/session.ax.txt"
echo "ok: $(grep -oEm1 '[^|]* · session [^|]+' "$out/session.ax.txt" | head -1)"
for w in 420 360 320; do check "$w"; done
echo "PASS: evidence in $out"
