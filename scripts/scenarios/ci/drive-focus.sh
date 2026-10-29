#!/usr/bin/env bash
# Scenario: drive-focus (macOS), throwaway probe for #1920
# On screen: a plain 500x350 window near the top left, then Fidget as BMO
#   with the fixture Harness and FIDGET_PROBE_DRIVE_FOCUS set. Fidget and the
#   window quit when the scenario ends.
# Input: synthetic key presses and a held key, a click into the window, a
#   click on the sprite (CGEvent).
# Duration: about 5 s; every wait gives up 3 s after its trigger.
# Grants: Accessibility for the terminal that runs it.
# Asserts, each item on its own so one miss does not hide the rest: with no
#   pill open the overlay takes the keyboard; keydown, keyup and repeat reach
#   it; a click into the window passes through, focus leaves and typing lands
#   in the window; a sprite click takes the keyboard back.
#
# Usage: drive-focus.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: drive-focus.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: drive-focus.sh --go <fidget binary> <fidget test binary>}
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-drive-focus-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools"
seed_tour_shown "$out/home"
log="$out/app.log" marks="$out/harness.log" winlog="$out/window.log" results="$out/results.txt"
: > "$marks"
: > "$winlog"
: > "$results"
failed=0

fail() {
  echo "FAIL: $*" | tee -a "$results" >&2
  echo "evidence in $out" >&2
  exit 1
}
proven() { echo "proven: $*" | tee -a "$results"; }
missed() {
  echo "failed: $*" | tee -a "$results" >&2
  failed=1
}

for tool in drive-focus-window drive-input; do
  [ "$tools/$tool" -nt "$root/scripts/scenarios/helpers/$tool.swift" ] ||
    swiftc -O "$root/scripts/scenarios/helpers/$tool.swift" -o "$tools/$tool"
done
input() { "$tools/drive-input" "$@"; }

"$tools/drive-focus-window" > "$winlog" 2>&1 &
window_pid=$!
trap 'stop_fidget; kill "$window_pid" 2> /dev/null || true; pkill -f "count=$marks" || true' EXIT
trap 'exit 130' INT TERM
wait_for_line 3 "$winlog" '^window: key$' || fail "the helper window never became key; see $winlog"

harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=nop count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 \
  FIDGET_PROBE_DRIVE_FOCUS="${FIDGET_PROBE_DRIVE_FOCUS:-1}" \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
# shellcheck disable=SC2034 # the wait helpers read pid
pid=$!

probe='^drive-probe: overlay-0 '
wait_for_next() { # <file> <ERE> <command...>
  local file=$1 pattern=$2 before
  shift 2
  before=$(grep -cE "$pattern" "$file" || true)
  "$@" || return 1
  wait_for_count 3 "$file" "$pattern" "$before"
}

wait_for_up 3 "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_trace 3 "$log" '^frame: .* sprite\(' || fail "Fidget traced no frame; see $log"

# 1. Launch: the overlay takes the keyboard with no pill open.
wait_for_trace 3 "$log" "${probe}request launch ok" || missed "take: no 'request launch ok'; see $log"
if wait_for_trace 3 "$log" "${probe}Focused\\(true\\)" && wait_for_line 3 "$winlog" '^window: resign key$'; then
  proven "take: Focused(true) and the helper window resigned key"
else
  missed "take: no Focused(true), or the helper window kept key; see $log and $winlog"
fi

# 2. Key edges reach the webview. 124 is Right Arrow.
if wait_for_next "$log" "${probe}keydown key=ArrowRight repeat=false" input key 124 &&
  wait_for_trace 3 "$log" "${probe}keyup key=ArrowRight"; then
  proven "keys: keydown and keyup for ArrowRight"
else
  missed "keys: no ArrowRight keydown/keyup; see $log"
fi

# 3. A held key: a second press flagged as auto-repeat, then the release.
if wait_for_next "$log" "${probe}keydown key=ArrowRight" input down 124 &&
  wait_for_next "$log" "${probe}keydown key=ArrowRight repeat=true" input down 124 repeat; then
  proven "repeat: an auto-repeat keydown arrives with repeat=true"
else
  missed "repeat: no keydown with repeat=true; see $log"
fi
input up 124

# 4. A click into the helper window, off the art, passes through and takes focus away.
input click 350 275
if wait_for_trace 3 "$log" "${probe}Focused\\(false\\)"; then
  proven "pause: Focused(false) after a click on another app's window"
else
  missed "pause: no Focused(false) after a click on another app's window; see $log"
fi
# 6 is Z.
if wait_for_line 3 "$winlog" '^window: mousedown$' &&
  wait_for_next "$winlog" '^window: keydown z$' input key 6 &&
  ! grep -qE "${probe}keydown key=z" "$log"; then
  proven "pass-through: the window took the click and the typing"
else
  missed "pass-through: the window missed the click or the typing; see $winlog"
fi

# 5. A sprite click takes the keyboard back.
wait_for_rest "$log" || missed "resume: the sprite never came to rest"
read -r w h < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r x y < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$log" | tail -1) || fail "no frame trace in $log"
before=$(line_count "$log" "${probe}Focused\\(true\\)")
input click "$((x + w / 2))" "$((y + h / 2))"
wait_for_trace 3 "$log" "${probe}request click ok" || missed "resume: no 'request click ok'; see $log"
# 123 is Left Arrow.
if wait_for_count 3 "$log" "${probe}Focused\\(true\\)" "$before" &&
  wait_for_next "$log" "${probe}keydown key=ArrowLeft" input key 123; then
  proven "resume: Focused(true) and ArrowLeft reached the overlay after a sprite click"
else
  missed "resume: no Focused(true) or no ArrowLeft after a sprite click; see $log"
fi

grep -E "$probe" "$log" > "$out/probe-lines.txt" || true
cp "$winlog" "$out/window-lines.txt"
[ "$failed" -eq 0 ] || fail "see $results"
echo "PASS: evidence in $out"
