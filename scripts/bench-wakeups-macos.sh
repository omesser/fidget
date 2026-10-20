#!/usr/bin/env bash
# Measures wakeups/sec, package idle residency and CPU time for a release fidget
# binary, per scenario, so a reviewer reruns it rather than trusting a doc.

# Needs sudo (powermetrics) and swift. Usage:
# scripts/bench-wakeups-macos.sh --binary PATH --scenario idle|chat|hidden
# [--duration SECS] [--out DIR]; baseline launches nothing.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

BINARY=""
SCENARIO=""
DURATION=30
OUT=".verify/bench-$(date +%Y%m%d-%H%M%S)"

while [ $# -gt 0 ]; do
  case "$1" in
    --binary)
      BINARY="$2"
      shift 2
      ;;
    --scenario)
      SCENARIO="$2"
      shift 2
      ;;
    --duration)
      DURATION="$2"
      shift 2
      ;;
    --out)
      OUT="$2"
      shift 2
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

case "$SCENARIO" in idle | chat | hidden | baseline) ;; *)
  echo "bench-wakeups-macos: --scenario must be idle, chat, hidden or baseline" >&2
  exit 2
  ;;
esac
if [ "$SCENARIO" != "baseline" ] && [ ! -x "$BINARY" ]; then
  echo "bench-wakeups-macos: --binary $BINARY not executable" >&2
  exit 2
fi

mkdir -p "$OUT"
LOG="$OUT/app.log"
PM="$OUT/powermetrics.txt"
META="$OUT/meta.txt"

APP_PID=""
PROP_PID=""
trap 'kill "$APP_PID" "$PROP_PID" 2> /dev/null; wait "$APP_PID" "$PROP_PID" 2> /dev/null' EXIT INT TERM

# Other agents build and run fidget on this machine. A baseline with one of
# theirs alive is not a baseline, and powermetrics cannot tell whose it is.
STRAY_BEFORE=$(pgrep -x fidget | tr '\n' ' ')
if [ "$SCENARIO" = "baseline" ] && [ -n "$STRAY_BEFORE" ]; then
  echo "bench-wakeups-macos: baseline refused, fidget already running: pid(s) $STRAY_BEFORE" >&2
  exit 1
fi

if [ "$SCENARIO" != "baseline" ]; then
  FIDGET_DIRECTOR_API_KEY=bench-placeholder \
    FIDGET_DIRECTOR=0 \
    FIDGET_TRACE_FRAMES=1 \
    FIDGET_TRACE_ENGINE=1 \
    FIDGET_CAPTURABLE=1 \
    "$BINARY" > "$LOG" 2>&1 &
  APP_PID=$!
  echo "launched pid=$APP_PID binary=$BINARY log=$LOG" >&2
fi

READY=$([ "$SCENARIO" = "baseline" ] && echo 1 || echo 0)
for _ in $(seq 1 40); do
  [ "$READY" -eq 1 ] && break
  grep -q '^overlay:' "$LOG" 2> /dev/null && {
    READY=1
    break
  }
  kill -0 "$APP_PID" 2> /dev/null || break
  perl -e 'select(undef,undef,undef,0.25)'
done
if [ "$READY" -ne 1 ]; then
  echo "bench-wakeups-macos: app never reported '^overlay:' - see $LOG" >&2
  exit 1
fi

# Let the first ticks settle before driving Summon or starting the idle clock:
# the sprite spawns mid-air and plays Falling until it lands, and a click during
# that window can miss the moving art, so wait for Grounded.
if [ -n "$APP_PID" ]; then
  for _ in $(seq 1 40); do
    grep -qE '^frame: .* (Grounded|Perched) ' "$LOG" 2> /dev/null && break
    perl -e 'select(undef,undef,undef,0.25)'
  done
  sleep 1
fi

PRESENCE_STATUS="n/a"
if [ "$SCENARIO" = "hidden" ]; then
  # The prop outlives the capture by a margin and quits on its own, so an
  # interrupted run leaves no window behind.
  swift scripts/fullscreen-window.swift $((DURATION + 60)) > "$OUT/fullscreen.log" 2>&1 &
  PROP_PID=$!
  for _ in $(seq 1 40); do
    grep -q '^presence: hidden' "$LOG" 2> /dev/null && break
    perl -e 'select(undef,undef,undef,0.25)'
  done
  if ! grep -q '^presence: hidden' "$LOG"; then
    echo "bench-wakeups-macos: sprite never reported 'presence: hidden' - see $OUT/fullscreen.log and $LOG" >&2
    exit 1
  fi
  # Past the fade, so the sample holds no visible frames.
  sleep 2
fi

SUMMON_STATUS="n/a"
if [ "$SCENARIO" = "chat" ]; then
  SPRITE_AT=$(
    python3 - "$LOG" << 'PY'
import re, sys
log = open(sys.argv[1]).read()
size = re.search(r"sprite (\d+)x(\d+)", log)
at = re.findall(r"^frame: .* (?:Grounded|Perched) .*? sprite\((-?\d+),(-?\d+)\)", log, re.M)
if not (size and at):
    sys.exit(1)
print(int(at[-1][0]), int(at[-1][1]), *size.groups())
PY
  )
  if [ -z "$SPRITE_AT" ]; then
    echo "bench-wakeups-macos: could not read sprite position for chat scenario" >&2
    exit 1
  fi
  read -r SX SY SW SH <<< "$SPRITE_AT"
  CX=$((SX + SW / 2))
  CY=$((SY + SH / 2))
  # `osascript ... click at` resolves an AX element under the point first, and
  # this borderless panel presents none (fails -25208). click-cursor.swift warps
  # the cursor and posts real HID events instead. `2` clicks reads as one Summon.
  swift scripts/click-cursor.swift "$CX" "$CY" 2 > "$OUT/click.log" 2>&1
  sleep 1
  if grep -qE 'verbs:.*Summon' "$LOG"; then
    SUMMON_STATUS="confirmed"
  else
    SUMMON_STATUS="NOT CONFIRMED - see $OUT/click.log and $LOG"
    echo "bench-wakeups-macos: $SUMMON_STATUS" >&2
  fi
  sleep 2
fi

START_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)
START_MS=$(($(date +%s) * 1000))
sudo powermetrics -i 1000 -n "$DURATION" --samplers tasks,cpu_power -o "$PM"
PM_STATUS=$?
END_ISO=$(date -u +%Y-%m-%dT%H:%M:%SZ)

if [ "$SCENARIO" = "hidden" ]; then
  # Hidden for the whole sample, or not a hidden sample: a `shown` after the
  # `hidden` means the prop was buried and the sprite came back mid-capture.
  if [ "$(grep -c '^presence: shown' "$LOG")" -gt 0 ]; then
    PRESENCE_STATUS="NOT HIDDEN THROUGHOUT - presence flipped back to shown, see $LOG"
    echo "bench-wakeups-macos: $PRESENCE_STATUS" >&2
    PM_STATUS=1
  else
    PRESENCE_STATUS="$(grep '^presence: hidden' "$LOG" | tail -1), frames traced during sample: $(awk -v s="$START_MS" '/^frame: / && $2 + 0 >= s + 0' "$LOG" | wc -l | tr -d ' ')"
  fi
fi

{
  echo "scenario: $SCENARIO"
  echo "binary: ${BINARY:-none}"
  echo "binary_mtime: $([ -n "$BINARY" ] && stat -f '%Sm' "$BINARY" || echo n/a)"
  echo "duration_secs: $DURATION"
  echo "start: $START_ISO"
  echo "end: $END_ISO"
  echo "pid: ${APP_PID:-none}"
  echo "note: powermetrics is system-wide - another agent's fidget build"
  echo "  can be running at the same time. Always parse with --pid ${APP_PID:-none},"
  echo "  never bare --process name matching."
  echo "fidget_pids_before: ${STRAY_BEFORE:-none}"
  echo "fidget_pids_after: $(pgrep -x fidget | tr '\n' ' ')"
  echo "load_average: $(sysctl -n vm.loadavg)"
  echo "cargo_or_rustc_running: $(pgrep -x 'cargo|rustc' | wc -l | tr -d ' ')"
  echo "summon: $SUMMON_STATUS"
  echo "presence: $PRESENCE_STATUS"
  echo "powermetrics_exit: $PM_STATUS"
  echo "machine: $(sysctl -n hw.model)"
  echo "os: $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
  echo "git_rev: $(git rev-parse --short HEAD 2> /dev/null || echo unknown)"
} > "$META"

cat "$META"
[ "$PM_STATUS" -eq 0 ] || exit 1
