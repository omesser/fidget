#!/usr/bin/env bash
# Scenario: poke-mid-climb (macOS)
# On screen: launches Fidget with the Static Director. The sprite is thrown at
#   a display's side edge and poked while it climbs. Fidget quits when the
#   scenario ends. No screenshots.
# Input: a real drag that throws the sprite, then clicks on it until one lands
#   as a Poke, for up to five throws. The cursor moves; keep hands off the
#   mouse for the run.
# Duration: a few seconds per throw, 30 s at most over five throws.
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
. "$root/scripts/scenarios/wait-helpers.mac.sh"
# Unique per run: the leaves' fixture tests run in parallel and would share check.txt.
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-poke-mid-climb-$(date +%Y%m%d-%H%M%S)-XXXX")
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools"
seed_tour_shown "$out/home"
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
climbing() { in_state Climbing; }
# The checker passes once the trace runs past the Poke's 2.5 s cooldown.
cooldown_traced() { python3 "$root/scripts/scenarios/poke-mid-climb-check.py" "$log" 2> "$out/check.txt"; }

start_ms=$(now_ms)
if [ -n "${FIDGET_SCENARIO_TRACE:-}" ]; then
  log=$FIDGET_SCENARIO_TRACE
else
  # Compiled, not interpreted: `swift script` takes most of a second to start,
  # and a climbing sprite has moved its own height by then.
  build "$root/scripts/click-cursor.swift" click-cursor
  build "$root/scripts/scenarios/throw-sprite.swift" throw-sprite
  build "$root/scripts/warp-cursor.swift" warp-cursor

  env HOME="$out/home" FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 \
    FIDGET_CHARACTERS="$root/characters" "$bin" > "$log" 2>&1 &
  pid=$!
  trap 'stop_fidget' EXIT INT TERM

  wait_for_rest "$log" || fail "the sprite never came to rest; see $log"
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
    wait_for_rest "$log" || continue
    read -r sx sy < <(sprite_xy)
    cx=$((sx + half))
    # Toward the nearer side of the sprite's own display, and the far side on
    # every other attempt, in case the Dock or a neighbour display is in the way.
    read -r x0 x1 < <(awk -v c="$cx" '$1 <= c && c < $2 { print; exit }' <<< "$displays") || true
    [ -n "${x1:-}" ] || { x0=$cx x1=$cx; }
    near_left=$((cx - x0 < x1 - cx))
    if [ $((attempt % 2)) -eq 0 ]; then near_left=$((1 - near_left)); fi
    if [ "$near_left" -eq 1 ]; then dx=-1500; else dx=1500; fi
    "$tools/throw-sprite" "$cx" "$((sy + half))" "$dx" >> "$out/input.txt" 2>&1 || fail "could not throw the sprite; see $out/input.txt"

    if ! wait_until 2 climbing; then
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
    # its own Poke: a double-click interval apart, stopping at the first that
    # lands. Aim near the top, since it rises; never into the menu bar above.
    if [ "$sx" -lt "$edge" ]; then px=$((edge + half / 2)); else px=$((edge - half / 2)); fi
    # Further into this display than the click, and not past it. The pill
    # opens after a dwell on the sprite and then holds the climb. Leave with
    # the click: a silent warp keeps :hover, and the gaps add up past that dwell.
    if [ "$px" -gt "$edge" ]; then into=1; else into=-1; fi
    away=$((px + into * 4 * size))
    read -r dx0 dx1 < <(awk -v c="$px" '$1 <= c && c < $2 { print; exit }' <<< "$displays") || true
    if [ -n "${dx1:-}" ]; then
      if [ "$away" -lt "$dx0" ]; then away=$dx0; fi
      if [ "$away" -ge "$dx1" ]; then away=$((dx1 - 1)); fi
    elif [ "$away" -lt 0 ]; then
      away=0
    fi
    before=$(pokes)
    for _ in 1 2 3 4 5 6; do
      in_state Climbing || break
      read -r sx sy < <(sprite_xy)
      [ "$sy" -gt "$size" ] || break
      "$tools/click-cursor" "$px" "$((sy + half / 9))" 1 >> "$out/input.txt" 2>&1 || fail "could not click the sprite; see $out/input.txt"
      "$tools/warp-cursor" "$away" "$sy" --event >> "$out/input.txt" 2>&1 || fail "could not move the pointer off the sprite; see $out/input.txt"
      if wait_for_count "$click_gap" "$log" '^verbs: .*Poke' "$before"; then
        poked=1
        break
      fi
    done
    [ -n "$poked" ] && break
    echo "attempt $attempt: no click landed before the climb ended" >> "$out/input.txt"
  done
  [ -n "$poked" ] || fail "five throws and no Poke landed mid-climb; see $out/input.txt"

  wait_until 3 cooldown_traced || true
  kill "$pid" 2> /dev/null || true
  wait "$pid" 2> /dev/null || true
fi

python3 "$root/scripts/scenarios/poke-mid-climb-check.py" "$log" 2> "$out/check.txt" || fail "$(cat "$out/check.txt")"

echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
