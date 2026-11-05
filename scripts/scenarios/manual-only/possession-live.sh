#!/usr/bin/env bash
# Scenario: possession-live (macOS)
# On screen: launches Fidget as BMO and a small fidget-type-target window at
#   the top right. Possesses the sprite from its menu, walks it right with
#   Space held, clicks the target window and types there, clicks the card,
#   walks again and presses Escape. Both quit at the end. No screenshots.
# Input: a right-drag from the sprite onto the Possess row, synthetic Right,
#   Space, K and Escape, a click on the target window and one on the card.
#   Keep hands off the mouse and keyboard for the run.
# Duration: about 15 s.
# Grants: Accessibility for the terminal that runs it.
# Asserts: Right walks the possessed sprite. With Right and Space held, a
#   click on another window pauses it: the walk stops for 30 frames, no jump
#   launches, and a typed k reaches that window. A click on the paused card
#   resumes, Right walks again, and Escape ends possession.
#
# Usage: possession-live.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# FIDGET_SCENARIO_TRACE and FIDGET_SCENARIO_LETTER skip the GUI.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
root=$(cd "$(dirname "$0")/../../.." && pwd)
if [ -n "${FIDGET_SCENARIO_TRACE:-}" ] || [ -n "${FIDGET_SCENARIO_LETTER:-}" ]; then
  . "$root/scripts/scenarios/helpers/possession-live-check.sh"
  if [ -z "${FIDGET_SCENARIO_TRACE:-}" ] || [ -z "${FIDGET_SCENARIO_LETTER:-}" ]; then
    echo "FAIL: set both FIDGET_SCENARIO_TRACE and FIDGET_SCENARIO_LETTER" >&2
    exit 1
  fi
  check_possession_live "$FIDGET_SCENARIO_TRACE" "$FIDGET_SCENARIO_LETTER" || exit 1
  echo "PASS: fixture trace"
  exit 0
fi
bin=${2:?usage: possession-live.sh --go <fidget binary>}
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-possession-live-$(date +%Y%m%d-%H%M%S)-XXXX")
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
build "$root/scripts/click-cursor.swift" click-cursor

last_frame() { grep '^frame: ' "$log" | tail -1; }
sprite_x() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),.*/\1/'; }
sprite_xy() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/'; }
frames() { line_count "$log" '^frame: '; }
walked_right_from() { [ $(($(sprite_x) - $1)) -ge 8 ]; }
# The newest <n> frames draw the sprite in one spot.
still_for() { # <n>
  grep '^frame: ' "$log" | tail -"$1" | grep -oE 'sprite\(-?[0-9]+,-?[0-9]+\)' |
    awk -v n="$1" '{ seen[$0] = 1; count++ } END { exit !(count == n && length(seen) == 1) }'
}
key() { # <virtual key> down|up
  "$tools/possession-input" key "$1" "$2" >> "$out/input.txt" 2>&1 || fail "could not post key $1 $2"
}
click() { # <x> <y>
  "$tools/click-cursor" "$1" "$2" 1 >> "$out/input.txt" 2>&1 || fail "could not click ($1,$2)"
}
focuses() { line_count "$log" '^overlay [^:]+: window focus'; }
# The keys prove focus. The overlay's focus line is noted, with the app in
# front, so a failed key step says where the keyboard went.
note_focus() { # <count before> <what>
  wait_for_count 3 "$log" '^overlay [^:]+: window focus' "$1" ||
    echo "note: no overlay focus line after $2; frontmost pid $("$tools/type-target" frontmost), fidget $pid" | tee -a "$out/notes.txt"
}
expect_walk() {
  local x
  x=$(sprite_x)
  wait_until 3 walked_right_from "$x" || fail "the held Right did not walk the sprite; see $log"
}

right=124 space=49 letter=40 escape=53

start_ms=$(now_ms)
env HOME="$out/home" FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_HITTEST=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" "$bin" > "$log" 2>&1 &
# shellcheck disable=SC2034 # read by stop_fidget
pid=$!
target_pid=
# Fidget quits first: it would keep the held keys if a step failed mid-hold.
release_keys() {
  local k
  for k in "$right" "$space"; do "$tools/possession-input" key "$k" up 2> /dev/null || true; done
}
trap 'stop_fidget; [ -z "$target_pid" ] || kill "$target_pid" 2> /dev/null; release_keys' EXIT
trap 'exit 130' INT TERM

wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_rest "$log" || fail "the sprite never came to rest; see $log"
read -r w h < <(sed -nE 's/^overlay: .* sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r sx sy < <(sprite_xy)
cx=$((sx + w / 2)) cy=$((sy + h / 2))
read -r ax ay aw _ < <("$tools/type-target" work-area "$cx" "$cy") || fail "no display holds the sprite at ($cx,$cy)"
# The card sits at the work area's top left; walking right needs room.
[ $((ax + aw - sx - w)) -ge 120 ] || fail "the sprite rests too near the right edge to walk right; see $log"

# Opened before possessing: an app that launches later takes the keyboard
# from the overlay. Top right, clear of the card and the sprite below.
"$tools/type-target" window "$((ax + aw - 380))" "$((ay + 60))" 340 140 > "$target_log" 2>&1 &
target_pid=$!
wait_for_trace 5 "$target_log" '^ready ' || fail "the target window never opened; see $target_log"
read -r tx ty tw th < <(sed -nE 's/^ready //p' "$target_log")

"$tools/possession-input" move "$cx" "$cy" >> "$out/input.txt" 2>&1 || fail "could not move the cursor onto the sprite"
before=$(frames)
wait_for_count 2 "$log" '^frame: ' "$((before + 1))" || fail "no frames after the cursor moved; see $log"
focused=$(focuses)
"$tools/possession-input" possess "$pid" "$cx" "$cy" >> "$out/input.txt" 2>&1 || fail "could not press Possess; see $out/input.txt"
wait_for_trace 3 "$log" '^menu: possession on' || fail "the Possess row did not possess the sprite; see $log"
note_focus "$focused" "Possess"
echo "ok: possessed"

key $right down
expect_walk
key $space down
echo "ok: Right walks the sprite, and Space is held"

paused_at=$(frames)
click "$((tx + tw / 2))" "$((ty + th / 2))"
wait_for_trace 3 "$target_log" '^focus$' || fail "the target window never became key; see $target_log"
wait_until 1 still_for 5 || fail "the sprite kept walking after focus left; see $log"
before=$(frames)
wait_for_count 2 "$log" '^frame: ' "$((before + 29))" || fail "fewer than 30 frames after the pause; see $log"
still_for 30 || fail "the sprite moved while paused, a stuck key; see $log"
! tail -n +"$((paused_at + 1))" <(grep '^frame: ' "$log") | grep -qE '^frame: [0-9]+ Falling' ||
  fail "a jump launched into the pause; see $log"
key $space up
key $right up
key $letter down
key $letter up
wait_for_trace 3 "$target_log" '^key: k$' || fail "the k never reached the target window; see $target_log"
echo "ok: focus left, the sprite paused without a jump, and k reached the other window"

focused=$(focuses)
click "$((ax + 16 + 24))" "$((ay + 16 + 12))"
note_focus "$focused" "the card click"
key $right down
expect_walk
key $right up
echo "ok: the card click resumed, and Right walks again"

key $escape down
key $escape up
wait_for_trace 3 "$log" '^possession: escape' || fail "Escape did not end possession; see $log"
echo "ok: Escape ended possession"

echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
