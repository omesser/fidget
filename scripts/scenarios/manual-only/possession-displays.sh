#!/usr/bin/env bash
# Scenario: possession-displays (macOS)
# On screen: launches Fidget as BMO and a small fidget-type-target window on
#   the display beside the sprite's. Possesses BMO from its menu and holds an
#   arrow until it walks onto that display, where it walks and jumps it,
#   clicks the target window, clicks the paused card, and presses Escape. Both
#   quit at the end. No screenshots.
# Input: a right-drag onto the Possess row, an arrow held for the walk across
#   (up to about 15 s) and briefly again, Space, two clicks, and Escape. Keep
#   hands off the mouse and keyboard for the run.
# Duration: about 25 s, 60 s at most.
# Grants: Accessibility for the terminal that runs it.
# Asserts: the new display's overlay takes the keyboard once the sprite
#   crosses; the keyup lands there and stops it; an arrow and Space drive it
#   there; a click away pauses it, and a click on the card on that display
#   resumes it; Escape releases possession.
# Skips (exit 2) when no display sits beside the sprite's at its height.
# Usage: possession-displays.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: possession-displays.sh --go <fidget binary>}
root=$(cd "$(dirname "$0")/../../.." && pwd)
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-possession-displays-$(date +%Y%m%d-%H%M%S)-XXXX")
mkdir -p "$out/home" "$tools"
seed_tour_shown "$out/home"
log="$out/app.log" target_log="$out/target.log"

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}
skip() {
  echo "SKIP: $*" >&2
  exit 2
}

# Another scenario may be compiling the same helper, so each build lands whole.
build() { # <source> <name>
  [ "$tools/$2" -nt "$1" ] && return
  swiftc -O "$1" -o "$tools/$2.$$" && mv -f "$tools/$2.$$" "$tools/$2"
}
build "$root/scripts/scenarios/helpers/possession-input.swift" possession-input
build "$root/scripts/scenarios/helpers/type-target.swift" type-target
build "$root/scripts/click-cursor.swift" click-cursor
build "$root/scripts/ax-settings.swift" ax
build_ax_events
. "$root/scripts/scenarios/helpers/possession-live-check.sh"

last_frame() { grep '^frame: ' "$log" | tail -1; }
sprite_x() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),.*/\1/'; }
sprite_xy() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/'; }
frames() { line_count "$log" '^frame: '; }
walked_from() { # <x>
  [ $((($(sprite_x) - $1) * sign)) -ge 8 ]
}
# The sprite's left edge (going right) or right edge (going left) is past the border.
crossed() {
  local x
  x=$(sprite_x)
  if [ "$sign" -gt 0 ]; then [ "$x" -ge "$border" ]; else [ $((x + w)) -le "$border" ]; fi
}
# The newest <n> frames draw the sprite in one spot.
still_for() { # <n>
  grep '^frame: ' "$log" | tail -"$1" | grep -oE 'sprite\(-?[0-9]+,-?[0-9]+\)' |
    awk -v n="$1" '{ seen[$0] = 1; count++ } END { exit !(count == n && length(seen) == 1) }'
}
# A jump frame after frame number <n>.
jumped_after() { # <n>
  grep '^frame: ' "$log" | tail -n +"$(($1 + 1))" | grep -qE ' Falling | jump#'
}
key() { # <virtual key> down|up
  "$tools/possession-input" key "$1" "$2" >> "$out/input.txt" 2>&1 || fail "could not post key $1 $2"
}
click() { # <x> <y>
  "$tools/click-cursor" "$1" "$2" 1 >> "$out/input.txt" 2>&1 || fail "could not click ($1,$2)"
}
# The paused card's centre on the new display, from every overlay's dump: each
# overlay is titled Fidget. Fails until the card shows there.
card_on_next() {
  "$tools/ax" dump "$pid" Fidget frames > "$out/overlays-ax.txt" 2>&1 || return 1
  awk -F'|' -v x="$nx" -v y="$ny" -v w="$nw" -v h="$nh" '
    split($NF, a, ",") == 4 && a[1] >= x && a[1] < x + w && a[2] >= y && a[2] < y + h
  ' "$out/overlays-ax.txt" > "$out/card-ax.txt"
  [ -n "$(card_center "$out/card-ax.txt")" ]
}
# Hold the arrow until the sprite walks 8 px, then let go and wait for rest.
step_arrow() { # <what>
  local x
  x=$(sprite_x)
  key "$arrow" down
  wait_until 3 walked_from "$x" || fail "$1 did not walk the sprite; see $log"
  key "$arrow" up
  wait_until 3 still_for 5 || fail "the sprite kept walking after $1; see $log"
}

right=124 left=123 space=49 escape=53

start_ms=$(now_ms)
env HOME="$out/home" FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_HITTEST=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" "$bin" > "$log" 2>&1 &
# shellcheck disable=SC2034 # read by stop_fidget
pid=$!
target_pid=
# Fidget quits first: it would keep the held keys if a step failed mid-hold.
release_keys() {
  local k
  for k in "$right" "$left" "$space"; do "$tools/possession-input" key "$k" up 2> /dev/null || true; done
}
trap 'stop_fidget; [ -z "$target_pid" ] || kill "$target_pid" 2> /dev/null; release_keys' EXIT
trap 'exit 130' INT TERM

wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_rest "$log" || fail "the sprite never came to rest; see $log"
read -r w h < <(sed -nE 's/^overlay: .* sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r sx sy < <(sprite_xy)
cx=$((sx + w / 2)) cy=$((sy + h / 2))
echo "ok: sprite at ($sx,$sy) size ${w}x${h}"

# Each overlay line is "<label> <x> <y> <w> <h>".
displays=$(sed -nE 's/^overlay: (overlay-[0-9]+) covers ([0-9]+)x([0-9]+) at \((-?[0-9]+),(-?[0-9]+)\)$/\1 \4 \5 \2 \3/p' "$log")
read -r here hx _ hw _ < <(echo "$displays" | awk -v x="$cx" -v y="$cy" '$2 <= x && x < $2 + $4 && $3 <= y && y < $3 + $5' | head -1) ||
  fail "no overlay covers the sprite's centre; see $log"
read -r next nx ny nw nh sign < <(echo "$displays" | awk -v l="$here" -v x="$hx" -v w="$hw" -v y="$cy" '
  $1 != l && $3 <= y && y < $3 + $5 {
    if ($2 + $4 == x) { print $0, -1; exit }
    if ($2 == x + w) { print $0, 1; exit }
  }') || skip "no display sits beside $here at the sprite's height ($(echo "$displays" | wc -l | tr -d ' ') display(s))"
if [ "$sign" -gt 0 ]; then arrow=$right name=right border=$((hx + hw)); else arrow=$left name=left border=$hx; fi
next_index=${next#overlay-}
echo "ok: $next at ($nx,$ny) ${nw}x${nh} is to the $name of $here"

read -r ax ay aw _ < <("$tools/type-target" work-area "$((nx + nw / 2))" "$((ny + nh / 2))") || fail "no work area on $next"
# Opened before possessing: an app that launches later takes the keyboard
# from the overlay. Top right of the new display, clear of the card.
"$tools/type-target" window "$((ax + aw - 380))" "$((ay + 60))" 340 140 > "$target_log" 2>&1 &
target_pid=$!
wait_for_trace 5 "$target_log" '^ready ' || fail "the target window never opened; see $target_log"
read -r tx ty tw th < <(sed -nE 's/^ready //p' "$target_log")

"$tools/possession-input" move "$cx" "$cy" >> "$out/input.txt" 2>&1 || fail "could not move the cursor onto the sprite"
before=$(frames)
wait_for_count 2 "$log" '^frame: ' "$((before + 1))" || fail "no frames after the cursor moved; see $log"
"$tools/possession-input" possess "$pid" "$cx" "$cy" >> "$out/input.txt" 2>&1 || fail "could not press Possess; see $out/input.txt"
wait_for_trace 3 "$log" '^menu: possession on' || fail "the Possess row did not possess the sprite; see $log"
echo "ok: possessed"

next_focus="^overlay $next: window focus"
focuses=$(line_count "$log" "$next_focus")
# Budget the walk at 100 px/s, under the ~175 px/s BMO walks.
if [ "$sign" -gt 0 ]; then distance=$((border - sx)); else distance=$((sx + w - border)); fi
key "$arrow" down
wait_until $((distance / 100 + 3)) crossed || fail "the held $name arrow did not carry the sprite past x=$border; see $log"
# The page's focus line is only a note: AppKit can make the overlay key with
# its web view not yet first responder. The keyup is what proves the keyboard.
wait_for_count 3 "$log" "$next_focus" "$focuses" ||
  echo "note: $next logged no page focus after the crossing" | tee -a "$out/notes.txt"
key "$arrow" up
wait_for_trace 3 "$log" "^possession: key $next $name up" || fail "the $name keyup never reached $next, so $next does not have the keyboard; see $log"
wait_until 3 still_for 5 || fail "the sprite kept walking after the $name keyup; see $log"
read -r sx sy < <(sprite_xy)
[ $((sx + w / 2)) -ge "$nx" ] && [ $((sx + w / 2)) -lt $((nx + nw)) ] || fail "the sprite came to rest at ($sx,$sy), off $next; see $log"
echo "ok: walked onto $next, which took the keyboard, and stopped on the keyup at ($sx,$sy)"

step_arrow "the $name arrow on $next"
key $space down
wait_for_trace 3 "$log" "^possession: key $next space down" || fail "Space never reached $next; see $log"
before=$(frames)
key $space up
wait_until 3 jumped_after "$before" || fail "Space did not jump the sprite on $next; see $log"
wait_until 3 still_for 5 || fail "the sprite never landed after the jump; see $log"
echo "ok: the arrow and Space drive the sprite on $next"

blurs=$(line_count "$log" "^overlay $next: window blur")
click "$((tx + tw / 2))" "$((ty + th / 2))"
wait_for_trace 3 "$target_log" '^focus$' || fail "the target window never became key; see $target_log"
wait_for_count 3 "$log" "^overlay $next: window blur" "$blurs" || fail "$next never lost focus to the target window; see $log"
echo "ok: the click away gave the target window the keyboard"

wait_for_ax 3 "$pid" card_on_next || fail "the paused card did not show on $next; see $out/overlays-ax.txt"
read -r card_x card_y < <(card_center "$out/card-ax.txt")
off_line="^hit-test: .* on overlay $next_index .*click-through OFF"
offs=$(line_count "$log" "$off_line")
"$tools/possession-input" move "$card_x" "$card_y" >> "$out/input.txt" 2>&1 || fail "could not move the cursor onto the card"
# A click before click-through turns off falls through the overlay.
wait_for_count 3 "$log" "$off_line" "$offs" || fail "click-through stayed on over the card on $next; see $log"
focuses=$(line_count "$log" "$next_focus")
click "$card_x" "$card_y"
wait_for_count 3 "$log" "$next_focus" "$focuses" || fail "the card click did not give $next the keyboard; see $log"
step_arrow "the $name arrow after the card click"
echo "ok: the card click resumed on $next"

key $escape down
key $escape up
wait_for_trace 3 "$log" '^possession: escape' || fail "Escape did not release possession; see $log"
echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
