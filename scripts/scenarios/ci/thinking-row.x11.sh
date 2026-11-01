#!/usr/bin/env bash
# Scenario: thinking-row (X11)
# On screen: launches Fidget as BMO with a fixture Harness. Chat opens by
#   itself and takes focus. One AT-SPI dump of the Chat window while the
#   Harness thinks, and one after the reply. Fidget quits when the scenario ends.
# Input: none.
# Duration: a few seconds, 10 s at most.
# Grants: a running X11 session. AT-SPI (python3-pyatspi) for the terminal that runs it.
# Asserts: the Thinking row is open while the Harness thinks, and collapsed
#   once the reply lands.
#
# Usage: thinking-row.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# Set FIDGET_SCENARIO_AX_OPEN and FIDGET_SCENARIO_AX_DONE to assert those dumps
# and skip the GUI. That checks the assertion, not the live window.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: thinking-row.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: thinking-row.x11.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

assert_row() { # <file> <glyph> <label>
  local file=$1 glyph=$2 label=$3 row lower prefix
  row=$(grep -iE '^button\|.*thinking' "$file" | head -n 1 || true)
  [ -n "$row" ] || fail "$label: no Thinking row in $file"
  lower=$(printf '%s' "$row" | tr '[:upper:]' '[:lower:]')
  prefix=$(printf 'button|%s' "$glyph" | tr '[:upper:]' '[:lower:]')
  case $lower in
    "$prefix"*) echo "ok: $label: $row" ;;
    *) fail "$label: Thinking row reads '$row', want it to start with $glyph" ;;
  esac
}

if [ -n "${FIDGET_SCENARIO_AX_OPEN:-}" ] || [ -n "${FIDGET_SCENARIO_AX_DONE:-}" ]; then
  if [ -z "${FIDGET_SCENARIO_AX_OPEN:-}" ] || [ -z "${FIDGET_SCENARIO_AX_DONE:-}" ]; then
    fail "set both FIDGET_SCENARIO_AX_OPEN and FIDGET_SCENARIO_AX_DONE"
  fi
  assert_row "$FIDGET_SCENARIO_AX_OPEN" "▾" "open"
  assert_row "$FIDGET_SCENARIO_AX_DONE" "▸" "done"
  echo "PASS: fixture dumps"
  exit 0
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 thinking-row needs a session, or set FIDGET_SCENARIO_AX_OPEN and FIDGET_SCENARIO_AX_DONE." >&2
  exit 2
fi
python3 -c 'import pyatspi' > /dev/null 2>&1 || {
  echo "SKIP: python3-pyatspi is not installed." >&2
  exit 2
}

test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-thinking-row-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home/.local/share/fidget"
log="$out/app.log" marks="$out/harness.log" hold="$out/hold"
: > "$marks"
mkfifo "$hold"

cat > "$out/home/.local/share/fidget/settings.json" << 'SETTINGS_EOF'
{"first_run_tour_shown": true}
SETTINGS_EOF

harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=scenario-thinking count=$marks hold=$hold"
[ "$(wc -w <<< "$harness")" -eq 5 ] || fail "a path in the Harness line holds a space: $harness"

start_ms=$(($(date +%s%N) / 1000000))
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=1 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'stop_fidget; pkill -f "count=$marks" || true' EXIT INT TERM
wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"

dump() { # <name>
  python3 "$root/scripts/ax-window-linux.py" dump "$pid" BMO > "$out/$1.ax.txt" 2>&1 ||
    fail "$1: AX dump failed; see $out/$1.ax.txt"
  assert_row "$out/$1.ax.txt" "$2" "$1"
}

wait_for_marks 3 "$marks" '^asked$' || fail "no wake reached the Harness; see $log"
wait_for_marks 2 "$marks" '^thought 1$' || fail "no thinking turn; see $log"
wait_for_marks 2 "$marks" '^thought 2$' || fail "no second thought; see $log"
# The wait keeps the dump that showed the open row. The Harness holds its
# reply until the FIFO opens; read-write never blocks, even if it is gone.
wait_for_ax_dump 2 "$pid" BMO '▾.*THINKING' "$out/mid-thought.ax.txt" ||
  fail "no open Thinking row in AX dump while the Harness thinks; see $log"
assert_row "$out/mid-thought.ax.txt" ▾ mid-thought
exec 3<> "$hold"
wait_for_marks 2 "$marks" '^thought 3$' || fail "no third thought; see $log"
wait_for_marks 2 "$marks" '^thought 4$' || fail "no fourth thought; see $log"
wait_for_marks 2 "$marks" '^replied$' || fail "no reply; see $log"
wait_for_ax_dump 2 "$pid" BMO '▸.*THINKING' || fail "thinking row did not collapse; see $log"
dump after-reply ▸
elapsed_ms=$(($(date +%s%N) / 1000000 - start_ms))
echo "PASS: thinking-row completed (${elapsed_ms}ms)"
