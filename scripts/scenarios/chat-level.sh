#!/usr/bin/env bash
# Scenario: chat-level (macOS only, since it asserts a macOS window level)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and takes focus. Finder is brought forward, then Chat is minimized
#   and restored twice: once while focused, once while Finder is in front.
#   Fidget quits when it ends.
# Input: none. Finder comes forward through `open -a`, Fidget through System
#   Events, and the minimizes through the Accessibility API, not the mouse.
# Duration: a few seconds, 10 s at most.
# Grants: Accessibility for the terminal that runs it.
# Asserts: Chat stays at the normal window level (0) while focused so it remains
#   a Perch under the overlay, and still at 0 once Finder is in front.
#   A minimized Chat logs that it is minimized, which is what lets the
#   quick-message pill back, and logs that it is not once restored, focused or
#   not.
#
# Usage: chat-level.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: chat-level.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: chat-level.sh --go <fidget binary> <fidget test binary>}
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-chat-level-$(date +%Y%m%d-%H%M%S)"
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

[ "$tools/window-layer" -nt "$root/scripts/scenarios/window-layer.swift" ] ||
  swiftc -O "$root/scripts/scenarios/window-layer.swift" -o "$tools/window-layer"
[ "$tools/ax" -nt "$root/scripts/ax-settings.swift" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"

# FIDGET_HARNESS splits on whitespace, so no path in it may hold a space.
harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=scenario-thinking count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

start_ms=$(now_ms)
# TRACE_BUBBLE is what logs `chat: <label> is open`; nothing else says a fresh
# Chat has opened.
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=1 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2> /dev/null || true; pkill -f "count=$marks" || true' EXIT INT TERM

at_layer() { [ "$("$tools/window-layer" "$pid" BMO)" = "$1" ]; }
logged() { [ "$(grep -cE "^[0-9]+ chat: chat-[^ ]+ is $1\$" "$trace")" -ge "$2" ]; }
minimize() { "$tools/ax" minimize "$pid" BMO "$1" || fail "could not set minimized to $1"; }

wait_at_layer() { # <layer> <description>
  wait_until 2 at_layer "$1" || fail "$2: Chat is at layer $("$tools/window-layer" "$pid" BMO), not $1"
}

wait_for_up 3 "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_marks 3 "$marks" '^asked$' || fail "no wake reached the Harness; see $log"
# `chat: <label> is open` is the line a fresh Chat writes; it never writes
# `is not minimized`, which is a deminiaturize transition.
wait_for_trace 3 "$trace" '^[0-9]+ chat: chat-[^ ]+ is open$' || fail "Chat never logged open; see $trace"
at_layer 0 || fail "focused Chat is at layer $("$tools/window-layer" "$pid" BMO), not 0"
echo "ok: focused Chat stays at layer 0 (a Perch under the overlay)"

open -a Finder
wait_at_layer 0 "Chat behind Finder"
echo "ok: unfocused Chat stays at layer 0"

osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true"
wait_at_layer 0 "refocused Chat"
before=$(line_count "$trace" '^[0-9]+ chat: chat-[^ ]+ is minimized$')
minimize true
wait_for_count 3 "$trace" '^[0-9]+ chat: chat-[^ ]+ is minimized$' "$before" || fail "minimizing a focused Chat logged nothing within 3s; see $trace"
logged minimized 1 || fail "minimizing a focused Chat logged nothing; see $trace"
before=$(line_count "$trace" '^[0-9]+ chat: chat-[^ ]+ is not minimized$')
minimize false
wait_for_count 3 "$trace" '^[0-9]+ chat: chat-[^ ]+ is not minimized$' "$before" || fail "restoring Chat logged nothing within 3s; see $trace"
logged "not minimized" 1 || fail "restoring Chat logged nothing; see $trace"
echo "ok: a focused Chat counts as closed while minimized"

open -a Finder
wait_at_layer 0 "Chat behind Finder after minimize"
before=$(line_count "$trace" '^[0-9]+ chat: chat-[^ ]+ is minimized$')
minimize true
wait_for_count 3 "$trace" '^[0-9]+ chat: chat-[^ ]+ is minimized$' "$before" || fail "minimizing an unfocused Chat logged nothing within 3s; see $trace"
logged minimized 2 || fail "minimizing an unfocused Chat logged nothing; see $trace"
before=$(line_count "$trace" '^[0-9]+ chat: chat-[^ ]+ is not minimized$')
minimize false
wait_for_count 3 "$trace" '^[0-9]+ chat: chat-[^ ]+ is not minimized$' "$before" || fail "restoring an unfocused Chat logged nothing within 3s; see $trace"
logged "not minimized" 2 || fail "restoring an unfocused Chat logged nothing; see $trace"
echo "ok: an unfocused Chat counts as closed while minimized"
elapsed_ms=$(($(now_ms) - start_ms))
echo "PASS: evidence in $out (${elapsed_ms}ms)"
