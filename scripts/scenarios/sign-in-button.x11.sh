#!/usr/bin/env bash
# Scenario: sign-in-button (X11)
# On screen: launches Fidget as BMO with a fixture Harness that advertises
#   agent sign-in. The tray menu's Chat… row, clicked over dbusmenu, opens
#   Chat on the needs-login landing, and Chat takes focus. Two button presses.
#   Open goes to a recording `xdg-open`, so no browser opens.
#   Three AT-SPI dumps of the Chat window. Fidget quits when it ends.
# Input: a dbusmenu click on the tray's Chat… row; AT-SPI actions on the
#   sign-in button and on Open; no pointer, no keys.
# Duration: about 30 s, 2 min at most.
# Grants: an X11 session with a tray host (StatusNotifierWatcher) and AT-SPI
#   (python3-pyatspi).
# Asserts: the sign-in button shows on the needs-login landing; pressing it
#   shows the waiting line; Open hands the sign-in URL to `xdg-open`; once
#   authenticate completes, a session opens and the waiting line disappears.
#
# Usage: sign-in-button.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# FIDGET_SCENARIO_AX_NEEDS_LOGIN, _AX_WAITING, _AX_SIGNED_IN and _OPENED assert
# those dumps and the hand-off and skip the GUI. That checks the assertion only.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: sign-in-button.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: sign-in-button.x11.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../.." && pwd)
link="https://example.test/device?code=ABCD-1234"

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

check_needs_login() { # <dump>
  grep -qE '^button\|Fake login\|' "$1" || fail "no Fake login button on needs-login; see $1"
  grep -qF 'Or run this in a terminal:' "$1" || fail "no terminal command hint on needs-login; see $1"
  echo "ok: needs-login shows Fake login button"
}

check_waiting() { # <dump>
  grep -qE '^label\|.*Finish signing in in your browser' "$1" || fail "no waiting line after button press; see $1"
  grep -qF "came from $launcher, so" "$1" || fail "waiting line does not name the Harness; see $1"
  echo "ok: waiting line appears after button press"
}

check_opened() { # <recorded file>
  [ "$(cat "$1")" = "$link" ] || fail "Open handed '$(cat "$1")' to xdg-open"
  echo "ok: Open accepted the link and handed it to xdg-open"
}

check_signed_in() { # <dump>
  grep -qF "|$launcher · session " "$1" || fail "mind line does not name the session; see $1"
  ! grep -qF 'Finish signing in in your browser' "$1" || fail "waiting line still present after sign-in; see $1"
  echo "ok: session opened, waiting line cleared"
}

if [ -n "${FIDGET_SCENARIO_AX_NEEDS_LOGIN:-}${FIDGET_SCENARIO_AX_WAITING:-}${FIDGET_SCENARIO_AX_SIGNED_IN:-}${FIDGET_SCENARIO_OPENED:-}" ]; then
  if [ -z "${FIDGET_SCENARIO_AX_NEEDS_LOGIN:-}" ] || [ -z "${FIDGET_SCENARIO_AX_WAITING:-}" ] ||
    [ -z "${FIDGET_SCENARIO_AX_SIGNED_IN:-}" ] || [ -z "${FIDGET_SCENARIO_OPENED:-}" ]; then
    fail "set FIDGET_SCENARIO_AX_NEEDS_LOGIN, FIDGET_SCENARIO_AX_WAITING, FIDGET_SCENARIO_AX_SIGNED_IN and FIDGET_SCENARIO_OPENED"
  fi
  # The launcher the fixture dumps name.
  launcher=${FIDGET_SCENARIO_LAUNCHER:-/opt/fidget/scripts/scenarios/fixture-harness.sh}
  check_needs_login "$FIDGET_SCENARIO_AX_NEEDS_LOGIN"
  check_waiting "$FIDGET_SCENARIO_AX_WAITING"
  check_opened "$FIDGET_SCENARIO_OPENED"
  check_signed_in "$FIDGET_SCENARIO_AX_SIGNED_IN"
  echo "PASS: fixture dumps"
  exit 0
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 sign-in-button needs a session, or set FIDGET_SCENARIO_AX_NEEDS_LOGIN, FIDGET_SCENARIO_AX_WAITING, FIDGET_SCENARIO_AX_SIGNED_IN and FIDGET_SCENARIO_OPENED." >&2
  exit 2
fi
python3 -c 'import pyatspi' > /dev/null 2>&1 || {
  echo "SKIP: python3-pyatspi is not installed." >&2
  exit 2
}

# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-sign-in-button-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home" "$out/bin"
log="$out/app.log" marks="$out/harness.log" opened="$out/opened.txt"
: > "$marks"
: > "$opened"

# FIDGET_HARNESS splits on whitespace, so no path in it may hold a space.
# Chat names the Harness by its launcher.
launcher="$root/scripts/scenarios/fixture-harness.sh"
harness="$launcher $test_bin script=auth-sign-in-link count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

# platform::open_url spawns `xdg-open` by name, so this one on PATH takes Open's URL.
printf '#!/bin/sh\nprintf "%%s\\n" "$*" >> %q\n' "$opened" > "$out/bin/xdg-open"
chmod +x "$out/bin/xdg-open"

env HOME="$out/home" PATH="$out/bin:$PATH" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=600 \
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

ax() { python3 "$root/scripts/ax-window-linux.py" "$@"; }
dump() { # <name>
  ax dump "$pid" BMO frames > "$out/$1.ax.txt" 2> "$out/$1.err" || fail "$1: AT-SPI dump failed; see $out/$1.err"
}
handed() { [ "$(cat "$opened")" = "$link" ]; }
sessions() { [ "$(grep -cx new "$marks")" -ge "$1" ]; }

wait_for 30 grep -qx spawn "$marks" || fail "Harness never spawned; see $log"
# Needs-login does not open Chat by itself; only a link Fidget's own sign-in
# raises does (docs/harness.md).
wait_for 15 grep -qx new "$marks" || fail "the Harness never asked for a session; see $marks"
rc=0
ax tray "$pid" BMO Chat > "$out/open.txt" 2>&1 || rc=$?
if [ "$rc" -eq 2 ]; then
  echo "SKIP: no tray host on this session; see $out/open.txt" >&2
  exit 2
fi
[ "$rc" -eq 0 ] || fail "the tray's Chat row did not open Chat; see $out/open.txt"

dump needs-login
check_needs_login "$out/needs-login.ax.txt"

ax press "$pid" BMO "Fake login" > "$out/press.txt" 2>&1 || fail "could not press Fake login button; see $out/press.txt"
sleep 1
dump waiting
check_waiting "$out/waiting.ax.txt"

ax press "$pid" BMO Open > "$out/press-open.txt" 2>&1 || fail "could not press Open on the sign-in link; see $out/press-open.txt"
wait_for 10 grep -qx 'elicit-url:accept' "$marks" || fail "sign-in form never answered; see $log"
wait_for 5 handed || true
check_opened "$opened"
wait_for 15 sessions 2 || fail "session did not open after sign-in; see $log"
sleep 1.5

dump signed-in
check_signed_in "$out/signed-in.ax.txt"
echo "PASS: evidence in $out"
