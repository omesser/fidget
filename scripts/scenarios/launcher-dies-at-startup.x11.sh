#!/usr/bin/env bash
# Scenario: launcher-dies-at-startup (X11)
# On screen: launches Fidget as BMO with a fixture Harness whose first launch
#   aborts before initialize. The tray menu's Chat… row is clicked over
#   dbusmenu, so Chat opens and takes focus. Chat is resized to 420 and 320
#   wide, then Codex is pressed. Four AT-SPI dumps. Fidget quits at the end.
# Input: a dbusmenu click on the tray's Chat… row; an AT-SPI action on Codex;
#   xdotool resizes; no pointer, no keys.
# Duration: about 30 s, 2 min at most.
# Grants: X11 with a tray host (StatusNotifierWatcher), python3-pyatspi, xdotool.
# Asserts: what only a live run can: the tray's Chat… row opens the Harness
#   error landing; at 420 and 320 its Error output and Command boxes end inside
#   the window and Error output starts above the composer; Codex, a re-pick
#   under FIDGET_HARNESS, launches the Harness again at once. Copy, labels and
#   capture are chat-landing-format.test.js's and the Rust tests'.
#
# Usage: launcher-dies-at-startup.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# FIDGET_SCENARIO_AX_FAILED, _AX_420, _AX_320 and _AX_LIVE assert those dumps
# and skip the GUI. That checks the assertion, not the live window.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: launcher-dies-at-startup.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: launcher-dies-at-startup.x11.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../.." && pwd)
dyld="dyld[0]: Library not loaded"

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

rect() { tr , ' ' <<< "${1##*|}"; }                   # <dump line>: its frame as "x y w h"
box() { grep -E '^label\|' "$2" | grep -m1 -F "$1"; } # <text> <dump>

check_failed() { # <dump>
  grep -qF "|Harness couldn't start|" "$1" || fail "Chat shows no Harness error landing; see $1"
  echo "ok: the tray's Chat row opened the Harness error landing"
}

# Fixed strings: the launcher is an absolute path, and a path is not a regex.
check_fits() { # <width> <dump>: both boxes end inside Chat, Error output above the composer
  local w=$1 f=$2 wx ww x y bw fold text row
  row=$(grep -m1 '^frame|' "$f") || fail "$w: no frame line in $f"
  read -r wx _ ww _ <<< "$(rect "$row")"
  [ "$ww" -eq "$w" ] || fail "$w: frame is $ww wide in $f"
  for text in "$dyld" "|$harness|"; do
    row=$(box "$text" "$f") || fail "$w: no box holds '$text' in $f"
    read -r x _ bw _ <<< "$(rect "$row")"
    [ $((x + bw)) -le $((wx + ww + 1)) ] || fail "$w: a box ends at $((x + bw)), past the window edge at $((wx + ww))"
  done
  # The composer covers the log's foot, so the fold is its top, not the window's.
  read -r _ y _ _ <<< "$(rect "$(box "$dyld" "$f")")"
  row=$(grep -m1 '^entry|Nothing can answer yet|' "$f") || fail "$w: no composer in $f"
  read -r _ fold _ _ <<< "$(rect "$row")"
  [ "$y" -lt "$fold" ] || fail "$w: Error output starts at y $y, under the composer at $fold"
  echo "ok: $w: both boxes inside the window, Error output at y $y above the composer at $fold"
}

live_line() { grep -qF -e "|$name · session " -e "|$name · no session yet|" "$1"; }

check_live() { # <dump>
  live_line "$1" || fail "after the re-pick the mind line names no live Harness; see $1"
  echo "ok: the re-pick launched it again at once"
}

if [ -n "${FIDGET_SCENARIO_AX_FAILED:-}${FIDGET_SCENARIO_AX_420:-}${FIDGET_SCENARIO_AX_320:-}${FIDGET_SCENARIO_AX_LIVE:-}" ]; then
  if [ -z "${FIDGET_SCENARIO_AX_FAILED:-}" ] || [ -z "${FIDGET_SCENARIO_AX_420:-}" ] ||
    [ -z "${FIDGET_SCENARIO_AX_320:-}" ] || [ -z "${FIDGET_SCENARIO_AX_LIVE:-}" ]; then
    fail "set FIDGET_SCENARIO_AX_FAILED, FIDGET_SCENARIO_AX_420, FIDGET_SCENARIO_AX_320 and FIDGET_SCENARIO_AX_LIVE"
  fi
  # The launcher line the fixture dumps name.
  harness=${FIDGET_SCENARIO_HARNESS:-/opt/fidget/scripts/scenarios/fixture-harness.sh /opt/fidget/target/debug/deps/fidget-0 script=abort-first count=/tmp/harness.log}
  name=${harness%% *}
  check_failed "$FIDGET_SCENARIO_AX_FAILED"
  check_fits 420 "$FIDGET_SCENARIO_AX_420"
  check_fits 320 "$FIDGET_SCENARIO_AX_320"
  check_live "$FIDGET_SCENARIO_AX_LIVE"
  echo "PASS: fixture dumps"
  exit 0
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 launcher-dies-at-startup needs a session, or set FIDGET_SCENARIO_AX_FAILED, FIDGET_SCENARIO_AX_420, FIDGET_SCENARIO_AX_320 and FIDGET_SCENARIO_AX_LIVE." >&2
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

# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-launcher-dies-at-startup-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home"
log="$out/app.log" marks="$out/harness.log"
: > "$marks"

# `abort-first` prints a dyld line and aborts on its first spawn, then answers.
harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=abort-first count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"
name=${harness%% *}

# No ambient wake: one would respawn the Harness and pass the re-pick for it.
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
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

ax() { python3 "$root/scripts/ax-window-linux.py" "$@"; }
dump() { ax dump "$pid" BMO frames > "$out/$1.ax.txt" 2> "$out/$1.err"; }
spawns() { grep -cx spawn "$marks" || true; }
respawned() { [ "$(spawns)" -ge 2 ]; }
shows_failed() { dump failed && grep -qF "|Harness couldn't start|" "$out/failed.ax.txt"; }
shows_live() { dump live && live_line "$out/live.ax.txt"; }

fits() { # <width>
  local w=$1 got
  ax size "$pid" BMO "$w" 560 > "$out/$w.frame.txt" 2> "$out/$w.frame.err" ||
    fail "$w: resize failed; see $out/$w.frame.err"
  read -r _ _ got _ < <(tr , ' ' < "$out/$w.frame.txt")
  [ "$got" -eq "$w" ] || fail "$w: Chat is $got wide after the resize"
  sleep 1
  dump "$w" || fail "$w: AT-SPI dump failed; see $out/$w.err"
  check_fits "$w" "$out/$w.ax.txt"
}

wait_for 20 grep -q 'exited before initialize' "$log" || fail "the launcher never died before initialize; see $log"
[ "$(spawns)" -eq 1 ] || fail "want one spawn before Chat opens, the fixture saw $(spawns)"
rc=0
ax tray "$pid" BMO Chat > "$out/open.txt" 2>&1 || rc=$?
if [ "$rc" -eq 2 ]; then
  echo "SKIP: no tray host on this session; see $out/open.txt" >&2
  exit 2
fi
[ "$rc" -eq 0 ] || fail "the tray's Chat row did not open Chat; see $out/open.txt"
wait_for 15 shows_failed || fail "Chat shows no Harness error landing; see $out/failed.ax.txt"
check_failed "$out/failed.ax.txt"
for w in 420 320; do fits "$w"; done

[ "$(spawns)" -eq 1 ] || fail "the Harness launched again before the re-pick ($(spawns) spawns)"
ax press "$pid" BMO Codex > "$out/press.txt" 2>&1 || fail "could not press Codex on the landing; see $out/press.txt"
wait_for 10 respawned || fail "the re-pick did not launch the Harness again; see $log"
wait_for 15 shows_live || true
check_live "$out/live.ax.txt"
echo "PASS: evidence in $out"
