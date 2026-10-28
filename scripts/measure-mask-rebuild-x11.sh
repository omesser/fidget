#!/usr/bin/env bash
# Mask update rate and cost on X11, per scenario (#428). Launches fidget with
# a bench copy of a shipped character, holds the pointer on the sprite so the
# cursor-over gate stays open, and times the mask_rebuild trace lines that
# land while the scenario's animation plays.

set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

bin="target/release/fidget"
scenario="idle"
target_updates=100
deadline_sec=60
scale=""
startup_sec=60

usage() {
  cat << 'EOF_USAGE'
Usage: measure-mask-rebuild-x11.sh [--bin PATH] [--scale N] [SCENARIO] [TARGET_UPDATES] [DEADLINE]

Runs fidget on $DISPLAY and stops after TARGET_UPDATES mask updates (default
100) or DEADLINE seconds (default 60) of the scenario's animation.

Scenarios:
  idle    BMO 126x128@1x, pointer parked in the corner (expect 0 updates)
  walk    BMO 126x128@1x walking, pointer on the sprite
  fast    BMO 126x128@1x looping react, its fastest art (10 fps), pointer on it
  large   BMO at 4x (504x512 on screen) looping react, pointer on it
  small   Nim 32x32 at its shipped 4x looping react (12 fps), pointer on it;
          --scale 1 for Nim at 1x

The bench copy keeps the shipped art, fps, and size (unless --scale says
otherwise) and replaces the Behaviors with one that repeats the scenario's
animation, so the Static Director keeps playing it. Its cursor reactions are
the default, indifferent.

The rate counts only updates that land while the scenario's animation is on
screen, over the time it was on screen. Each update is "full" (the pixmap is
rebuilt) or "position-only" (the cached pixmap moves).

Build with: cargo build --release -p fidget
Needs xdotool for every scenario but idle.
EOF_USAGE
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --bin) bin="$2" && shift 2 ;;
    --scale) scale="$2" && shift 2 ;;
    --help | -h) usage ;;
    --) shift && break ;;
    --*) echo "Unknown option: $1" >&2 && exit 2 ;;
    *) break ;;
  esac
done
[ $# -gt 0 ] && scenario="$1" && shift
[ $# -gt 0 ] && target_updates="$1" && shift
[ $# -gt 0 ] && deadline_sec="$1" && shift

# The shipped package, its scale (empty keeps the manifest's), and the
# animation the scenario times (empty for idle: every update counts).
case "$scenario" in
  idle) package="bmo" steady="" ;;
  walk) package="bmo" steady="walk" ;;
  fast) package="bmo" steady="react" ;;
  large) package="bmo" steady="react" scale="${scale:-4}" ;;
  small) package="nim" steady="react" ;;
  *)
    echo "Unknown scenario: $scenario" >&2
    echo "Run with --help for usage" >&2
    exit 2
    ;;
esac

[ -x "$bin" ] || {
  echo "Binary not found: $bin" >&2
  echo "Build with: cargo build --release -p fidget" >&2
  exit 2
}
follow=0
if [ -n "$steady" ]; then
  if [ -n "${DISPLAY:-}" ] && command -v xdotool > /dev/null; then
    follow=1
  else
    echo "No DISPLAY or xdotool: the pointer stays put, so the gate may stay shut." >&2
  fi
fi

here=$(dirname "${BASH_SOURCE[0]}")
follower="$here/lib/follow-sprite.sh"
out=$(mktemp -d -t fidget-mask-measure-XXXXXX)
characters="$out/characters"
mkdir -p "$characters" "$out/home" "$out/data"
cp -R "$here/../characters/$package" "$characters/$package"
# One Behavior that plays the steady animation many times over, the scale
# when given, and no [cursor] table. The steady animation loops, so a
# once-only one (react) cycles its frames at its fps.
manifest="$characters/$package/character.manifest"
awk -v scale="$scale" -v steady="${steady:-idle}" '
  BEGIN { if (scale != "") print "scale = " scale }
  /^\[/ { skip = ($0 ~ /^\[(cursor|behaviors\.[^]]*)\]$/); section = $0 }
  skip { next }
  section == "[animations." steady "]" && /^loop *=/ { next }
  scale != "" && /^scale *=/ { next }
  { print }
  END {
    printf "\n[behaviors.bench]\nplay = ["
    for (i = 1; i <= 50; i++) printf "%s\"%s\"", (i > 1 ? ", " : ""), steady
    print "]"
  }' "$here/../characters/$package/character.manifest" > "$manifest"
log="$out/data/process.log"
: > "$log"
updates="$out/updates.log"
: > "$updates"

pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill -- "-$pid" "$pid" 2> /dev/null || true; done
  wait 2> /dev/null || true
}
trap cleanup EXIT
# A signal has to end the run; cleanup is the EXIT trap's job.
trap 'exit 130' INT TERM

# As of 2026-10, bash 5.2 can fail to parse a trap that fires in $(...) and
# can hang on SIGINT as a foreground child exits. With the traps set, run
# no $(...), and run children in the background and `wait` on them.
in_background() {
  "$@" &
  wait "$!"
}

echo "=== X11 Mask Rebuild Measurement ==="
echo "Scenario: $scenario ($package${scale:+ at ${scale}x}${steady:+, timing $steady})"
echo "Target updates: $target_updates"
echo "Deadline: ${deadline_sec}s"
echo "Binary: $bin"
echo "Run folder: $out"
echo ""

# Idle parks the pointer where the sprite never goes.
[ -z "$steady" ] && [ -n "${DISPLAY:-}" ] && command -v xdotool > /dev/null &&
  in_background xdotool mousemove 0 0

# The app writes stderr, traces included, to the process log in its data
# folder, so that file is the log to read. A pointer resting on the sprite
# shows the hover bubble after 1.5 s, and the bubble stops a walk; the
# hour-long hover delay keeps the sprite walking under the pointer.
env -u FIDGET_DIRECTOR_API_KEY -u XAI_API_KEY -u XDG_DATA_HOME \
  HOME="$out/home" FIDGET_DATA_DIR="$out/data" FIDGET_DIRECTOR=0 \
  FIDGET_HOVER_DELAY_MS=3600000 \
  FIDGET_TRACE_MASK_REBUILD=1 FIDGET_TRACE_FRAMES=1 \
  FIDGET_CHARACTERS="$characters" FIDGET_INSTANCES="$package" \
  "$bin" > "$out/stdout.log" 2>&1 &
pids+=("$!")

# One reader over the log, from its first line. Every wait below is a read
# on it, so each step returns on the line it waits for.
exec {feed}< <(exec tail -n +1 -F "$log" 2> /dev/null)
pids+=("$!")

now_us() { now=${EPOCHREALTIME/./}; }
# Reads the next line into $line, or fails once $1 (microseconds) passes.
next_line() {
  now_us
  local left=$(($1 - now))
  [ "$left" -gt 0 ] || return 1
  printf -v left '%d.%06d' $((left / 1000000)) $((left % 1000000))
  IFS= read -r -t "$left" -u "$feed" line
}

echo -n "Waiting for overlay ready..."
now_us
until_us=$((now + startup_sec * 1000000))
ready=""
while next_line "$until_us"; do
  case "$line" in *'overlay: '*' display'*)
    ready=$line
    break
    ;;
  esac
done
[ -n "$ready" ] || {
  echo " TIMEOUT"
  echo "No overlay line within ${startup_sec}s. Log tail:"
  in_background tail -20 "$log"
  exit 1
}
echo " ready"

# The pointer rides the middle of the sprite box wherever the sprite goes.
# The follower runs in its own process group so cleanup stops its tail too.
if [ "$follow" -eq 1 ] && [[ "$ready" =~ sprite\ ([0-9]+)x([0-9]+) ]]; then
  setsid "$follower" "$log" "$((BASH_REMATCH[1] / 2))" "$((BASH_REMATCH[2] / 2))" &
  pids+=("$!")
fi

# The clock runs while the steady animation is on screen: from a frame line
# that names it to the next frame line that names another. Idle has no
# steady animation, so its clock runs from the first grounded frame on.
echo -n "Waiting for the ${steady:-grounded} animation..."
settle=" ${steady}#"
[ -n "$steady" ] || settle=" Grounded "
settled=0
while next_line "$until_us"; do
  case "$line" in
    *' frame: '*"$settle"*)
      settled=1
      break
      ;;
  esac
done
[ "$settled" -eq 1 ] || {
  echo " TIMEOUT"
  echo "The ${steady:-grounded} animation did not start within ${startup_sec}s. Log tail:"
  in_background tail -20 "$log"
  exit 1
}
echo " playing"

echo "Waiting for $target_updates mask updates (deadline: ${deadline_sec}s)..."
now_us
start=$now
until_us=$((start + deadline_sec * 1000000))
on=1
since=$start
clock=0
count=0
# A target of 0 runs out the deadline, for idle.
while { [ "$target_updates" -eq 0 ] || [ "$count" -lt "$target_updates" ]; } && next_line "$until_us"; do
  case "$line" in
    *' frame: '*)
      now_us
      case "$line" in
        *" ${steady}#"*) [ "$on" -eq 1 ] || { on=1 && since=$now; } ;;
        *) [ -z "$steady" ] || [ "$on" -eq 0 ] || { on=0 && clock=$((clock + now - since)); } ;;
      esac
      ;;
    *'mask_rebuild:'*)
      [ "$on" -eq 1 ] || continue
      count=$((count + 1))
      printf '%s\n' "$line" >> "$updates"
      ;;
  esac
done
now_us
[ "$on" -eq 0 ] || clock=$((clock + now - since))

for pid in "${pids[@]}"; do kill -- "-$pid" "$pid" 2> /dev/null || true; done
wait 2> /dev/null || true
pids=()

printf -v seconds '%d.%03d' $((clock / 1000000)) $((clock % 1000000 / 1000))
echo ""
echo "=== Results ($count mask updates in ${seconds}s of ${steady:-the run}) ==="
echo ""
position_only=0
while IFS= read -r line; do
  case "$line" in *'(position-only)'*) position_only=$((position_only + 1)) ;; esac
done < "$updates"
full_rebuilds=$((count - position_only))
echo "Total mask updates: $count"
echo "  Full rebuilds: $full_rebuilds (mask/facing/scale changed, pixmap regenerated)"
echo "  Position-only: $position_only (reused cached pixmap)"
echo ""
[ "$count" -eq 0 ] || in_background awk -v full="$full_rebuilds" -v total="$count" 'BEGIN {
  printf "Full rebuild ratio: %.2f (lower is better; 0.00 = all reuse)\n", full / total
}'

# Per kind: n, median, min-max, mean. Rate is updates per second of the
# steady animation. The trace is wall time, so a rebuild the scheduler
# preempts reads long; CPU per second is the rate at each kind's median.
sed -n 's/.*mask_rebuild: .*, \([0-9.]*\) ms\( (position-only)\)\{0,1\}$/\1\2/p' "$updates" |
  awk '{ print ($2 == "" ? "full" : "position-only"), $1 }' | sort -k1,1 -k2,2n > "$out/times.txt"
# shellcheck disable=SC2016 # The $1 and $2 are awk's.
in_background awk -v secs="$seconds" -v total="$count" -v scenario="$scenario" '
  { k = $1; n[k]++; v[k, n[k]] = $2; sum[k] += $2 }
  END {
    rate = secs > 0 ? total / secs : 0
    printf "Rate: %.1f updates/sec\n", rate
    for (k in n) {
      m = n[k] % 2 ? v[k, (n[k] + 1) / 2] : (v[k, n[k] / 2] + v[k, n[k] / 2 + 1]) / 2
      typical += n[k] * m
      printf "%s: n=%d median %.3f ms, range %.3f to %.3f ms, mean %.3f ms\n", k, n[k], m, v[k, 1], v[k, n[k]], sum[k] / n[k]
    }
    cpu = secs > 0 ? typical / secs : 0
    printf "Mask time per second, at the medians: %.3f ms\n", cpu
    printf "summary: scenario=%s updates=%d seconds=%s rate=%.2f ms_per_s=%.4f\n", scenario, total, secs, rate, cpu
  }' "$out/times.txt"

echo ""
echo "Opaque pixel count:"
# shellcheck disable=SC2016 # The $1 is awk's.
in_background awk 'match($0, /[0-9]+ opaque pixels/) {
    c = substr($0, RSTART, RLENGTH - 14)
    if (!(c in n)) order[++k] = c
    n[c]++
  } END {
    if (k == 0) print "  (none)"
    for (i = 1; i <= k; i++) printf "  %d pixels: %d update(s)\n", order[i], n[order[i]]
  }' "$updates"
echo ""

case "$scenario" in
  idle)
    if [ "$count" -eq 0 ]; then
      echo "Idle: 0 updates, the cursor-over gate held."
    else
      echo "Idle: unexpected updates; the pointer may have crossed the sprite."
    fi
    ;;
  walk)
    [ "$count" -eq 0 ] || echo "Full rebuilds: $(((200 * full_rebuilds + count) / (2 * count)))% of updates"
    echo "Walking moves the sprite every tick, so updates follow the motion;"
    echo "position-only ones reuse the pixmap until the walk frame changes."
    ;;
  *)
    echo "The sprite stands still, so each update is a frame change: the rate"
    echo "is bounded by the animation's fps."
    ;;
esac
echo ""
echo "Log: $log"
[ "$count" -ge "$target_updates" ] || echo "Deadline: $count of $target_updates updates"
