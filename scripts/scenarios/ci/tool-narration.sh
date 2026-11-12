#!/usr/bin/env bash
# Scenario: tool-narration (macOS)
# On screen: launches Fidget as BMO with a fixture Harness that replies the way
#   grok does: planning, a tool call, then `behavior: walk` with the dialogue
#   on its line, and no `messageId`. Chat opens by itself. One screenshot of
#   the Chat window. Fidget quits when the scenario ends.
# Input: none.
# Duration: a few seconds, 10 s at most.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: the bubble and the Chat reply say only "I am walking. The floor ends
#   here.", never the planning or the marker, and Chat has a Thinking row.
#
# Usage: tool-narration.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: tool-narration.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: tool-narration.sh --go <fidget binary> <fidget test binary>}
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-tool-narration-$(date +%Y%m%d-%H%M%S)"
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
[ "$tools/ax" -nt "$root/scripts/ax-settings.swift" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"
build_ax_events

# FIDGET_HARNESS splits on whitespace, so no path in it may hold a space.
harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=scenario-tool-narration count=$marks"
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

said='I am walking\. The floor ends here\.'
# <name> <window title> <pattern the dump must match>
check() {
  wait_for_ax_dump 3 "$pid" "$2" "$said" "$out/$1.ax.txt" || fail "$1: never said the reply; see $out/$1.ax.txt"
  ! grep -qF 'planted' "$out/$1.ax.txt" || fail "$1: the planning was said; see $out/$1.ax.txt"
  ! grep -qiF 'behavior:' "$out/$1.ax.txt" || fail "$1: the marker was said; see $out/$1.ax.txt"
  [ -z "${3:-}" ] || grep -qiE "$3" "$out/$1.ax.txt" || fail "$1: no match for '$3'; see $out/$1.ax.txt"
  echo "ok: $1"
}

wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_marks 3 "$marks" '^asked$' || fail "no wake reached the Harness; see $log"
wait_for_marks 3 "$marks" '^replied$' || fail "no reply; see $log"
check bubble Fidget
check chat BMO '^AXButton\|(▸|▾) thinking'
id=$("$tools/window-id" "$pid" BMO) || fail "no Chat window"
screencapture -x -o -l "$id" "$out/chat.png"
echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
