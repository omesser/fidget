#!/usr/bin/env bash
# Scenario: possession-keyup (macOS)
# On screen: launches Fidget as BMO with the Static Director on one display,
#   possesses it from its menu, and walks it with an arrow key. Finder comes to
#   the front once, mid-walk. Fidget quits when the scenario ends. No
#   screenshots.
# Input: a right-click on the sprite, a press on the menu's Possess row,
#   synthetic arrow key edges, `open -a Finder`, and one click back on the
#   sprite. Keep hands off the mouse and keyboard for the run.
# Duration: about 10 s.
# Grants: Accessibility for the terminal that runs it.
# Asserts: a held arrow walks the possessed sprite and its keyup stops it at
#   once, for 30 frames. Then, with the arrow held, Finder takes focus: the
#   walk stops, the keyup lands in Finder, and once a click hands the overlay
#   focus back the sprite still stands, so no key is left stuck.
#
# Usage: possession-keyup.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: possession-keyup.sh --go <fidget binary>}
root=$(cd "$(dirname "$0")/../../.." && pwd)
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-possession-keyup-$(date +%Y%m%d-%H%M%S)-XXXX")
mkdir -p "$out/home" "$tools"
seed_tour_shown "$out/home"
log="$out/app.log"

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
build "$root/scripts/click-cursor.swift" click-cursor

last_frame() { grep '^frame: ' "$log" | tail -1; }
sprite_x() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),.*/\1/'; }
sprite_xy() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/'; }
frames() { line_count "$log" '^frame: '; }
# The sprite has gone 8 px from <x> toward <sign>.
walked_from() { # <x> <sign: 1 right, -1 left>
  [ $((($(sprite_x) - $1) * $2)) -ge 8 ]
}
# The newest <n> frames draw the sprite in one spot.
still_for() { # <n>
  grep '^frame: ' "$log" | tail -"$1" | grep -oE 'sprite\(-?[0-9]+,-?[0-9]+\)' |
    awk -v n="$1" '{ seen[$0] = 1; count++ } END { exit !(count == n && length(seen) == 1) }'
}
key() { "$tools/possession-input" key "$arrow" "$1" >> "$out/input.txt" 2>&1 || fail "could not post the arrow $1"; }
# A held key walks the sprite away from <x>; fail within 3 s if it does not.
expect_walk() { # <x>
  wait_until 3 walked_from "$1" "$sign" || fail "the held arrow did not walk the sprite; see $log"
}
# Stopped within 1 s, then the next 30 frames hold that spot.
expect_stand() { # <what stopped it>
  wait_until 1 still_for 5 || fail "the sprite kept walking after $1; see $log"
  local x before
  x=$(sprite_x) before=$(frames)
  wait_for_count 2 "$log" '^frame: ' "$((before + 29))" || fail "fewer than 30 frames after $1; see $log"
  if [ "$(sprite_x)" != "$x" ] || ! still_for 30; then
    fail "the sprite moved again after $1, a stuck key; see $log"
  fi
}

start_ms=$(now_ms)
env HOME="$out/home" FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" "$bin" > "$log" 2>&1 &
# shellcheck disable=SC2034 # read by stop_fidget
pid=$!
# Fidget quits first: it would keep Finder's arrow keys if a step failed mid-hold.
trap 'stop_fidget; "$tools/possession-input" key "${arrow:-124}" up 2> /dev/null || true' EXIT
trap 'exit 130' INT TERM

wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_rest "$log" || fail "the sprite never came to rest; see $log"
read -r w h < <(sed -nE 's/^overlay: .* sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r dx dw < <(sed -nE 's/^overlay: .* covers ([0-9]+)x[0-9]+ at \((-?[0-9]+),-?[0-9]+\)/\2 \1/p' "$log" | head -1) || fail "no display bounds in $log"
read -r sx sy < <(sprite_xy)
cx=$((sx + w / 2)) cy=$((sy + h / 2))
# Walk toward the roomier side of the display: 124 is Right, 123 Left.
if [ $((cx - dx)) -lt $((dw / 2)) ]; then arrow=124 sign=1; else arrow=123 sign=-1; fi

focuses=$(line_count "$log" '^overlay [^:]+: window focus')
"$tools/possession-input" possess "$pid" "$cx" "$cy" >> "$out/input.txt" 2>&1 || fail "could not press Possess; see $out/input.txt"
wait_for_trace 3 "$log" '^menu: possession on' || fail "the Possess row did not possess the sprite; see $log"
wait_for_count 3 "$log" '^overlay [^:]+: window focus' "$focuses" || fail "the possessed overlay never took focus; see $log"
echo "ok: possessed, and the overlay has the keyboard"

key down
expect_walk "$(sprite_x)"
echo "ok: the held arrow walks the sprite"
key up
expect_stand "the keyup"
echo "ok: the keyup stopped the walk at once"

key down
expect_walk "$(sprite_x)"
blurs=$(line_count "$log" '^overlay [^:]+: window blur')
open -a Finder || fail "could not bring Finder to the front"
wait_for_count 3 "$log" '^overlay [^:]+: window blur' "$blurs" || fail "the overlay never lost focus to Finder; see $log"
expect_stand "Finder took focus"
key up
echo "ok: focus left mid-walk, the walk stopped, and the keyup went to Finder"

focuses=$(line_count "$log" '^overlay [^:]+: window focus')
read -r sx sy < <(sprite_xy)
"$tools/click-cursor" "$((sx + w / 2))" "$((sy + h / 2))" 1 >> "$out/input.txt" 2>&1 || fail "could not click the sprite; see $out/input.txt"
wait_for_count 3 "$log" '^overlay [^:]+: window focus' "$focuses" || fail "the click did not hand the overlay focus back; see $log"
expect_stand "focus came back"
echo "ok: back on the overlay, the sprite still stands: no stuck key"

echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
