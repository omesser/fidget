#!/usr/bin/env bash
# Scenario: chat-level (macOS only, since it asserts a macOS window level)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and takes focus. Finder is brought forward, then Chat is minimized
#   and restored twice: once while focused, once while Finder is in front.
#   Fidget quits when it ends.
# Input: none. Finder comes forward through `open -a`, Fidget through System
#   Events, and the minimizes through the Accessibility API, not the mouse.
# Duration: about 30 s, 2 min at most.
# Grants: Accessibility for the terminal that runs it.
# Asserts: Chat sits at NSStatusWindowLevel (25) while focused and at the
#   normal level (0) once Finder is in front. A minimized Chat logs that it is
#   minimized, which is what lets the quick-message pill back, and logs that
#   it is not once restored, focused or not.
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
out="${TMPDIR:-/tmp}/fidget-scenario-chat-level-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools"
log="$out/app.log" marks="$out/harness.log"
: > "$marks"

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

env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=6 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2> /dev/null || true; pkill -f "count=$marks" || true' EXIT

wait_for() { # <seconds> <command...>
  local n=$(($1 * 4))
  shift
  until "$@" > /dev/null 2>&1; do
    kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
    n=$((n - 1))
    [ "$n" -gt 0 ] || return 1
    sleep 0.25
  done
}
at_layer() { [ "$("$tools/window-layer" "$pid" BMO)" = "$1" ]; }
logged() { [ "$(grep -cE "^chat: chat-[^ ]+ is $1\$" "$log")" -ge "$2" ]; }
minimize() { "$tools/ax" minimize "$pid" BMO "$1" || fail "could not set minimized to $1"; }

wait_for 30 grep -qx asked "$marks" || fail "no wake reached the Harness; see $log"
wait_for 15 at_layer 25 || fail "focused Chat is not at layer 25"
echo "ok: focused Chat floats at layer 25"

open -a Finder
wait_for 5 at_layer 0 || fail "Chat stayed at layer $("$tools/window-layer" "$pid" BMO) behind Finder"
echo "ok: unfocused Chat drops to layer 0"

osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true"
wait_for 5 at_layer 25 || fail "refocused Chat did not float again"
minimize true
wait_for 5 logged minimized 1 || fail "minimizing a focused Chat logged nothing; see $log"
minimize false
wait_for 5 logged "not minimized" 1 || fail "restoring Chat logged nothing; see $log"
echo "ok: a focused Chat counts as closed while minimized"

open -a Finder
wait_for 5 at_layer 0 || fail "Chat did not drop behind Finder"
minimize true
wait_for 5 logged minimized 2 || fail "minimizing an unfocused Chat logged nothing; see $log"
minimize false
wait_for 5 logged "not minimized" 2 || fail "restoring an unfocused Chat logged nothing; see $log"
echo "ok: an unfocused Chat counts as closed while minimized"
echo "PASS: evidence in $out"
