#!/usr/bin/env bash
# Scenario: thinking-row (macOS)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and takes focus. Two screenshots of the Chat window. Fidget quits
#   when the scenario ends.
# Input: none.
# Duration: a few seconds, 10 s at most.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: the Thinking row is open while the Harness thinks, and collapsed
#   once the reply lands.
#
# Usage: thinking-row.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: thinking-row.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: thinking-row.sh --go <fidget binary> <fidget test binary>}
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-thinking-row-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home/Library/Application Support/fidget" "$tools"
log="$out/app.log" marks="$out/harness.log"
trace="$out/home/Library/Application Support/fidget/process.log"
: > "$marks"

cat > "$out/home/Library/Application Support/fidget/settings.json" << 'SETTINGS_EOF'
{"first_run_tour_shown": true}
SETTINGS_EOF

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

[ -x "$tools/window-id" ] || swiftc -O "$root/scripts/scenarios/window-id.swift" -o "$tools/window-id"
[ -x "$tools/ax" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"

# FIDGET_HARNESS splits on whitespace, so no path in it may hold a space.
harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=scenario-thinking count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

start_ms=$(now_ms)
# TRACE_BUBBLE is what logs `chat: <label> is open`; nothing else says a fresh
# Chat has opened.
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=1 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2> /dev/null || true; pkill -f "count=$marks" || true' EXIT INT TERM

# The first row is the turn that thought; the ask turn before it has none.
capture() { # <name> <glyph the first Thinking row starts with>
  local id row
  id=$("$tools/window-id" "$pid" BMO) || fail "$1: no Chat window"
  screencapture -x -o -l "$id" "$out/$1.png"
  "$tools/ax" dump "$pid" BMO > "$out/$1.ax.txt" 2>&1 || fail "$1: AX dump failed; see $out/$1.ax.txt"
  row=$(grep -iEm1 '^AXButton\|(▸|▾) thinking' "$out/$1.ax.txt") || fail "$1: no Thinking row in $out/$1.ax.txt"
  [[ $row == "AXButton|$2"* ]] || fail "$1: Thinking row reads '$row', want it to start with $2"
  echo "ok: $1: $row"
}

wait_for_up 3 "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_marks 3 "$marks" '^asked$' || fail "no wake reached the Harness; see $log"
# `chat: <label> is open` is the line a fresh Chat writes; it never writes
# `is not minimized`, which is a deminiaturize transition.
wait_for_trace 3 "$trace" '^[0-9]+ chat: chat-[^ ]+ is open$' || fail "Chat never logged open; see $trace"
[ -n "$("$tools/window-id" "$pid" BMO 2> /dev/null)" ] || fail "Chat did not open for the ask"
wait_for_marks 3 "$marks" '^thought 2$' || fail "no thinking turn; see $log"
capture mid-thought ▾
wait_for_marks 3 "$marks" '^replied$' || fail "no reply; see $log"
capture after-reply ▸
elapsed_ms=$(($(now_ms) - start_ms))
echo "PASS: evidence in $out (${elapsed_ms}ms)"
