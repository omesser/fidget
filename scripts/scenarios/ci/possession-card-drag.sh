#!/usr/bin/env bash
# Scenario: possession-card-drag (macOS)
# On screen: launches Fidget as BMO and a small fidget-type-target window at
#   the top right. Possesses the sprite from the menu bar icon's menu, drags
#   the bindings card by its background, clicks the target window, drags the
#   paused card, clicks it to resume, walks the sprite, and possesses again
#   after Escape. Both quit at the end. One screenshot of the moved card,
#   cropped to it.
# Input: a click on the menu bar icon and AXPress on its Possess row, twice;
#   two left-drags on the card, a click on the target window and one on the
#   card, a synthetic arrow and Escape. Keep hands off the mouse and keyboard.
# Duration: about 15 s.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: the card follows a drag, expanded and paused. Focus leaving and
#   coming back does not move it, dragging the paused card does not resume
#   possession, a plain click on the paused card does, and an arrow still
#   walks the sprite. The next possession seats the card in the corner again.
#   Card positions and the paused state come from the FIDGET_TRACE_BUBBLE card
#   line, which the overlay writes on each re-layout.
#
# Usage: possession-card-drag.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: possession-card-drag.sh --go <fidget binary>}
root=$(cd "$(dirname "$0")/../../.." && pwd)
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-possession-card-drag-$(date +%Y%m%d-%H%M%S)-XXXX")
mkdir -p "$out/home" "$tools"
seed_tour_shown "$out/home"
log="$out/app.log" target_log="$out/target.log"

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

# Another scenario may be compiling the same helper, so each build lands whole.
build() { # <source> <name>
  [ "$tools/$2" -nt "$1" ] && return
  swiftc -O "$1" -o "$tools/$2.$$" && mv -f "$tools/$2.$$" "$tools/$2"
}
build "$root/scripts/scenarios/helpers/possession-input.swift" possession-input
build "$root/scripts/scenarios/helpers/type-target.swift" type-target
build "$root/scripts/scenarios/helpers/throw-sprite.swift" throw-sprite
build "$root/scripts/click-cursor.swift" click-cursor
build "$root/scripts/ax-settings.swift" ax

last_frame() { grep '^frame: ' "$log" | tail -1; }
sprite_x() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),.*/\1/'; }
sprite_xy() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/'; }
frames() { line_count "$log" '^frame: '; }
walked_from() {
  local d=$(($(sprite_x) - $1))
  [ "${d#-}" -ge 8 ]
}
key() { # <virtual key> down|up
  "$tools/possession-input" key "$1" "$2" >> "$out/input.txt" 2>&1 || fail "could not post key $1 $2"
}
click() { # <x> <y>
  "$tools/click-cursor" "$1" "$2" 1 >> "$out/input.txt" 2>&1 || fail "could not click ($1,$2)"
}
card_pattern='^overlay [^:]+: possession card at '
cards() { line_count "$log" "${card_pattern}.* paused=$1"; }

# The newest card line in global points: the overlay's own corner plus the
# display origin its covers line names.
card_xy() {
  local line label x y
  line=$(grep -E "$card_pattern" "$log" | tail -1)
  label=$(sed -E 's/^overlay ([^:]+):.*/\1/' <<< "$line")
  read -r x y < <(sed -E 's/.* at \((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/' <<< "$line")
  read -r ox oy < <(sed -nE "s/^overlay: $label covers [0-9]+x[0-9]+ at \((-?[0-9]+),(-?[0-9]+)\)/\1 \2/p" "$log" | tail -1)
  echo "$((x + ${ox:-0})) $((y + ${oy:-0}))"
}
card_at() { [ "$(card_xy)" = "$1 $2" ]; }
paused_now() { grep -E "$card_pattern" "$log" | tail -1 | grep -q "paused=$1"; }
# A press on the heading, clear of the footer's buttons.
drag_card() { # <dx>
  local x y
  read -r x y < <(card_xy)
  "$tools/possession-input" move "$((x + 40))" "$((y + 14))" >> "$out/input.txt" 2>&1 || fail "could not move onto the card"
  wait_for_count 2 "$log" '^frame: ' "$(($(frames) + 1))" || fail "no frames after the cursor moved; see $log"
  # Slow steps, so the press is a drag and not a throw of anything under it.
  "$tools/throw-sprite" "$((x + 40))" "$((y + 14))" "$1" 20 0.02 >> "$out/input.txt" 2>&1 || fail "could not drag the card"
}
# An arrow toward the roomier side, which on a Dock perch can be either.
walk() {
  local x arrow
  x=$(sprite_x)
  if [ $((x + w / 2 - ax)) -lt $((aw / 2)) ]; then arrow=$right; else arrow=$left; fi
  key "$arrow" down
  wait_until 3 walked_from "$x" || fail "a held arrow did not walk the sprite; see $log"
  key "$arrow" up
}
# The menu bar icon's row, not the sprite's menu: a right-click has to find
# the sprite where it walks, and a menu over the Dock can close before its
# row is read. Focus lines are not reliable on macOS 27; the card's paused
# state is what the overlay acts on.
possess() { # <possessions so far>
  "$tools/ax" open "$pid" Possess "" > "$out/possess-$1.txt" 2>&1 || fail "the menu bar icon's Possess row failed; see $out/possess-$1.txt"
  wait_for_count 3 "$log" '^menu: possession on' "$1" || fail "the Possess row did not possess the sprite; see $log"
}

left=123 right=124 escape=53

start_ms=$(now_ms)
env HOME="$out/home" FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" "$bin" > "$log" 2>&1 &
# shellcheck disable=SC2034 # read by stop_fidget
pid=$!
target_pid=
release_arrows() {
  local k
  for k in "$left" "$right"; do "$tools/possession-input" key "$k" up 2> /dev/null || true; done
}
trap 'stop_fidget; [ -z "$target_pid" ] || kill "$target_pid" 2> /dev/null; release_arrows' EXIT
trap 'exit 130' INT TERM

wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_rest "$log" || fail "the sprite never came to rest; see $log"
read -r w h < <(sed -nE 's/^overlay: .* sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r sx sy < <(sprite_xy)
read -r ax ay aw _ < <("$tools/type-target" work-area "$((sx + w / 2))" "$((sy + h / 2))") || fail "no display holds the sprite"

# Opened before possessing: an app that launches later takes the keyboard
# from the overlay. Top right, clear of the card's drags.
"$tools/type-target" window "$((ax + aw - 380))" "$((ay + 60))" 340 140 > "$target_log" 2>&1 &
target_pid=$!
wait_for_trace 5 "$target_log" '^ready ' || fail "the target window never opened; see $target_log"
read -r tx ty tw th < <(sed -nE 's/^ready //p' "$target_log")

possess 0
wait_for_trace 3 "$log" "${card_pattern}.* paused=false" || fail "no live card line; is FIDGET_TRACE_BUBBLE on? see $log"
read -r x0 y0 < <(card_xy)
echo "ok: possessed, card at ($x0,$y0)"

drag_card 300
# throw-sprite's drag also rises 10 points.
x1=$((x0 + 300)) y1=$((y0 - 10 < ay ? ay : y0 - 10))
wait_until 3 card_at "$x1" "$y1" || fail "the card did not follow the drag to ($x1,$y1), it is at ($(card_xy)); see $log"
echo "ok: the expanded card dropped at ($x1,$y1)"

paused=$(cards true)
click "$((tx + tw / 2))" "$((ty + th / 2))"
wait_for_trace 3 "$target_log" '^focus$' || fail "the target window never became key; see $target_log"
wait_for_count 3 "$log" "${card_pattern}.* paused=true" "$paused" || fail "the card never re-laid out as paused; see $log"
card_at "$x1" "$y1" || fail "focus leaving moved the card to ($(card_xy)); see $log"
echo "ok: focus left and the paused card stayed put"

drag_card -150
x2=$((x1 - 150)) y2=$((y1 - 10 < ay ? ay : y1 - 10))
wait_until 3 card_at "$x2" "$y2" || fail "the paused card did not follow the drag to ($x2,$y2), it is at ($(card_xy)); see $log"
paused_now true || fail "dragging the paused card resumed possession; see $log"
grep -qx 'focus' <(tail -1 "$target_log") || fail "the target window lost the keyboard to the drag; see $target_log"
echo "ok: the paused card dropped at ($x2,$y2) and the keyboard stayed away"

live=$(cards false)
click "$((x2 + 40))" "$((y2 + 14))"
wait_for_count 3 "$log" "${card_pattern}.* paused=false" "$live" || fail "a click on the paused card did not resume; see $log"
card_at "$x2" "$y2" || fail "focus coming back moved the card to ($(card_xy)); see $log"
# The card's own region, never the display: evidence can end up on a public PR.
screencapture -x -R "$((x2 - 8)),$((y2 - 8)),360,280" "$out/card-moved.png" || fail "no screenshot; grant Screen Recording"
walk
echo "ok: the click resumed in place, and an arrow walks the sprite"

key $escape down
key $escape up
wait_for_trace 3 "$log" '^possession: escape' || fail "Escape did not end possession; see $log"
possess 1
wait_until 3 card_at "$x0" "$y0" || fail "the next possession seated the card at ($(card_xy)), not ($x0,$y0); see $log"
echo "ok: the next possession seats the card in the corner again"

echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
echo "screenshot: $out/card-moved.png"
