#!/usr/bin/env bash
# Scenario: drive-focus (X11), throwaway probe for #1920
# On screen: nothing on your display. A private Xvfb display with xfwm4,
#   mousepad on a scratch file, and Fidget as a standing BMO with the fixture
#   Harness and FIDGET_PROBE_DRIVE_FOCUS set.
# Input: xdotool key presses, a held key, a click into the editor, a click on
#   the sprite.
# Duration: about 5 s; every wait gives up 3 s after its trigger.
# Grants: Xvfb, xfwm4, mousepad, xdotool, xprop and inotifywait (inotify-tools).
# Asserts, each item on its own so one miss does not hide the rest: with no
#   pill open the overlay takes the keyboard; keydown, keyup and repeat reach
#   it; a click into the editor passes through, focus leaves and typing lands
#   in the editor; a sprite click takes the keyboard back.
#
# Usage: drive-focus.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: drive-focus.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: drive-focus.x11.sh --go <fidget binary> <fidget test binary>}
for tool in Xvfb xfwm4 mousepad xdotool xprop inotifywait stdbuf flock; do
  command -v "$tool" > /dev/null 2>&1 || {
    echo "SKIP: $tool is not installed." >&2
    exit 2
  }
done
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-drive-focus-x11-$(date +%Y%m%d-%H%M%S)-XXXX")
mkdir -p "$out/home/.local/share/fidget" "$out/characters"
log="$out/home/.local/share/fidget/process.log" marks="$out/harness.log" results="$out/results.txt"
: > "$marks"
: > "$log"
: > "$results"
echo '{"first_run_tour_shown": true}' > "$out/home/.local/share/fidget/settings.json"
doc="$out/doc.txt"
printf abc > "$doc"
started=()
failed=0

fail() {
  echo "FAIL: $*" | tee -a "$results" >&2
  echo "evidence in $out" >&2
  exit 1
}
proven() { echo "proven: $*" | tee -a "$results"; }
missed() {
  echo "failed: $*" | tee -a "$results" >&2
  failed=1
}
lock="$out/run.lock"
exec 9> "$lock"
flock -n 9 || fail "another run holds $lock"
cleanup() {
  local tree=() p
  exec 9>&-
  for p in "${started[@]}"; do mapfile -t -O "${#tree[@]}" tree < <(process_tree "$p"); done
  [ "${#tree[@]}" -gt 0 ] || return 0
  kill "${tree[@]}" 2> /dev/null || true
  wait_for_exit 3 "${tree[0]}" "$lock" || kill -9 "${tree[@]}" 2> /dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

exec {ready}< <(
  Xvfb -displayfd 1 -screen 0 1280x800x24 -nolisten tcp 2> "$out/xvfb.log" &
  echo "pid $!"
  wait "$!"
)
read -r -t 3 _ xvfb <&"$ready" || fail "Xvfb did not start"
started+=("$xvfb")
read -r -t 3 n <&"$ready" || fail "Xvfb gave no display in 3 s; see $out/xvfb.log"
export DISPLAY=":$n"

root_events() { stdbuf -oL xprop -root -spy; }
root_prop_matches() { # <property> <ERE>
  wait_on_source 3 root_events grep -qEm1 "^$1\\(.*$2"
}
xfwm4 --compositor=off > "$out/xfwm4.log" 2>&1 &
started+=($!)
root_prop_matches _NET_SUPPORTING_WM_CHECK 'window id' || fail "xfwm4 did not start in 3 s"

HOME="$out/editor-home" mousepad --disable-server "$doc" > "$out/mousepad.log" 2>&1 &
started+=($!)
editor_window() { xdotool search --name "doc\.txt - Mousepad" 2> /dev/null | head -1 | grep .; }
wait_on_source 3 root_events probe_each_line editor_window || fail "mousepad did not map in 3 s"
editor=$(editor_window)
active_is() { root_prop_matches _NET_ACTIVE_WINDOW "window id # $(printf '0x%x' "$1")(,|\$)"; }
xdotool windowactivate "$editor"
active_is "$editor" || fail "mousepad did not take focus"
xdotool windowmove "$editor" 40 40 windowsize "$editor" 600 400

cp -R "$root/characters/bmo" "$out/characters/"
sed -E '/^play = /s/"walk"/"idle"/g' "$root/characters/bmo/character.manifest" > "$out/characters/bmo/character.manifest"

harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=nop count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"
env HOME="$out/home" FIDGET_HARNESS="$harness" FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_PROBE_DRIVE_FOCUS="${FIDGET_PROBE_DRIVE_FOCUS:-1}" \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$out/characters" \
  "$bin" > "$out/app.log" 2>&1 &
pid=$!
started+=("$pid")
exec 9>&-

frame='^[0-9.]+ frame: '
probe='^[0-9.]+ drive-probe: overlay-0 '
wait_for_next() { # <ERE> <command...>
  local pattern=$1 before
  shift
  before=$(grep -cE "$pattern" "$log" || true)
  "$@" >> "$out/helpers.log" 2>&1 || return 1
  wait_for_count 3 "$log" "$pattern" "$before"
}
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
overlay_window() {
  local w
  for w in $(xdotool search --onlyvisible --name '^Fidget$' 2> /dev/null); do
    eval "$(xdotool getwindowgeometry --shell "$w")"
    # shellcheck disable=SC2153 # WIDTH comes from the eval above
    [ "$WIDTH" -gt 100 ] && echo "$w" && return 0
  done
  return 1
}

wait_for_trace 3 "$log" "$frame" || fail "Fidget drew no frame in 3 s; see $log"
wait_for_trace 3 "$log" "$frame.* (Grounded|Perched) " || fail "the Character did not land in 3 s; see $log"
read -r sw sh < <(sed -nE 's/.* overlay: .*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
overlay=$(overlay_window) || fail "no overlay window on $DISPLAY"

# 1. Launch: the overlay takes the keyboard with no pill open.
wait_for_trace 3 "$log" "${probe}request launch ok" || missed "take: no 'request launch ok'; see $log"
if wait_for_trace 3 "$log" "${probe}Focused\\(true\\)" && active_is "$overlay"; then
  proven "take: Focused(true) and _NET_ACTIVE_WINDOW is the overlay"
else
  missed "take: no Focused(true) or the overlay is not active (active: $(xdotool getactivewindow getwindowname 2> /dev/null))"
fi

# 2. Key edges reach the webview.
if wait_for_next "${probe}keydown key=ArrowRight repeat=false" xdotool key Right &&
  wait_for_trace 3 "$log" "${probe}keyup key=ArrowRight"; then
  proven "keys: keydown and keyup for ArrowRight"
else
  missed "keys: no ArrowRight keydown/keyup; see $log"
fi

# 3. A held key: a second press with no release in between.
if wait_for_next "${probe}keydown key=ArrowRight" xdotool keydown Right &&
  wait_for_next "${probe}keydown key=ArrowRight repeat=true" xdotool keydown Right; then
  proven "repeat: a second keydown with no keyup arrives with repeat=true"
else
  missed "repeat: no keydown with repeat=true; see $log"
fi
xdotool keyup Right

# 4. A click into the editor, off the art, passes through and takes focus away.
eval "$(xdotool getwindowgeometry --shell "$editor")"
# shellcheck disable=SC2153 # X, Y, WIDTH and HEIGHT come from the eval above
xdotool mousemove "$((X + WIDTH / 2))" "$((Y + HEIGHT / 2))" click 1
if wait_for_trace 3 "$log" "${probe}Focused\\(false\\)"; then
  proven "pause: Focused(false) after a click on the editor"
else
  missed "pause: no Focused(false) after a click on the editor; see $log"
fi
if active_is "$editor"; then
  xdotool type --delay 15 zz
  text=$(editor_text || true)
  if [[ $text == *zz* ]] && ! grep -qE "${probe}keydown key=z" "$log"; then
    proven "pass-through: the editor took the click and the typing ($text)"
  else
    missed "pass-through: the editor holds \"$text\""
  fi
else
  missed "pass-through: the editor is not active after its click"
fi

# 5. A sprite click takes the keyboard back.
at=$(grep -E "$frame" "$log" | tail -1 | sed -E 's/.* sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/')
read -r sx sy <<< "$at"
before=$(line_count "$log" "${probe}Focused\\(true\\)")
xdotool mousemove "$((sx + sw / 2))" "$((sy + sh / 2))" click 1 mousemove 900 700
wait_for_trace 3 "$log" "${probe}request click ok" || missed "resume: no 'request click ok'; see $log"
if wait_for_count 3 "$log" "${probe}Focused\\(true\\)" "$before" &&
  wait_for_next "${probe}keydown key=ArrowLeft" xdotool key Left; then
  proven "resume: Focused(true) and ArrowLeft reached the overlay after a sprite click"
else
  missed "resume: no Focused(true) or no ArrowLeft after a sprite click (active: $(xdotool getactivewindow getwindowname 2> /dev/null))"
fi

grep -E "$probe" "$log" > "$out/probe-lines.txt" || true
[ "$failed" -eq 0 ] || fail "see $results"
echo "PASS: evidence in $out" | tee -a "$results"
