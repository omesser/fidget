#!/usr/bin/env bash
# Scenario: landing-link-click (X11)
# On screen: launches Fidget as BMO with Codex picked and no `npx` on PATH, so
#   no Harness runs. The tray menu's Chat… row is clicked over dbusmenu, so
#   Chat opens on the "Codex needs `npx`" landing and takes focus. The install
#   link is clicked. Two AT-SPI dumps. Fidget quits at the end.
# Input: one real click on the link through xdotool; no keys. The link goes to
#   a recording `xdg-open`, so no browser opens.
# Duration: about 20 s, 1 min at most.
# Grants: an X11 session with a tray host (StatusNotifierWatcher), AT-SPI
#   (python3-pyatspi) and xdotool.
# Asserts: the landing draws `npx` and https://nodejs.org/ as their own
#   elements, with no backtick left; nothing reaches `xdg-open` before the
#   click; the click hands exactly https://nodejs.org/ to it; Chat still shows
#   the landing after it, so the webview did not navigate.
#
# Usage: landing-link-click.x11.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# FIDGET_SCENARIO_AX_LANDING, _AX_AFTER and _OPENED assert those dumps and the
# recorded hand-off and skip the GUI. That checks the assertion, not the window.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: landing-link-click.x11.sh --go <fidget binary>}
root=$(cd "$(dirname "$0")/../.." && pwd)
url=https://nodejs.org/

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

landing() { grep -qF '|Codex needs ' "$1"; }

check_landing() { # <dump>: sets x y w h to the link's frame
  local f=$1 row
  landing "$f" || fail "Chat shows no Codex needs npx landing; see $f"
  ! grep -qF '`' "$f" || fail "a backtick is left in the landing copy; see $f"
  awk -F'|' '$2 == "npx"' "$f" | grep -q . || fail "npx is not drawn as its own element; see $f"
  row=$(awk -F'|' -v u="$url" '$2 == u' "$f" | head -1)
  [ -n "$row" ] || fail "$url is not drawn as its own element; see $f"
  read -r x y w h <<< "$(tr , ' ' <<< "${row##*|}")"
  echo "ok: the landing draws npx and $url apart from the copy, no backtick left"
}

check_handed() { # <recorded file>
  [ "$(cat "$1")" = "$url" ] || fail "the click handed '$(cat "$1")' to xdg-open, want $url"
  echo "ok: the click handed $url to xdg-open"
}

check_after() { # <dump>
  landing "$1" || fail "Chat left the landing after the click, so the webview navigated; see $1"
  echo "ok: Chat still shows the landing"
}

if [ -n "${FIDGET_SCENARIO_AX_LANDING:-}" ] || [ -n "${FIDGET_SCENARIO_AX_AFTER:-}" ] || [ -n "${FIDGET_SCENARIO_OPENED:-}" ]; then
  if [ -z "${FIDGET_SCENARIO_AX_LANDING:-}" ] || [ -z "${FIDGET_SCENARIO_AX_AFTER:-}" ] || [ -z "${FIDGET_SCENARIO_OPENED:-}" ]; then
    fail "set FIDGET_SCENARIO_AX_LANDING, FIDGET_SCENARIO_AX_AFTER and FIDGET_SCENARIO_OPENED"
  fi
  check_landing "$FIDGET_SCENARIO_AX_LANDING"
  check_handed "$FIDGET_SCENARIO_OPENED"
  check_after "$FIDGET_SCENARIO_AX_AFTER"
  echo "PASS: fixture dumps"
  exit 0
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 landing-link-click needs a session, or set FIDGET_SCENARIO_AX_LANDING, FIDGET_SCENARIO_AX_AFTER and FIDGET_SCENARIO_OPENED." >&2
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

out="${TMPDIR:-/tmp}/fidget-scenario-landing-link-click-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home" "$out/bin"
log="$out/app.log" opened="$out/opened.txt"
: > "$opened"

# platform::open_url spawns `xdg-open` by name, so this one on PATH takes the hand-off.
printf '#!/bin/sh\nprintf "%%s\\n" "$*" >> %q\n' "$opened" > "$out/bin/xdg-open"
chmod +x "$out/bin/xdg-open"
path="$out/bin:/usr/bin:/bin:/usr/sbin:/sbin"
if PATH=$path command -v npx > /dev/null; then
  echo "SKIP: npx is on $path, so Codex would launch and no landing shows." >&2
  exit 2
fi

env HOME="$out/home" PATH="$path" \
  FIDGET_HARNESS=codex \
  FIDGET_DIRECTOR_WAKE_SECS=600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2> /dev/null || true' EXIT

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
dump() { ax dump "$pid" BMO frames > "$out/$1.ax.txt" 2> "$out/$1.err"; }
shows_landing() { dump "$1" && landing "$out/$1.ax.txt"; }
handed() { [ "$(cat "$opened")" = "$url" ]; }

sleep 3
rc=0
ax tray "$pid" BMO Chat > "$out/open.txt" 2>&1 || rc=$?
if [ "$rc" -eq 2 ]; then
  echo "SKIP: no tray host on this session; see $out/open.txt" >&2
  exit 2
fi
[ "$rc" -eq 0 ] || fail "the tray's Chat row did not open Chat; see $out/open.txt"
wait_for 15 shows_landing landing || fail "Chat shows no Codex needs npx landing; see $out/landing.ax.txt"
check_landing "$out/landing.ax.txt"

[ ! -s "$opened" ] || fail "something reached xdg-open before the click: $(cat "$opened")"
xdotool mousemove --sync "$((x + w / 2))" "$((y + h / 2))" click 1 > "$out/click.txt" 2>&1 ||
  fail "could not click the link; see $out/click.txt"
wait_for 5 handed || true
check_handed "$opened"

sleep 1
dump after || fail "AT-SPI dump failed after the click; see $out/after.err"
check_after "$out/after.ax.txt"
echo "PASS: evidence in $out"
