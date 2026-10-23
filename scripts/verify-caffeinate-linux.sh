#!/usr/bin/env bash
# Verify: Caffeinate on Linux (#1789)
# On screen: launches Fidget as BMO with a fixture Harness and clicks its tray
#   menu: Caffeinate on, Caffeinate off, Caffeinate on, then Quit.
# Input: no pointer or keys. The tray rows are clicked over dbusmenu.
# Duration: a few seconds, 30 s at most.
# Grants: an X11 or Wayland desktop session with a tray host, logind that lets
#   this user inhibit (any active desktop session does), python3-gi,
#   python3-pyatspi and dbus-monitor.
# Asserts: `systemd-inhibit --list` shows Fidget once Caffeinate is on, drops
#   it once it is off, and drops it once Fidget quits. Each wait is on logind's
#   BlockInhibited change, so nothing polls.
#
# Usage: verify-caffeinate-linux.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: verify-caffeinate-linux.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: verify-caffeinate-linux.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

for tool in systemd-inhibit dbus-monitor; do
  command -v "$tool" > /dev/null 2>&1 || {
    echo "SKIP: $tool is not installed." >&2
    exit 2
  }
done
systemd-inhibit --list > /dev/null 2>&1 || {
  echo "SKIP: systemd-inhibit --list failed, so there is no logind to ask." >&2
  exit 2
}

test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-verify-caffeinate-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home/.local/share/fidget"
log="$out/app.log" marks="$out/harness.log"
: > "$marks"

cat > "$out/home/.local/share/fidget/settings.json" << 'SETTINGS_EOF'
{"first_run_tour_shown": true}
SETTINGS_EOF

# count= gives this run's Harness a unique command line for the kill on exit.
harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=nop count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line has a space: $harness"

# logind's PropertiesChanged on each inhibitor change. dbus-monitor's first
# lines arrive once it is subscribed, so the reader checks once before any
# change can be missed.
login1_changes() {
  dbus-monitor --system \
    "type='signal',path='/org/freedesktop/login1',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'"
}
listed() { systemd-inhibit --list --no-pager | grep -qE "^Fidget +.* Caffeinate +block"; }
not_listed() { ! listed; }
wait_for_inhibitors() { # <listed|not_listed>
  wait_on_source 5 login1_changes probe_each_line "$1"
}
show() { # <step>
  {
    echo "== $1"
    systemd-inhibit --list --no-pager
  } | tee -a "$out/inhibitors.txt"
}
tray() { # <row>
  python3 "$root/scripts/ax-window-linux.py" tray "$pid" - "$1" >> "$out/tray.txt" 2>&1
}

listed && fail "Fidget already has an inhibitor before this run started"

env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'stop_fidget; pkill -f "count=$marks" || true' EXIT INT TERM
wait_for_up 5 "$log" || fail "Fidget never wrote its overlay line; see $log"

rc=0
tray Caffeinate || rc=$?
if [ "$rc" -eq 2 ]; then
  echo "SKIP: no tray host on this session; see $out/tray.txt" >&2
  exit 2
fi
[ "$rc" -eq 0 ] || fail "the tray's Caffeinate row did not click; see $out/tray.txt"
wait_for_inhibitors listed || fail "Caffeinate on, but logind lists no Fidget inhibitor; see $log"
show "Caffeinate on"
echo "ok: on, logind lists Fidget"

tray Caffeinate || fail "the second Caffeinate click failed; see $out/tray.txt"
wait_for_inhibitors not_listed || fail "Caffeinate off, but logind still lists Fidget"
show "Caffeinate off"
echo "ok: off, logind lists no Fidget"

tray Caffeinate || fail "the third Caffeinate click failed; see $out/tray.txt"
wait_for_inhibitors listed || fail "Caffeinate back on, but logind lists no Fidget inhibitor"
echo "ok: on again"

# Quit is process::exit, which skips Drop, so this is the kernel closing the
# inhibitor fd. The click's reply can race the exit, so its status is moot.
tray Quit || true
wait "$pid" 2> /dev/null || true
quit_pid=$pid
pid=
listed && {
  wait_for_inhibitors not_listed || fail "Fidget quit, but logind still lists it"
}
show "after Quit"
kill -0 "$quit_pid" 2> /dev/null && fail "Fidget is still running after Quit"
echo "ok: quit, logind lists no Fidget"

echo "PASS: evidence in $out"
