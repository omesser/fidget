#!/usr/bin/env bash
# Scenario: speak-repeat (X11)
# On screen: launches Fidget as BMO with a fixture Harness, then opens Chat
#   from the tray. Two typed turns. In the first the Harness calls `speak`
#   over Fidget's MCP server and then replies with the same line. Fidget quits
#   when the scenario ends.
# Input: xdotool types two lines into Chat and presses Enter after each.
# Duration: a few seconds, 15 s at most.
# Grants: a running X11 session with a tray host. AT-SPI (python3-pyatspi) and xdotool.
# Asserts: the repeated line shows one bubble, writes one `speak` row to the
#   Action Log, and shows once in Chat, with no "No answer came back." note
#   (#1701).
#
# Usage: speak-repeat.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: speak-repeat.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: speak-repeat.x11.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 speak-repeat needs a session." >&2
  exit 2
fi
python3 -c 'import pyatspi' > /dev/null 2>&1 || {
  echo "SKIP: python3-pyatspi is not installed." >&2
  exit 2
}
command -v xdotool > /dev/null 2>&1 || {
  echo "SKIP: xdotool is not installed." >&2
  exit 2
}

test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-speak-repeat-$(date +%Y%m%d-%H%M%S)"
data="$out/home/.local/share/fidget"
mkdir -p "$data"
log="$out/app.log" marks="$out/harness.log" actions="$data/action-log.jsonl"
: > "$marks"
echo '{"first_run_tour_shown": true}' > "$data/settings.json"
# The fixture's line, from `SPOKEN` in src-tauri/src/harness.rs.
spoken='I said this one already.'

harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=scenario-speak-repeat count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_TRACE_BUBBLE=1 FIDGET_TRACE_DIRECTOR=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'stop_fidget; pkill -f "count=$marks" || true' EXIT INT TERM
wait_for_up 3 "$log" || fail "Fidget never wrote its overlay line; see $log"

python3 "$root/scripts/ax-window-linux.py" tray "$pid" BMO Chat > "$out/tray.txt" 2>&1 ||
  fail "the tray's Chat row opened no Chat; see $out/tray.txt"
# The mind line names a session once Chat has its opening, which then focuses
# the input.
wait_for_ax_dump 5 "$pid" BMO ' · session ' || fail "Chat never named a session; see $log"
chat=$(xdotool search --onlyvisible --name '^BMO$' | head -1) || true
[ -n "$chat" ] || fail "no visible X11 window titled BMO"
xdotool windowactivate --sync "$chat"

bubble='^overlay [^:]*: showSpeech '
send() { # <line>
  xdotool type --delay 15 "$1"
  xdotool key Return
}

# 1. `speak`, then the same line as the reply.
bubbles=$(line_count "$log" "$bubble")
replies=$(line_count "$marks" '^replied$')
settled='^director: [^ ]+ playing fidget$'
played=$(line_count "$log" "$settled")
send 'say it twice'
wait_for_marks 5 "$marks" '^speak ' || fail "the typed turn never called speak; see $marks"
grep -m1 '^speak ' "$marks" | grep -qF '"success":true' || fail "speak did not succeed: $(grep -m1 '^speak ' "$marks")"
wait_for_count 5 "$marks" '^replied$' "$replies" || fail "the Harness never replied; see $marks"
# The reply after the settle: the Behavior plays and the line is dropped.
wait_for_count 5 "$log" "$settled" "$played" ||
  fail "the reply was not settled to its Behavior alone; see $log"

# 2. A second turn. Its answer in Chat means the first reply landed there too.
send 'and now'
wait_for_ax_dump 5 "$pid" BMO 'All done\.' "$out/chat.ax.txt" || fail "the second reply never reached Chat; see $log"

shown=$(($(line_count "$log" "$bubble") - bubbles))
[ "$shown" -eq 2 ] || fail "bubble: $shown bubbles for two turns, want 2 (the spoken line and All done.); see $log"
echo "ok: bubble: the repeated line showed once ($shown bubbles for two turns)"

rows=$(grep -F '"event":"speak"' "$actions" || true)
[ "$(grep -c . <<< "$rows")" -eq 1 ] || fail "Action Log: $(grep -c . <<< "$rows") speak rows, want 1; see $actions"
grep -qF '"declined":null' <<< "$rows" || fail "Action Log: the speak row was declined: $rows"
grep -qF '"turn_open":true' <<< "$rows" || fail "Action Log: the speak row is outside the turn: $rows"
echo "ok: Action Log: $rows"

said=$(grep -cF "$spoken" "$out/chat.ax.txt" || true)
[ "$said" -eq 1 ] || fail "Chat: \"$spoken\" shows $said times, want 1; see $out/chat.ax.txt"
! grep -qF 'No answer came back.' "$out/chat.ax.txt" || fail "Chat: a missing-answer note; see $out/chat.ax.txt"
echo "ok: Chat: \"$spoken\" shows once and no missing-answer note"

echo "PASS: evidence in $out"
