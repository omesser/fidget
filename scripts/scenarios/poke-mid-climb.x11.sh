#!/usr/bin/env bash
# Scenario: poke-mid-climb (X11)
# On screen: launches Fidget with the Static Director. The sprite is thrown at
#   a display's side edge and poked while it climbs. Fidget quits when the
#   scenario ends. No screenshots.
# Input: a real drag through xdotool that throws the sprite, then clicks on it
#   until one lands as a Poke, for up to five throws. The cursor moves; keep
#   hands off the mouse for the run.
# Duration: about 20 s, 1 min at most.
# Grants: an X11 session and xdotool.
# Asserts: the Poke starts react over, then the sprite stays Climbing at one
#   position in one climb frame through the cooldown, and climbs on after it.
# Fixture: FIDGET_SCENARIO_TRACE=<saved app log> runs only the check and
#   launches nothing.
#
# Usage: poke-mid-climb.x11.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: poke-mid-climb.x11.sh --go <fidget binary>}
root=$(cd "$(dirname "$0")/../.." && pwd)

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

check() { # <app log>
  python3 "$root/scripts/scenarios/poke-mid-climb-check.py" "$1" 2> "$out/check.txt" || fail "$(cat "$out/check.txt")"
}

# Unique per run: the leaves' fixture tests run in parallel and would share check.txt.
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-poke-mid-climb-$(date +%Y%m%d-%H%M%S)-XXXX")

if [ -n "${FIDGET_SCENARIO_TRACE:-}" ]; then
  mkdir -p "$out"
  check "$FIDGET_SCENARIO_TRACE"
  echo "PASS: fixture trace"
  exit 0
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 poke-mid-climb needs a session, or set FIDGET_SCENARIO_TRACE." >&2
  exit 2
fi
command -v xdotool > /dev/null 2>&1 || {
  echo "SKIP: xdotool is not installed." >&2
  exit 2
}

mkdir -p "$out/home/.local/share/fidget"
cat > "$out/home/.local/share/fidget/settings.json" << 'SETTINGS_EOF'
{"first_run_tour_shown": true}
SETTINGS_EOF
log="$out/app.log"
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

# One xdotool call per gesture: a process per step would stretch the drag past
# the 60 ms the app measures a Throw over. Timings are throw-sprite.swift's.
# X11 has no negative root coordinate, and xdotool reads "-1300" as an option.
throw_sprite() { # <x> <y> <dx>
  local args=(mousemove "$1" "$2" sleep 0.12 mousedown 1 sleep 0.15) step x
  # 10 steps with tighter sleeps (0.01s each = 100ms total) for smoother motion.
  for step in 1 2 3 4 5 6 7 8 9 10; do
    x=$(($1 + $3 * step / 10))
    args+=(mousemove "$((x < 0 ? 0 : x))" "$(($2 - 10 * step / 10))" sleep 0.01)
  done
  xdotool "${args[@]}" mouseup 1
}

click_at() { # <x> <y>
  xdotool mousemove "$1" "$2" sleep 0.12 mousedown 1 sleep 0.06 mouseup 1
}

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
  # every other attempt, in case a panel or a neighbour display is in the way.
  read -r x0 x1 < <(awk -v c="$cx" '$1 <= c && c < $2 { print; exit }' <<< "$displays") || true
  [ -n "${x1:-}" ] || { x0=$cx x1=$cx; }
  near_left=$((cx - x0 < x1 - cx))
  if [ $((attempt % 2)) -eq 0 ]; then near_left=$((1 - near_left)); fi
  if [ "$near_left" -eq 1 ]; then dx=-1500; else dx=1500; fi
  throw_sprite "$cx" "$((sy + half))" "$dx" >> "$out/input.txt" 2>&1 || fail "could not throw the sprite; see $out/input.txt"

  if ! wait_climbing 40; then
    echo "attempt $attempt: no climb" >> "$out/input.txt"
    continue
  fi
  read -r sx sy < <(sprite_xy)
  # A wall climb is centred on a display edge; a Perch climb stands clear of it.
  edge=$(awk -v c="$((sx + half))" '{ for (i = 1; i <= 2; i++) if ($i - c <= 2 && c - $i <= 2) { print $i; exit } }' <<< "$displays")
  if [ -z "$edge" ]; then
    echo "attempt $attempt: climbed a Perch at x=$((sx + half))" >> "$out/input.txt"
    continue
  fi

  # Only the display's half of the sprite is drawn. Clicks a double-click
  # interval apart are separate Pokes; stop at the first. Aim near the top,
  # since it rises while the click travels, but never into a top panel.
  # Into the display: right of a left edge, left of a right edge.
  if [ "$dx" -lt 0 ]; then into=1; else into=-1; fi
  px=$((edge + into * half / 2))
  before=$(pokes)
  for _ in 1 2 3 4 5 6; do
    in_state Climbing || break
    read -r sx sy < <(sprite_xy)
    [ "$sy" -gt "$size" ] || break
    click_at "$px" "$((sy + half / 9))" >> "$out/input.txt" 2>&1 || fail "could not click the sprite; see $out/input.txt"
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

# The quick-message pill holds a climb while the pointer rests on the sprite.
# Move further into the display than the click landed.
away=$((px + into * 4 * size))
xdotool mousemove "$((away < 0 ? 0 : away))" "$sy" >> "$out/input.txt" 2>&1 || fail "could not move the pointer off the sprite; see $out/input.txt"

sleep 4
kill "$pid" 2> /dev/null || true
wait "$pid" 2> /dev/null || true

check "$log"
echo "PASS: evidence in $out"
