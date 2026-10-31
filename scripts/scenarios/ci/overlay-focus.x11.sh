#!/usr/bin/env bash
# Scenario: overlay-focus (X11)
# On screen: nothing on your display. A private 800x600 Xvfb display with
#   icewm, as the AppImage catalog test runs it, and Fidget as a standing BMO
#   with the fixture Harness.
# Input: xdotool cursor moves and Escape.
# Duration: about 2 s; every wait gives up 3 s after its trigger.
# Grants: Xvfb, icewm, xdotool, xprop and the fidget test binary.
# Asserts: once the overlay has drawn, it refuses focus and is not the active
#   window (#1699); opening the pill lets it take focus; closing the pill
#   makes it refuse focus again.
#
# Usage: overlay-focus.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: overlay-focus.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: overlay-focus.x11.sh --go <fidget binary> <fidget test binary>}
for tool in Xvfb icewm xdotool xprop stdbuf flock; do
  command -v "$tool" > /dev/null 2>&1 || {
    echo "SKIP: $tool is not installed." >&2
    exit 2
  }
done
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-overlay-focus-x11-$(date +%Y%m%d-%H%M%S)-XXXX")
mkdir -p "$out/home/.local/share/fidget" "$out/characters" "$out/icewm-home"
log="$out/home/.local/share/fidget/process.log" marks="$out/harness.log" results="$out/results.txt"
: > "$marks"
: > "$log"
: > "$results"
echo '{"first_run_tour_shown": true}' > "$out/home/.local/share/fidget/settings.json"
started=()

fail() {
  echo "FAIL: $*" | tee -a "$results" >&2
  echo "evidence in $out" >&2
  exit 1
}
ok() { echo "ok: $*" | tee -a "$results"; }
lock="$out/run.lock"
exec 9> "$lock"
flock -n 9 || fail "another run holds $lock"
# TERM to the tree of each process this run started, up to 3 s for all of them
# to exit, then kill -9 on what is left of those trees. Only by PID.
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

# The catalog's size (AppImage/appimage.github.io#10467).
exec {ready}< <(
  Xvfb -displayfd 1 -screen 0 800x600x24 -nolisten tcp 2> "$out/xvfb.log" &
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
# Its own HOME, so a user's icewm preferences don't change focus rules.
HOME="$out/icewm-home" icewm > "$out/icewm.log" 2>&1 &
started+=($!)
root_prop_matches _NET_SUPPORTING_WM_CHECK 'window id' || fail "icewm did not start in 3 s"

cp -R "$root/characters/bmo" "$out/characters/"
sed -E '/^play = /s/"walk"/"idle"/g' "$root/characters/bmo/character.manifest" > "$out/characters/bmo/character.manifest"
harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=slow count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"
start_ms=$(now_ms)
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
wait_for_next() { # <ERE> <command...>
  local pattern=$1 before
  shift
  before=$(grep -cE "$pattern" "$log" || true)
  "$@" >> "$out/helpers.log" 2>&1 || fail "could not run $*"
  wait_for_count 3 "$log" "$pattern" "$before"
}
# The landing gets its own 3 s after the first frame.
wait_for_trace 3 "$log" "$frame" || fail "Fidget drew no frame in 3 s; see $log"
wait_for_trace 3 "$log" "$frame.* (Grounded|Perched) " || fail "the Character did not land in 3 s; see $log"
# The overlay reports its pill once its page has loaded, after its first draw.
wait_for_trace 3 "$log" "$closed" || fail "the overlay never reported its pill in 3 s; see $log"
read -r sw sh < <(sed -nE 's/.* overlay: .*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"

# The display-sized "Fidget" window; GTK's client leader and the taskbar
# anchor share the title but are one pixel.
overlay_window() {
  local w
  for w in $(xdotool search --onlyvisible --name '^Fidget$' 2> /dev/null); do
    eval "$(xdotool getwindowgeometry --shell "$w")"
    # shellcheck disable=SC2153 # WIDTH comes from the eval above
    [ "$WIDTH" -gt 100 ] && echo "$w" && return 0
  done
  return 1
}
overlay=$(overlay_window) || fail "no overlay window on $DISPLAY"
# WM_HINTS input is what a window manager reads to give a window focus.
accepts_focus() { xprop -id "$overlay" WM_HINTS | grep -q "accepts input or input focus: $1"; }
active_window() { xprop -root _NET_ACTIVE_WINDOW | sed -nE 's/.*window id # (0x[0-9a-f]+).*/\1/p'; }
overlay_active() { [ "$(active_window)" = "$(printf '0x%x' "$overlay")" ]; }

# 1. Startup: the overlay has drawn and refuses focus, so icewm leaves it inactive.
accepts_focus False || fail "startup: the overlay accepts focus once drawn ($(xprop -id "$overlay" WM_HINTS | tr -s '\t\n' ' '))"
! overlay_active || fail "startup: the overlay is the active window"
ok "startup: the overlay refuses focus and is not active"

# 2. The pill opens on hover and the overlay takes focus for its typing.
at=$(grep -E "$frame" "$log" | tail -1 | sed -E 's/.* sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/')
read -r sx sy <<< "$at"
cx=$((sx + sw / 2)) cy=$((sy + sh / 2))
xdotool mousemove "$((cx + 300))" "$cy"
wait_for_next "$draft\"\" focused=true" xdotool mousemove "$cx" "$cy" || fail "pill-open: the pill did not open; see $log"
accepts_focus True || fail "pill-open: the overlay refuses focus with the pill open"
ok "pill-open: the overlay accepts focus"

# 3. Escape closes the pill and the overlay refuses focus again.
wait_for_next "$closed" xdotool key Escape || fail "pill-close: the pill did not close; see $log"
wait_until 3 accepts_focus False || fail "pill-close: the overlay still accepts focus"
ok "pill-close: the overlay refuses focus again"

echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)" | tee -a "$results"
