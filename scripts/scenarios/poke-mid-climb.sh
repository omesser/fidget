#!/usr/bin/env bash
# Scenario: poke-mid-climb (macOS)
# On screen: launches Fidget with the offline Director. The sprite is thrown at
#   the outer edge of the leftmost or rightmost display and poked while it
#   climbs. Fidget quits when the scenario ends. No screenshots.
# Input: a real drag that throws the sprite, then clicks on it until one lands
#   as a Poke. The cursor moves; keep hands off the mouse for the run.
# Duration: about 20 s, 1 min at most.
# Grants: Accessibility for the terminal that runs it.
# Asserts: after the Poke the sprite stays Climbing at one position, plays
#   react, then holds one climb frame, and climbs on once the cooldown ends.
#
# Usage: poke-mid-climb.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: poke-mid-climb.sh --go <fidget binary>}
root=$(cd "$(dirname "$0")/../.." && pwd)
out="${TMPDIR:-/tmp}/fidget-scenario-poke-mid-climb-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools"
log="$out/app.log"

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

last_frame() { grep '^frame: ' "$log" | tail -1; }
in_state() { last_frame | grep -qE " ($1) "; }
wait_state() { # <tenths of a second> <state regex>
  local n=$1
  until in_state "$2"; do
    kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
    n=$((n - 1))
    [ "$n" -gt 0 ] || return 1
    sleep 0.1
  done
}
sprite_xy() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/'; }

# FIDGET_SCENARIO_TRACE checks a saved trace and launches nothing.
if [ -n "${FIDGET_SCENARIO_TRACE:-}" ]; then
  log=$FIDGET_SCENARIO_TRACE
else
  # Compiled, not interpreted: `swift script` takes most of a second to start,
  # and a climbing sprite has moved its own height by then.
  [ "$tools/click-cursor" -nt "$root/scripts/click-cursor.swift" ] || swiftc -O "$root/scripts/click-cursor.swift" -o "$tools/click-cursor"
  [ "$tools/throw-sprite" -nt "$root/scripts/throw-sprite.swift" ] || swiftc -O "$root/scripts/throw-sprite.swift" -o "$tools/throw-sprite"

  env HOME="$out/home" FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 "$bin" > "$log" 2>&1 &
  pid=$!
  trap 'kill "$pid" 2> /dev/null || true' EXIT

  wait_state 300 'Grounded|Perched' || fail "the sprite never came to rest; see $log"
  read -r size < <(sed -nE 's/^overlay: .* sprite ([0-9]+)x[0-9]+;.*/\1/p' "$log" | head -1) || fail "no sprite size in $log"
  read -r left right < <(sed -nE 's/^overlay: .* covers ([0-9]+)x[0-9]+ at \((-?[0-9]+),-?[0-9]+\)/\2 \1/p' "$log" |
    awk '{ r = $1 + $2; if (NR == 1 || $1 < l) l = $1; if (NR == 1 || r > m) m = r } END { print l, m }')
  [ -n "${right:-}" ] || fail "no display bounds in $log"
  half=$((size / 2))

  climbed=
  for attempt in 1 2 3 4 5; do
    wait_state 100 'Grounded|Perched' || continue
    read -r sx sy < <(sprite_xy)
    cx=$((sx + half))
    # Toward the nearer outer edge, hard enough to clear the Dock on the way.
    if [ $((cx - left)) -lt $((right - cx)) ]; then dx=-1500 edge=$left; else dx=1500 edge=$right; fi
    "$tools/throw-sprite" "$cx" "$((sy + half))" "$dx" >> "$out/input.txt" 2>&1 || fail "could not throw the sprite; see $out/input.txt"
    if wait_state 40 Climbing; then
      read -r sx sy < <(sprite_xy)
      # A wall climb is centred on the edge; anything else climbed the Dock.
      if [ $((sx + half - edge)) -ge -2 ] && [ $((sx + half - edge)) -le 2 ]; then
        climbed=1
        break
      fi
    fi
    echo "attempt $attempt: no climb at the edge" >> "$out/input.txt"
    sleep 3
  done
  [ -n "$climbed" ] || fail "five throws and no climb up a display edge; see $out/input.txt"

  # Only the half on this side of the edge is drawn. Clicks are 0.6 s apart so
  # two never pair into a Summon; aimed just above the last frame's centre,
  # since the sprite keeps rising while the click is on its way.
  if [ "$edge" = "$left" ]; then px=$((edge + half / 2)); else px=$((edge - half / 2)); fi
  for _ in 1 2 3 4 5 6; do
    in_state Climbing || break
    read -r sx sy < <(sprite_xy)
    "$tools/click-cursor" "$px" "$((sy + half / 9))" 1 >> "$out/input.txt" 2>&1 || fail "could not click the sprite; see $out/input.txt"
    sleep 0.6
    grep -q '^verbs: .*Poke' "$log" && break
  done
  sleep 4
  kill "$pid" 2> /dev/null || true
fi

python3 - "$log" 2> "$out/check.txt" << 'EOF' || fail "$(cat "$out/check.txt")"
import re, sys

frame = re.compile(r"^frame: (\d+) (\w+) pos\((-?\d+),(-?\d+)\) \S+ (\S+)#(\d+) ")
events, state = [], None
for line in open(sys.argv[1], errors="replace"):
    m = frame.match(line)
    if m:
        at, st, x, y, animation, index = m.groups()
        events.append((int(at), st, int(x), int(y), animation, int(index)))
        state = st
    elif line.startswith("verbs:") and "Poke" in line and state == "Climbing":
        events.append("poke")


def miss(message):
    print(message, file=sys.stderr)
    sys.exit(1)


if "poke" not in events:
    miss("no Poke landed on a climbing sprite")
after = [e for e in events[events.index("poke") + 1 :] if e != "poke"]
# POKE_COOLDOWN_MS is 2500; the windows leave a tick either side of it.
paused = [f for f in after if f[0] - after[0][0] < 2300]
resumed = [f for f in after if 2600 <= f[0] - after[0][0] <= 3500]
if any(f[1] != "Climbing" for f in paused):
    miss("the sprite left the wall during the pause")
if len({(f[2], f[3]) for f in paused}) != 1:
    miss("the sprite moved during the pause")
if "react" not in {f[4] for f in paused}:
    miss("no react on the wall after the Poke")
if len({f[5] for f in paused if f[4] == "climb"}) > 1:
    miss("the sprite climbed in place during the pause")
if not any(f[1] == "Climbing" and f[3] < paused[0][3] for f in resumed):
    miss("the sprite did not climb on after the cooldown")
print(f"ok: paused at {paused[0][2:4]} for {paused[-1][0] - paused[0][0]} ms in one climb pose, then climbed on")
EOF

echo "PASS: evidence in $out"
