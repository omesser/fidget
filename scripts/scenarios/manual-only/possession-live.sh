#!/usr/bin/env bash
# Scenario: possession-live (macOS)
# On screen: launches Fidget as BMO with a fixture Harness. TextEdit opens on a
#   scratch file. The scenario Control-clicks the sprite, presses Possess,
#   holds Right and Space, clicks TextEdit, clicks the paused card, and presses
#   Escape. Fidget quits when the scenario ends, and so does the TextEdit this
#   run started. No screenshots.
# Input: one Control-click on the sprite, the Possess row, Right and Space
#   held, one click in TextEdit, the letter k, one click on the paused card,
#   and Escape. Keep hands off the mouse and keyboard for the run.
# Duration: about 15 s, 45 s at most.
# Grants: Accessibility and Screen Recording for the terminal that runs it.
# Asserts: Possess gives the overlay the keyboard; holding Right walks the
#   sprite at least 8 px; the click in TextEdit pauses it without launching
#   the held Space; k reaches TextEdit; a click on the paused card returns the
#   keyboard; Escape releases possession. Frame waits go through wait_for_trace.
#
# Usage: possession-live.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# FIDGET_SCENARIO_TRACE and FIDGET_SCENARIO_LETTER check a saved log and the
# editor text, and skip the GUI.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: possession-live.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: possession-live.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../../.." && pwd)
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"
. "$root/scripts/scenarios/helpers/possession-live-check.sh"

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

if [ -n "${FIDGET_SCENARIO_TRACE:-}" ] || [ -n "${FIDGET_SCENARIO_LETTER:-}" ]; then
  if [ -z "${FIDGET_SCENARIO_TRACE:-}" ] || [ -z "${FIDGET_SCENARIO_LETTER:-}" ]; then
    fail "set both FIDGET_SCENARIO_TRACE and FIDGET_SCENARIO_LETTER"
  fi
  check_possession_live "$FIDGET_SCENARIO_TRACE" "$FIDGET_SCENARIO_LETTER" || exit 1
  echo "PASS: fixture trace"
  exit 0
fi

test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-possession-live-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home" "$tools"
seed_tour_shown "$out/home"
log="$out/app.log" marks="$out/harness.log" doc="$out/letter-doc.txt"
: > "$marks"
# A leading word, so TextEdit does not capitalize the letter that proves the hand-off.
printf 'mark ' > "$doc"

# Runs a helper, stopping it after <seconds>, so a stuck osascript cannot hang the run.
capped() { # <seconds> <command...>
  local secs=$1 cmd dog status=0
  shift
  "$@" &
  cmd=$!
  (
    sleep "$secs"
    process_tree "$cmd" | xargs kill 2> /dev/null
  ) 2> /dev/null &
  dog=$!
  wait "$cmd" || status=$?
  process_tree "$dog" | xargs kill 2> /dev/null || true
  return "$status"
}

textedit_pid=
textedit_opened=()
capture_textedit_pid() {
  textedit_pid=$(pgrep -x TextEdit | head -1) || true
  [ -n "$textedit_pid" ] || fail "TextEdit did not start"
}
refuse_running_textedit() {
  ! pgrep -x TextEdit > /dev/null ||
    fail "TextEdit is running. Quit it first: this run opens and quits TextEdit itself."
}

build() { # <source> <name>
  [ "$tools/$2" -nt "$1" ] && return 0
  swiftc -O "$1" -o "$tools/$2.$$" && mv -f "$tools/$2.$$" "$tools/$2"
}
build "$root/scripts/scenarios/helpers/possession-live-mac.swift" possession-live-mac
build "$root/scripts/scenarios/helpers/possession-input.swift" possession-input
build "$root/scripts/click-cursor.swift" click-cursor
[ "$tools/ax" -nt "$root/scripts/ax-settings.swift" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"

harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=nop count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

release_keys() {
  [ -x "$tools/possession-input" ] || return 0
  "$tools/possession-input" key 124 up 2> /dev/null || true
  "$tools/possession-input" key 49 up 2> /dev/null || true
}
cleanup() {
  release_keys
  stop_fidget
  pkill -f "count=$marks" || true
  if [ -n "${textedit_pid:-}" ]; then
    close_opened_documents "$textedit_pid" || true
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

refuse_running_textedit
capped 3 open -g -a TextEdit "$doc" || fail "could not open TextEdit"
textedit_opened+=("$doc")
capture_textedit_pid

start_ms=$(now_ms)
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!

wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_trace 3 "$log" '^frame: .* sprite\(' || fail "Fidget traced no frame; see $log"
wait_for_rest "$log" || fail "the sprite never came to rest; see $log"
read -r w h < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r x y < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$log" | tail -1) || fail "no frame trace in $log"
cx=$((x + w / 2))
cy=$((y + h / 2))
echo "ok: sprite at ($x,$y) size ${w}x${h}, centre ($cx,$cy)"

"$tools/possession-live-mac" possess "$pid" "$cx" "$cy" > "$out/menu.txt" 2>&1 ||
  fail "could not press Possess; see $out/menu.txt"
wait_for_trace 3 "$log" '^menu: possession on' || fail "the Possess row did not possess the sprite; see $log"
wait_for_trace 3 "$log" '^overlay [^:]+: window focus' || fail "the possessed overlay never took focus; see $log"
echo "ok: possessed, and the overlay has the keyboard"

"$tools/possession-input" key 124 down >> "$out/keys.txt" 2>&1 || fail "could not hold Right"
"$tools/possession-input" key 49 down >> "$out/keys.txt" 2>&1 || fail "could not hold Space"
wait_for_trace 3 "$log" '^frame: .* walk#' || fail "the held Right did not walk the sprite; see $log"

blurs=$(line_count "$log" '^overlay [^:]+: window blur')
capped 3 open -a TextEdit "$doc" || fail "could not bring TextEdit forward"
read -r tx ty tw th < <("$tools/possession-live-mac" window TextEdit) || fail "TextEdit has no window"
"$tools/click-cursor" "$((tx + tw / 2))" "$((ty + th / 2))" 1 >> "$out/keys.txt" 2>&1 ||
  fail "could not click TextEdit"
wait_for_count 3 "$log" '^overlay [^:]+: window blur' "$blurs" || fail "the overlay never lost focus to TextEdit; see $log"
wait_until 3 sprite_at_rest "$log" || fail "the sprite kept walking after TextEdit took focus; see $log"
check_possession_live "$log" "$doc" pause || fail "the pause check failed; see $log"
release_keys

"$tools/ax" type "$textedit_pid" k >> "$out/keys.txt" 2>&1 || fail "could not type into TextEdit"
"$tools/ax" value "$textedit_pid" > "$out/letter.txt" 2>> "$out/keys.txt" || fail "could not read TextEdit; see $out/keys.txt"
grep -q '[Kk]' "$out/letter.txt" || fail "the letter did not reach TextEdit (it holds $(cat "$out/letter.txt"))"

"$tools/ax" dump "$pid" Fidget frames > "$out/card.txt" 2>&1 || fail "could not read the paused card; see $out/card.txt"
read -r card_x card_y < <(card_center "$out/card.txt") || fail "the paused card did not appear; see $out/card.txt"
echo "ok: paused card at ($card_x,$card_y)"
focuses=$(line_count "$log" '^overlay [^:]+: window focus')
"$tools/click-cursor" "$card_x" "$card_y" 1 >> "$out/keys.txt" 2>&1 || fail "could not click the card"
wait_for_count 3 "$log" '^overlay [^:]+: window focus' "$focuses" || fail "the card click did not resume; see $log"
echo "ok: the card click resumed"

"$tools/possession-input" key 53 down >> "$out/keys.txt" 2>&1 || fail "could not press Escape"
"$tools/possession-input" key 53 up >> "$out/keys.txt" 2>&1 || fail "could not release Escape"
wait_for_trace 3 "$log" '^possession: escape' || fail "Escape did not release possession; see $log"
check_possession_live "$log" "$out/letter.txt" || fail "the possession check failed; see $log"

echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
