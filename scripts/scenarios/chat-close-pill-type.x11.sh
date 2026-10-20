#!/usr/bin/env bash
# Scenario: chat-close-pill-type (X11)
# On screen: nothing on your display. A private Xvfb display with xfwm4,
#   mousepad on a scratch file, and Fidget as a standing BMO with the fixture
#   Harness.
# Input: xdotool cursor moves, clicks, typed words, Enter, Escape, Ctrl+S.
# Duration: about 4 s; every wait gives up 3 s after its trigger.
# Grants: Xvfb, xfwm4, mousepad, xdotool, xprop and inotifywait (inotify-tools).
# Asserts: a sprite click leaves typing in the editor; the pill opens on hover
#   and takes typing with no click (#1565); Enter and Escape hand typing back
#   to the editor; a click into the editor keeps the pill's text while typing
#   goes to the editor.
#
# Usage: chat-close-pill-type.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: chat-close-pill-type.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: chat-close-pill-type.x11.sh --go <fidget binary> <fidget test binary>}
for tool in Xvfb xfwm4 mousepad xdotool xprop inotifywait stdbuf flock; do
  command -v "$tool" > /dev/null 2>&1 || {
    echo "SKIP: $tool is not installed." >&2
    exit 2
  }
done
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.x11.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-chat-close-pill-type-x11-$(date +%Y%m%d-%H%M%S)-XXXX")
mkdir -p "$out/home/.local/share/fidget" "$out/characters"
log="$out/home/.local/share/fidget/process.log" marks="$out/harness.log" results="$out/results.txt"
: > "$marks"
: > "$log"
: > "$results"
echo '{"first_run_tour_shown": true}' > "$out/home/.local/share/fidget/settings.json"
doc="$out/doc.txt"
printf abc > "$doc"
started=()

fail() {
  echo "FAIL: $*" | tee -a "$results" >&2
  echo "evidence in $out" >&2
  exit 1
}
ok() { echo "ok: $*" | tee -a "$results"; }
# What this run starts inherits fd 9 and holds the lock until it and its
# children exit; this shell lets go once Fidget is up.
lock="$out/run.lock"
exec 9> "$lock"
flock -n 9 || fail "another run holds $lock"
cleanup() {
  local tree=() p
  exec 9>&-
  for p in "${started[@]}"; do mapfile -t -O "${#tree[@]}" tree < <(process_tree "$p"); done
  [ "${#tree[@]}" -gt 0 ] || return 0
  kill "${tree[@]}" 2> /dev/null || true
  # The lock, not one PID, says when every process that inherited it is gone.
  wait_for_exit 3 "${tree[0]}" "$lock" || kill -9 "${tree[@]}" 2> /dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# -displayfd picks a free display and writes its number once it accepts clients.
exec {ready}< <(
  Xvfb -displayfd 1 -screen 0 1280x800x24 -nolisten tcp 2> "$out/xvfb.log" &
  echo "pid $!"
  wait
)
read -r -t 3 _ xvfb <&"$ready" || fail "Xvfb did not start"
started+=("$xvfb")
read -r -t 3 n <&"$ready" || fail "Xvfb gave no display in 3 s; see $out/xvfb.log"
export DISPLAY=":$n"

# xprop -spy prints the root's properties now and on each change: an event
# stream. Unfiltered, since a property named before its atom exists never matches.
root_events() { stdbuf -oL xprop -root -spy; }
root_prop_matches() { # <property> <ERE>
  wait_on_source 3 root_events grep -qEm1 "^$1\\(.*$2"
}
xfwm4 --compositor=off > "$out/xfwm4.log" 2>&1 &
started+=($!)
root_prop_matches _NET_SUPPORTING_WM_CHECK 'window id' || fail "xfwm4 did not start in 3 s"

HOME="$out/editor-home" mousepad --disable-server "$doc" > "$out/mousepad.log" 2>&1 &
started+=($!)
# Searched again on each root change, _NET_CLIENT_LIST among them.
editor_window() { xdotool search --name "doc\.txt - Mousepad" 2> /dev/null | head -1 | grep .; }
wait_on_source 3 root_events probe_each_line editor_window || fail "mousepad did not map in 3 s"
editor=$(editor_window)
active_is() { root_prop_matches _NET_ACTIVE_WINDOW "window id # $(printf '0x%x' "$1")(,|\$)"; }
xdotool windowactivate "$editor"
active_is "$editor" || fail "mousepad did not take focus"

mkdir -p "$out/characters"
cp -R "$root/characters/bmo" "$out/characters/"
sed -E '/^play = /s/"walk"/"idle"/g' "$root/characters/bmo/character.manifest" > "$out/characters/bmo/character.manifest"

harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=slow count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"
env HOME="$out/home" FIDGET_HARNESS="$harness" FIDGET_DIRECTOR_WAKE_SECS=600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 FIDGET_HOVER_DELAY_MS=300 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$out/characters" \
  "$bin" > "$out/app.log" 2>&1 &
pid=$!
started+=("$pid")
exec 9>&-

frame='^[0-9]+ frame: '
draft='^[0-9]+ overlay: draft .*text='
closed='^[0-9]+ overlay: draft .* closed$'
# Runs <command...> as the trigger, then waits for one more <ERE> line in the log.
wait_for_next() { # <ERE> <command...>
  local pattern=$1 before
  shift
  before=$(grep -cE "$pattern" "$log" || true)
  "$@" >> "$out/helpers.log" 2>&1 || fail "could not run $*"
  wait_for_count 3 "$log" "$pattern" "$before"
}
wait_for_trace 3 "$log" "$frame.* (Grounded|Perched) " || fail "the Character did not land in 3 s; see $log"
read -r sw sh < <(sed -nE 's/.* overlay: .*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
sprite_center() {
  local at
  at=$(grep -E "$frame" "$log" | tail -1 | sed -E 's/.* sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/')
  read -r sx sy <<< "$at"
  cx=$((sx + sw / 2)) cy=$((sy + sh / 2))
}
# The editor's text: Ctrl+S to the focused editor, then the save's rename over the doc.
editor_text() {
  active_is "$editor" || return 1
  local w line
  exec {w}< <(timeout 3 inotifywait -e moved_to --include '/doc\.txt$' "$(dirname "$doc")" 2>&1)
  while read -r -t 3 line <&"$w" && [ "$line" != "Watches established." ]; do :; done
  [ "$line" = "Watches established." ] || {
    exec {w}<&-
    return 1
  }
  xdotool key ctrl+s
  read -r -t 3 line <&"$w" || {
    exec {w}<&-
    return 1
  }
  exec {w}<&-
  cat "$doc"
}
# Fidget samples the buttons each frame: the release waits for its press trace.
# [<x> <y>] moves the pointer there with the release, off the sprite's dwell.
click_at() { # <x> <y> [<x> <y>]
  local before
  before=$(line_count "$log" '^[0-9]+ click: down ')
  xdotool mousemove "$1" "$2" mousedown 1
  wait_for_count 3 "$log" '^[0-9]+ click: down ' "$before" || return 1
  xdotool mouseup 1 ${3:+mousemove "$3" "$4"}
}
open_pill() { # <case>
  sprite_center
  xdotool mousemove "$((cx + 400))" "$cy"
  wait_for_next "$draft\"\" focused=true" xdotool mousemove "$cx" "$cy" || fail "$1: the pill did not open; see $log"
}
type_into_pill() { # <word> <case>
  wait_for_next "$draft\"[^\"]*$1\"" xdotool type --delay 15 "$1" || fail "$2: \"$1\" never reached the pill; see $log"
  ok "$2: the pill took \"$1\" with no click"
}
typing_back_in_editor() { # <word> <case> <expected text>
  active_is "$editor" || fail "$2: the editor did not get focus back (active: $(xdotool getactivewindow getwindowname 2> /dev/null))"
  xdotool type --delay 15 "$1"
  [ "$(editor_text)" = "$3" ] || fail "$2: the editor holds \"$(cat "$doc")\", not \"$3\""
  ! grep -qE "$draft\"[^\"]*$1\"" "$log" || fail "$2: the pill took \"$1\"; see $log"
  ok "$2: typing went back to the editor"
}

# 1. No pill open: a sprite click leaves typing in the editor.
# Readiness: the click waits for the overlay's first draft report. Related,
# unconfirmed: https://github.com/omesser/fidget/issues/1578
wait_for_trace 3 "$log" "$closed" || fail "sprite-click: the overlay never reported its pill in 3 s; see $log"
sprite_center
click_at "$cx" "$cy" "$((cx + 400))" "$cy" || fail "sprite-click: Fidget saw no press; see $log"
wait_for_trace 3 "$log" '^[0-9]+ click: up ' || fail "sprite-click: Fidget saw no click; see $log"
xdotool type --delay 15 four
[ "$(editor_text)" = fourabc ] || fail "sprite-click: the editor holds \"$(cat "$doc")\", not fourabc (active: $(xdotool getactivewindow getwindowname 2> /dev/null))"
! grep -qE "$draft\"[^\"]*four\"" "$log" || fail "sprite-click: the pill took typing; see $log"
ok "sprite-click: typing stayed in the editor"

# 2. Hover opens the pill, which takes typing; Enter sends it and hands typing back.
open_pill enter
type_into_pill one enter
wait_for_next "$closed" xdotool key Return || fail "enter: the pill did not close; see $log"
typing_back_in_editor six enter foursixabc

# 3. Escape closes the pill and hands typing back.
open_pill escape
type_into_pill seven escape
wait_for_next "$closed" xdotool key Escape || fail "escape: the pill did not close; see $log"
typing_back_in_editor eight escape foursixeightabc

# 4. A click into the editor, below its last line, keeps the pill's text while
# the caret moves to the end: "ten" lands there, not where the caret was.
open_pill click-away
type_into_pill nine click-away
eval "$(xdotool getwindowgeometry --shell "$editor")"
mark=$(wc -l < "$log")
kept="$draft\"[Nn]ine\" focused=false"
before=$(line_count "$log" "$kept")
# shellcheck disable=SC2153 # X, Y, WIDTH and HEIGHT come from the eval above
click_at "$((X + WIDTH / 2))" "$((Y + HEIGHT * 7 / 10))" || fail "click-away: Fidget saw no press; see $log"
wait_for_count 3 "$log" "$kept" "$before" || fail "click-away: the pill did not keep \"nine\" after the click; see $log"
typing_back_in_editor ten click-away foursixeightabcten
! tail -n +"$((mark + 1))" "$log" | grep -qE "$closed" || fail "click-away: the pill closed after the click; see $log"
ok "click-away: the pill kept \"nine\" and typing went to the click"

echo "PASS: evidence in $out" | tee -a "$results"
