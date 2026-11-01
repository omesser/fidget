#!/usr/bin/env bash
# Scenario: chat-header-narrow (macOS)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and takes focus, then is resized to 420, 360 and 320 points wide.
#   One screenshot of the Chat window per width. Fidget quits when it ends.
# Input: none. The resizes go through the Accessibility API, not the mouse.
# Duration: a few seconds, 10 s at most.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: at each width the Instance name, the Character chip and the mind
#   line share one row, all three end inside the window, and the page is no
#   wider than its scroll area, so Chat never scrolls sideways. The mind line
#   names the fixture's long unbroken path, the shape #1090 reports.
#
# Usage: chat-header-narrow.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: chat-header-narrow.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: chat-header-narrow.sh --go <fidget binary> <fidget test binary>}
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-chat-header-narrow-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools"
seed_tour_shown "$out/home"
log="$out/app.log" marks="$out/harness.log"
: > "$marks"

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

[ -x "$tools/window-id" ] || swiftc -O "$root/scripts/scenarios/helpers/window-id.swift" -o "$tools/window-id"
# Rebuilt when the source is newer: this scenario needs its `size` and `frames`.
[ "$tools/ax" -nt "$root/scripts/ax-settings.swift" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"
build_ax_events

# FIDGET_HARNESS splits on whitespace, so no path in it may hold a space.
harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=scenario-thinking count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

start_ms=$(now_ms)
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=1 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'stop_fidget; pkill -f "count=$marks" || true' EXIT INT TERM

mind='^AXStaticText\|[^|]*\|[^|]* · session [^|]+\|'
dump() { # <name>
  "$tools/ax" dump "$pid" BMO frames > "$out/$1.ax.txt" 2>&1
}
shows_session() { dump session && grep -qE "$mind" "$out/session.ax.txt"; }
rect() { # <dump line>: its frame as "x y w h"
  tr , ' ' <<< "${1##*|}"
}
fits_width() { # <width>: the dump's page is no wider than the window
  local aw
  dump "$1" || return 1
  read -r _ _ aw _ <<< "$(rect "$(grep -m1 '^AXWebArea|' "$out/$1.ax.txt")")"
  [ -n "$aw" ] && [ "$aw" -le "$1" ]
}

check() { # <width>
  local w=$1 f=$out/$1.ax.txt got x y ww h wx wr sw aw rows label top=-1 bottom=-1
  "$tools/ax" size "$pid" BMO "$w" 560 > "$out/$w.frame.txt" 2>&1 || fail "$w: resize failed; see $out/$w.frame.txt"
  read -r wx _ got _ < <(tr , ' ' < "$out/$w.frame.txt")
  [ "$got" -eq "$w" ] || fail "$w: Chat is $got points wide after the resize"
  # The resize lands before WebKit reflows, so wait for the page to fit the new width.
  wait_for_ax 2 "$pid" fits_width "$w" || fail "$w: the page never reflowed to fit; see $f"
  screencapture -x -o -l "$("$tools/window-id" "$pid" BMO)" "$out/$w.png"
  wr=$((wx + got))

  # The two texts before the mind line are the Instance name and the chip.
  rows=$(grep -E '^AXStaticText\|' "$f" | grep -EB2 -m1 "$mind") ||
    fail "$w: no mind line naming a session in $f"
  [ "$(wc -l <<< "$rows")" -eq 3 ] || fail "$w: no name and chip before the mind line in $f"
  while IFS= read -r row; do
    read -r x y ww h <<< "$(rect "$row")"
    label=$(cut -d'|' -f3 <<< "$row")
    [ $((x + ww)) -le $((wr + 1)) ] || fail "$w: '$label' ends at $((x + ww)), past the window edge at $wr"
    if [ "$top" -lt 0 ]; then
      top=$y bottom=$((y + h))
    elif [ "$y" -ge "$bottom" ] || [ $((y + h)) -le "$top" ]; then
      fail "$w: '$label' at y $y..$((y + h)) left the name's row $top..$bottom"
    fi
  done <<< "$rows"

  read -r _ _ sw _ <<< "$(rect "$(grep -m1 '^AXScrollArea|' "$f")")"
  read -r _ _ aw _ <<< "$(rect "$(grep -m1 '^AXWebArea|' "$f")")"
  [ "$aw" -le $((sw + 1)) ] || fail "$w: the page is $aw points wide in a $sw-point scroll area, so Chat scrolls sideways"
  echo "ok: $w: one row inside the window, page $aw of $sw points"
}

wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_marks 3 "$marks" '^asked$' || fail "no wake reached the Harness; see $log"
wait_for_ax 3 "$pid" "$tools/window-id" "$pid" BMO || fail "Chat did not open for the ask"
wait_for_ax 3 "$pid" shows_session || fail "the mind line names no session; see $out/session.ax.txt"
echo "ok: $(grep -oEm1 '[^|]* · session [^|]+' "$out/session.ax.txt")"
for w in 420 360 320; do check "$w"; done
echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
