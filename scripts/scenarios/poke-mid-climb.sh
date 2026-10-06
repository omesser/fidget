#!/usr/bin/env bash
# Scenario: poke-mid-climb (macOS)
# On screen: launches Fidget with the Static Director. The sprite is thrown at
#   a display's side edge and poked while it climbs. Fidget quits when the
#   scenario ends. No screenshots.
# Input: a real drag that throws the sprite, then clicks on it until one lands
#   as a Poke, for up to five throws. The cursor moves; keep hands off the
#   mouse for the run.
# Duration: about 20 s, 1 min at most.
# Grants: Accessibility for the terminal that runs it.
# Asserts: the Poke starts react over, then the sprite stays Climbing at one
#   position in one climb frame through the cooldown, and climbs on after it.
# Fixture: FIDGET_SCENARIO_TRACE=<saved app log> runs only the check and
#   launches nothing.
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

# Another scenario may be compiling the same helper, so each build lands whole.
build() { # <source> <name>
  [ "$tools/$2" -nt "$1" ] && return
  swiftc -O "$1" -o "$tools/$2.$$" && mv -f "$tools/$2.$$" "$tools/$2"
}

last_frame() { grep '^frame: ' "$log" | tail -1; }
in_state() { last_frame | grep -qE " ($1) "; }
sprite_xy() { last_frame | sed -E 's/.*sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/'; }
pokes() { grep -c '^verbs: .*Poke' "$log" || true; }

# Resting and not walking: a press on a walking sprite lands where it was, and
# the drag then throws whatever window is underneath.
wait_still() { # <tenths of a second>
  local n=$1 a b
  while [ "$n" -gt 0 ]; do
    kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
    if in_state 'Grounded|Perched'; then
      a=$(sprite_xy)
      sleep 0.2
      b=$(sprite_xy)
      [ "$a" = "$b" ] && in_state 'Grounded|Perched' && return 0
    fi
    n=$((n - 1))
    sleep 0.1
  done
  return 1
}

wait_climbing() { # <tenths of a second>
  local n=$1
  until in_state Climbing; do
    n=$((n - 1))
    [ "$n" -gt 0 ] || return 1
    sleep 0.1
  done
}

if [ -n "${FIDGET_SCENARIO_TRACE:-}" ]; then
  log=$FIDGET_SCENARIO_TRACE
else
  # Compiled, not interpreted: `swift script` takes most of a second to start,
  # and a climbing sprite has moved its own height by then.
  build "$root/scripts/click-cursor.swift" click-cursor
  build "$root/scripts/scenarios/throw-sprite.swift" throw-sprite

  env HOME="$out/home" FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 \
    FIDGET_CHARACTERS="$root/characters" "$bin" > "$log" 2>&1 &
  pid=$!
  trap 'kill "$pid" 2> /dev/null || true' EXIT

  wait_still 300 || fail "the sprite never came to rest; see $log"
  size=$(sed -nE 's/^overlay: .* sprite ([0-9]+)x[0-9]+;.*/\1/p' "$log" | head -1)
  [ -n "$size" ] || fail "no sprite size in $log"
  half=$((size / 2))
  # Every display's left and right edge, as "x0 x1" per line.
  displays=$(sed -nE 's/^overlay: .* covers ([0-9]+)x[0-9]+ at \((-?[0-9]+),-?[0-9]+\)/\2 \1/p' "$log" |
    awk '{ print $1, $1 + $2 }')
  [ -n "$displays" ] || fail "no display bounds in $log"
  click_gap=$(sed -nE 's/^overlay: double-click interval ([0-9]+)ms/\1/p' "$log" | head -1)
  click_gap=$(awk -v ms="${click_gap:-500}" 'BEGIN { print (ms + 100) / 1000 }')

  poked=
  for attempt in 1 2 3 4 5; do
    wait_still 100 || continue
    read -r sx sy < <(sprite_xy)
    cx=$((sx + half))
    # Toward the nearer side of the sprite's own display, and the far side on
    # every other attempt, in case the Dock or a neighbour display is in the way.
    read -r x0 x1 < <(awk -v c="$cx" '$1 <= c && c < $2 { print; exit }' <<< "$displays")
    [ -n "${x1:-}" ] || { x0=$cx x1=$cx; }
    near_left=$((cx - x0 < x1 - cx))
    if [ $((attempt % 2)) -eq 0 ]; then near_left=$((1 - near_left)); fi
    if [ "$near_left" -eq 1 ]; then dx=-1500; else dx=1500; fi
    "$tools/throw-sprite" "$cx" "$((sy + half))" "$dx" >> "$out/input.txt" 2>&1 || fail "could not throw the sprite; see $out/input.txt"

    if ! wait_climbing 40; then
      echo "attempt $attempt: no climb" >> "$out/input.txt"
      continue
    fi
    read -r sx sy < <(sprite_xy)
    # A wall climb is centred on a display edge; a Dock climb stands clear of it.
    edge=$(awk -v c="$((sx + half))" '{ for (i = 1; i <= 2; i++) if ($i - c <= 2 && c - $i <= 2) { print $i; exit } }' <<< "$displays")
    if [ -z "$edge" ]; then
      echo "attempt $attempt: climbed the Dock at x=$((sx + half))" >> "$out/input.txt"
      continue
    fi

    # Only the half on the display's side of the edge is drawn. Each click is
    # its own Poke: they are a double-click interval apart, and stop at the
    # first that lands. Aimed near the sprite's top, since it keeps rising
    # while the click is on its way; never into the menu bar above it.
    if [ "$sx" -lt "$edge" ]; then px=$((edge + half / 2)); else px=$((edge - half / 2)); fi
    before=$(pokes)
    for _ in 1 2 3 4 5 6; do
      in_state Climbing || break
      read -r sx sy < <(sprite_xy)
      [ "$sy" -gt "$size" ] || break
      "$tools/click-cursor" "$px" "$((sy + half / 9))" 1 >> "$out/input.txt" 2>&1 || fail "could not click the sprite; see $out/input.txt"
      sleep "$click_gap"
      if [ "$(pokes)" -gt "$before" ]; then
        poked=1
        break
      fi
    done
    [ -n "$poked" ] && break
    echo "attempt $attempt: no click landed before the climb ended" >> "$out/input.txt"
  done
  [ -n "$poked" ] || fail "five throws and no Poke landed mid-climb; see $out/input.txt"

  sleep 4
  kill "$pid" 2> /dev/null || true
  wait "$pid" 2> /dev/null || true
fi

python3 - "$log" 2> "$out/check.txt" << 'EOF' || fail "$(cat "$out/check.txt")"
import re, sys

frame = re.compile(r"^frame: (\d+) (\w+) pos\((-?\d+),(-?\d+)\) \S+ (\S+)#(\d+) ")
frames, poke_at, state = [], None, None
for line in open(sys.argv[1], errors="replace"):
    m = frame.match(line)
    if m:
        at, st, x, y, animation, index = m.groups()
        frames.append((int(at), st, int(x), int(y), animation, int(index)))
        state = st
    elif poke_at is None and line.startswith("verbs:") and "Poke" in line and state == "Climbing":
        poke_at = len(frames)


def miss(message):
    print(message, file=sys.stderr)
    sys.exit(1)


if poke_at is None:
    miss("no Poke landed on a climbing sprite")
after = frames[poke_at:]
# POKE_COOLDOWN_MS is 2500; the windows leave a tick either side of it.
span = after[-1][0] - after[0][0] if after else 0
if span < 2300:
    miss(f"the trace stops {span / 1000:.1f} s after the Poke")
paused = [f for f in after if f[0] - after[0][0] < 2300]
resumed = [f for f in after if 2600 <= f[0] - after[0][0] <= 3500]
if any(f[1] != "Climbing" for f in paused):
    miss("the sprite left the wall during the pause")
if len({(f[2], f[3]) for f in paused}) != 1:
    miss("the sprite moved during the pause")
if after[0][4:] != ("react", 0):
    miss("the Poke did not start react over")
if len({f[5] for f in paused if f[4] == "climb"}) > 1:
    miss("the sprite climbed in place during the pause")
if not any(f[1] == "Climbing" and f[3] < paused[0][3] for f in resumed):
    miss("the sprite did not climb on after the cooldown")
print(f"ok: paused at {paused[0][2:4]} for {paused[-1][0] - paused[0][0]} ms in one climb pose, then climbed on")
EOF

echo "PASS: evidence in $out"
