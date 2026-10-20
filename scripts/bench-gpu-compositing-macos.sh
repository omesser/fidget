#!/usr/bin/env bash
# GPU compositing cost of the overlay on macOS, same subcommands as the Linux
# twin. Frame rate is N/A (needs Instruments), so `frame:` ticks stand in as
# ticks_hz. All but env and baseline take the screen and need GREEN_LIGHT.

set -euo pipefail
cd "$(dirname "$0")/.."

seconds=15
walk_timeout=180
out=""
bin="${FIDGET_VERIFY_BIN:-target/debug/fidget}"
scenario=""

usage() {
  cat >&2 << EOF
Usage: $0 <env|baseline|idle|walking|chat|multi|hidden|matrix> [--seconds N] [--walk-timeout N] [--bin PATH] [--out DIR]

env and baseline touch nothing on screen. The rest launch fidget, click the
sprite, or cover the display, and need FIDGET_BENCH_GREEN_LIGHT=1.
Watts need sudo for powermetrics; run \`sudo -v\` first for an unattended run.
EOF
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --seconds)
      seconds="$2"
      shift 2
      ;;
    --walk-timeout)
      walk_timeout="$2"
      shift 2
      ;;
    --bin)
      bin="$2"
      shift 2
      ;;
    --out)
      out="$2"
      shift 2
      ;;
    --help | -h) usage ;;
    env | baseline | idle | walking | chat | multi | hidden | matrix)
      scenario="$1"
      shift
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      ;;
  esac
done

[ -n "$scenario" ] || usage

case "$scenario" in
  env | baseline) ;;
  *)
    if [ "${FIDGET_BENCH_GREEN_LIGHT:-}" != 1 ]; then
      echo "$scenario takes over the desktop (launches fidget, moves the cursor, covers the display)." >&2
      echo "Set FIDGET_BENCH_GREEN_LIGHT=1 once the operator has agreed." >&2
      exit 2
    fi
    ;;
esac

out="${out:-$(mktemp -d /tmp/fidget-gpu-bench-XXXXXX)}"
mkdir -p "$out"

APP_PID=""
PROP_PID=""
PM_PID=""
SCRATCH_HOME=""

cleanup() {
  stop_app
  if [ -n "$PROP_PID" ]; then
    kill -TERM "$PROP_PID" 2> /dev/null || true
  fi
  if [ -n "$PM_PID" ]; then
    sudo -n kill -TERM "$PM_PID" 2> /dev/null || true
  fi
}
trap cleanup EXIT

stop_app() {
  if [ -n "$APP_PID" ]; then
    kill -TERM "$APP_PID" 2> /dev/null || true
    sleep 0.3
    kill -KILL "$APP_PID" 2> /dev/null || true
    wait "$APP_PID" 2> /dev/null || true
  fi
  APP_PID=""
  if [ -n "$SCRATCH_HOME" ]; then
    rm -rf "$SCRATCH_HOME"
  fi
  SCRATCH_HOME=""
}

need_bin() {
  [ -x "$bin" ] || {
    echo "no $bin — run: (cd src-tauri && cargo build --bin fidget)" >&2
    exit 2
  }
}

probe_host() {
  MACHINE=$(sysctl -n hw.model)
  OS="$(sw_vers -productVersion) ($(sw_vers -buildVersion))"
  local accel
  accel=$(ioreg -r -d 1 -c IOAccelerator 2> /dev/null || true)
  ACCEL_CLASS=$(printf '%s\n' "$accel" | sed -n 's/.*"IOClass" = "\([^"]*\)".*/\1/p' | head -1)
  ACCEL_MODEL=$(printf '%s\n' "$accel" | sed -n 's/.*"model" = "\([^"]*\)".*/\1/p' | head -1)
  [ -n "$ACCEL_CLASS" ] || ACCEL_CLASS=none
  [ -n "$ACCEL_MODEL" ] || ACCEL_MODEL=unknown
  if printf '%s\n' "$accel" | grep -q '"Device Utilization %"'; then
    IOREG_STATS=present
  else
    IOREG_STATS=absent
  fi
  local displays
  displays=$(system_profiler SPDisplaysDataType 2> /dev/null || true)
  SCREENS=$(printf '%s\n' "$displays" | grep -c 'Resolution:' || true)
  REFRESH_HZ=$(printf '%s\n' "$displays" | sed -n 's/.*@ \([0-9.]*\)Hz.*/\1/p' | head -1)
  [ -n "$REFRESH_HZ" ] || REFRESH_HZ=unknown

  SUDO_OK=0
  if [ "$scenario" != env ]; then
    if sudo -n true 2> /dev/null; then
      SUDO_OK=1
    elif [ -t 0 ] && sudo -v; then
      SUDO_OK=1
    fi
  elif sudo -n true 2> /dev/null; then
    SUDO_OK=1
  fi
  if command -v powermetrics > /dev/null 2>&1; then
    POWERMETRICS=present
  else
    POWERMETRICS=absent
  fi
}

print_env() {
  cat << EOF
machine=$MACHINE
os=$OS
accelerator=$ACCEL_CLASS ($ACCEL_MODEL)
ioreg_stats=$IOREG_STATS
screens=$SCREENS
refresh_hz=$REFRESH_HZ
powermetrics=$POWERMETRICS
sudo_cached=$SUDO_OK
bin=$bin
seconds=$seconds
walk_timeout=$walk_timeout
frame_rate=N/A (presented frames need Instruments Metal System Trace; ticks_hz counts engine frame: lines)
green_light=${FIDGET_BENCH_GREEN_LIGHT:-unset}
EOF
}

log_lines() {
  awk 'END { print NR + 0 }' "$1"
}

rate() {
  local count=$1
  local window=$2
  awk -v c="$count" -v s="$window" 'BEGIN { if (s <= 0) print "N/A"; else printf "%.2f", c / s }'
}

emit() {
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$@" | tee -a "$out/rows.tsv"
}

# One line per second: "<Device Utilization %> <In use system memory bytes>".
# Samples the first accelerator; a Mac with two GPUs reports the first ioreg lists.
ioreg_sample() {
  local stats
  stats=$(ioreg -r -d 1 -c IOAccelerator 2> /dev/null | grep -m1 PerformanceStatistics || true)
  local util mem
  util=$(printf '%s' "$stats" | sed -n 's/.*"Device Utilization %"=\([0-9]*\).*/\1/p')
  mem=$(printf '%s' "$stats" | sed -n 's/.*"In use system memory"=\([0-9]*\).*/\1/p')
  if [ -z "$util" ] || [ -z "$mem" ]; then
    return 1
  fi
  printf '%s %s\n' "$util" "$mem"
}

# Fills GPU_PCT, GPU_ACTIVE, GPU_W, VRAM_MB, GPU_REASON for one window.
measure_gpu() {
  local window=$1
  local name=$2
  local pm="$out/${name}.powermetrics.txt"
  local io="$out/${name}.ioreg.txt"
  GPU_PCT=N/A
  GPU_ACTIVE=N/A
  GPU_W=N/A
  VRAM_MB=N/A
  GPU_REASON=""
  PM_PID=""
  local pm_started=0
  if [ "$SUDO_OK" -eq 1 ] && [ "$POWERMETRICS" = present ]; then
    sudo -n powermetrics -i 1000 -n "$window" --samplers gpu_power -o "$pm" 2> "$out/${name}.powermetrics.err" &
    PM_PID=$!
    pm_started=1
  else
    GPU_REASON="powermetrics skipped (sudo not cached or powermetrics absent)"
  fi
  : > "$io"
  local _
  for _ in $(seq 1 "$window"); do
    ioreg_sample >> "$io" || true
    sleep 1
  done
  if [ -n "$PM_PID" ]; then
    wait "$PM_PID" || true
    PM_PID=""
  fi
  local n
  n=$(log_lines "$io")
  if [ "$n" -gt 0 ]; then
    GPU_PCT=$(awk '{ s += $1 } END { printf "%.1f", s / NR }' "$io")
    VRAM_MB=$(awk '{ s += $2 } END { printf "%.0f", s / NR / 1048576 }' "$io")
    GPU_REASON="ioreg $n samples${GPU_REASON:+. $GPU_REASON}"
  else
    GPU_REASON="ioreg had no Device Utilization field${GPU_REASON:+. $GPU_REASON}"
  fi
  if [ -s "$pm" ]; then
    local summary
    summary=$(python3 scripts/parse-powermetrics.py --process '' "$pm" 2> /dev/null || true)
    GPU_W=$(printf '%s\n' "$summary" | awk '/GPU power avg:/ { printf "%.2f", $4 / 1000 }')
    GPU_ACTIVE=$(printf '%s\n' "$summary" | awk '/GPU HW active residency avg:/ { sub(/%/, "", $6); print $6 }')
    [ -n "$GPU_W" ] || GPU_W=N/A
    [ -n "$GPU_ACTIVE" ] || GPU_ACTIVE=N/A
  elif [ "$pm_started" -eq 1 ]; then
    GPU_REASON="$GPU_REASON. powermetrics wrote nothing, see $out/${name}.powermetrics.err"
  fi
}

sample_row() {
  local name=$1
  local log=$2
  local window=$3
  local notes=$4
  local from ticks hz
  from=$(log_lines "$log")
  measure_gpu "$window" "$name"
  if [ "$name" = baseline ]; then
    ticks=N/A
    hz=N/A
  else
    ticks=$(awk -v from="$from" 'NR > from && /^frame: / { n++ } END { print n + 0 }' "$log")
    hz=$(rate "$ticks" "$window")
    notes="$notes, ticks=$ticks"
  fi
  emit "$name" "$GPU_PCT" "$GPU_ACTIVE" "$GPU_W" "$VRAM_MB" "$hz" "$notes. $GPU_REASON"
}

run_baseline() {
  local log="$out/baseline.log"
  : > "$log"
  local stray
  stray=$(pgrep -x fidget | tr '\n' ' ' || true)
  if [ -n "$stray" ]; then
    echo "baseline refused, fidget already running: pid(s) $stray" >&2
    exit 1
  fi
  sample_row baseline "$log" "$seconds" "no fidget"
}

launch_app() {
  local log=$1
  need_bin
  # Scratch HOME so the bench does not write the user's settings. The API key
  # skips the Keychain read a worktree build would otherwise block on.
  SCRATCH_HOME=$(mktemp -d)
  FIDGET_DIRECTOR_API_KEY=bench-placeholder \
    FIDGET_DIRECTOR=0 \
    FIDGET_TRACE_FRAMES=1 \
    FIDGET_CAPTURABLE=1 \
    FIDGET_INSTANCES="${FIDGET_INSTANCES:-BMO}" \
    FIDGET_CHARACTERS="${FIDGET_CHARACTERS:-$PWD/characters}" \
    HOME="$SCRATCH_HOME" \
    "$bin" > "$log" 2>&1 &
  APP_PID=$!
}

wait_overlay() {
  local log=$1
  local _
  for _ in $(seq 1 40); do
    if grep -q '^overlay: [0-9]* display' "$log" 2> /dev/null; then
      break
    fi
    if ! kill -0 "$APP_PID" 2> /dev/null; then
      return 1
    fi
    sleep 0.5
  done
  grep -q '^overlay: [0-9]* display' "$log" 2> /dev/null || return 1
  # The sprite spawns mid-air; wait for it to land before sampling or clicking.
  for _ in $(seq 1 40); do
    if grep -qE '^frame: .* (Grounded|Perched) ' "$log" 2> /dev/null; then
      return 0
    fi
    sleep 0.25
  done
  return 0
}

run_with_overlay() {
  local name=$1
  local log="$out/${name}.log"
  launch_app "$log"
  if ! wait_overlay "$log"; then
    echo "app did not report an overlay; see $log" >&2
    tail -40 "$log" >&2 || true
    stop_app
    exit 1
  fi
  sleep 2
}

sprite_center() {
  python3 - "$1" << 'PY'
import re, sys
log = open(sys.argv[1]).read()
size = re.search(r"sprite (\d+)x(\d+)", log)
at = re.findall(r"^frame: .* (?:Grounded|Perched) .*? sprite\((-?\d+),(-?\d+)\)", log, re.M)
if not (size and at):
    sys.exit(1)
w, h = map(int, size.groups())
print(int(at[-1][0]) + w // 2, int(at[-1][1]) + h // 2)
PY
}

run_idle() {
  run_with_overlay idle
  sample_row idle "$out/idle.log" "$seconds" "pointer left alone"
  stop_app
}

run_walking() {
  local log="$out/walking.log"
  run_with_overlay walking
  local deadline saw=0
  deadline=$((SECONDS + walk_timeout))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if grep -qE ' (walk|ballwalk)#' "$log"; then
      saw=1
      break
    fi
    if ! kill -0 "$APP_PID" 2> /dev/null; then
      break
    fi
    sleep 1
  done
  if [ "$saw" -ne 1 ]; then
    emit walking N/A N/A N/A N/A N/A "no walk or ballwalk frame within ${walk_timeout}s"
    stop_app
    return 0
  fi
  local from
  from=$(log_lines "$log")
  sample_row walking "$log" "$seconds" "walk frame seen"
  local walks
  walks=$(awk -v from="$from" 'NR > from && / (walk|ballwalk)#/ { n++ } END { print n + 0 }' "$log")
  echo "walking: walk_frames=$walks during the sample"
  stop_app
}

run_chat() {
  local log="$out/chat.log"
  run_with_overlay chat
  local center
  if ! center=$(sprite_center "$log"); then
    emit chat N/A N/A N/A N/A N/A "no Grounded or Perched frame line to aim a double-click"
    stop_app
    return 0
  fi
  # click-cursor.swift warps the cursor and posts real HID clicks; osascript
  # cannot click a borderless panel that presents no AX element (-25208).
  # shellcheck disable=SC2086
  swift scripts/click-cursor.swift $center 2 > "$out/click.log" 2>&1 || true
  local _ opened=0
  for _ in $(seq 1 20); do
    if grep -qE 'verbs:.*Summon' "$log"; then
      opened=1
      break
    fi
    sleep 0.25
  done
  if [ "$opened" -ne 1 ]; then
    emit chat N/A N/A N/A N/A N/A "double-click at $center did not log Summon, see $out/click.log"
    stop_app
    return 0
  fi
  sleep 2
  sample_row chat "$log" "$seconds" "Summon logged at $center, pointer left on sprite"
  stop_app
}

run_multi() {
  if [ "$SCREENS" -lt 2 ]; then
    emit multi N/A N/A N/A N/A N/A "host has ${SCREENS} display"
    return 0
  fi
  run_with_overlay multi
  local reported
  reported=$(sed -n 's/^overlay: \([0-9]*\) display.*/\1/p' "$out/multi.log" | head -1)
  sample_row multi "$out/multi.log" "$seconds" "screens=$SCREENS overlay displays=${reported:-unknown}"
  stop_app
}

run_hidden() {
  local log="$out/hidden.log"
  run_with_overlay hidden
  # The prop outlives the sample and quits on its own, so an interrupted run
  # leaves no window behind.
  swift scripts/fullscreen-window.swift $((seconds + 60)) > "$out/fullscreen.log" 2>&1 &
  PROP_PID=$!
  local _ hidden=0
  for _ in $(seq 1 40); do
    if grep -q '^presence: hidden' "$log"; then
      hidden=1
      break
    fi
    sleep 0.25
  done
  if [ "$hidden" -ne 1 ]; then
    emit hidden N/A N/A N/A N/A N/A "fullscreen prop did not log presence hidden, see $out/fullscreen.log"
    kill -TERM "$PROP_PID" 2> /dev/null || true
    PROP_PID=""
    stop_app
    return 0
  fi
  sleep 2
  local from
  from=$(log_lines "$log")
  sample_row hidden "$log" "$seconds" "presence hidden under fullscreen prop"
  if awk -v from="$from" 'NR > from && /^presence: shown/ { found = 1 } END { exit !found }' "$log"; then
    echo "hidden: presence flipped back to shown during the sample, row is not a hidden sample" >&2
  fi
  kill -TERM "$PROP_PID" 2> /dev/null || true
  PROP_PID=""
  stop_app
}

probe_host
echo "scenario	gpu_pct	gpu_active_pct	gpu_w	vram_mb	ticks_hz	notes" > "$out/rows.tsv"

print_env
case "$scenario" in
  env) ;;
  baseline) run_baseline ;;
  idle) run_idle ;;
  walking) run_walking ;;
  chat) run_chat ;;
  multi) run_multi ;;
  hidden) run_hidden ;;
  matrix)
    run_baseline
    run_idle
    run_walking
    run_chat
    run_multi
    run_hidden
    ;;
esac

echo "out=$out"
column -t -s $'\t' "$out/rows.tsv" || cat "$out/rows.tsv"
