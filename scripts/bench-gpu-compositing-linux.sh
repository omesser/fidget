#!/usr/bin/env bash
# GPU compositing cost of the transparent overlay on Linux (#425).
# mask_rebuild lines are XShapeCombineMask calls. Per-call time belongs to #428.

set -euo pipefail
cd "$(dirname "$0")/.."

seconds=15
walk_timeout=180
shot=""
out=""
bin="target/debug/fidget"
scenario=""

usage() {
  echo "Usage: $0 <env|baseline|idle|walking|chat|multi|hidden|matrix> [--seconds N] [--walk-timeout N] [--shot FILE] [--out DIR] [--bin PATH]" >&2
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
    --shot)
      shot="$2"
      shift 2
      ;;
    --out)
      out="$2"
      shift 2
      ;;
    --bin)
      bin="$2"
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

out="${out:-$(mktemp -d /tmp/fidget-gpu-bench-XXXXXX)}"
mkdir -p "$out"

APP_PID=""
PANEL_PID=""
COVER_PIDS=()
SCRATCH_HOME=""
CLK_TCK=$(getconf CLK_TCK)

stop_covers() {
  if [ "${#COVER_PIDS[@]}" -gt 0 ]; then
    kill -TERM "${COVER_PIDS[@]}" 2> /dev/null || true
  fi
  COVER_PIDS=()
}

cleanup() {
  if [ -n "$APP_PID" ]; then
    kill -TERM "$APP_PID" 2> /dev/null || true
    sleep 0.3
    kill -KILL "$APP_PID" 2> /dev/null || true
  fi
  if [ -n "$PANEL_PID" ]; then
    kill -TERM "$PANEL_PID" 2> /dev/null || true
  fi
  stop_covers
  if [ -n "$SCRATCH_HOME" ]; then
    rm -rf "$SCRATCH_HOME"
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
    echo "no $bin — run: cargo build -p fidget --bin fidget" >&2
    exit 2
  }
}

probe_host() {
  DISPLAY_SERVER=none
  if [ -n "${WAYLAND_DISPLAY:-}" ] && [ -z "${DISPLAY:-}" ]; then
    DISPLAY_SERVER=wayland
  elif [ -n "${WAYLAND_DISPLAY:-}" ] && [ -n "${DISPLAY:-}" ]; then
    DISPLAY_SERVER=xwayland
  elif [ -n "${DISPLAY:-}" ]; then
    DISPLAY_SERVER=x11
  fi

  COMPOSITOR=none
  COMP_PID=""
  local comm
  for comm in mutter gnome-shell kwin_x11 kwin_wayland xfwm4 picom compton sway weston; do
    if pgrep -x "$comm" > /dev/null 2>&1; then
      COMPOSITOR=$comm
      COMP_PID=$(pgrep -xo "$comm")
      break
    fi
  done

  COMPOSITING=unknown
  VBLANK=unknown
  if [ "$COMPOSITOR" = xfwm4 ] && command -v xfconf-query > /dev/null 2>&1; then
    COMPOSITING=$(xfconf-query -c xfwm4 -p /general/use_compositing 2> /dev/null || echo unknown)
    VBLANK=$(xfconf-query -c xfwm4 -p /general/vblank_mode 2> /dev/null || echo unknown)
  fi

  XSERVER=none
  X_PID=""
  for comm in Xtigervnc Xorg Xwayland X; do
    if pgrep -x "$comm" > /dev/null 2>&1; then
      XSERVER=$comm
      X_PID=$(pgrep -xo "$comm")
      break
    fi
  done

  SCREENS=0
  REFRESH_HZ=unknown
  X_VENDOR=unknown
  GEOMETRY=unknown
  if [ -n "${DISPLAY:-}" ] && command -v xrandr > /dev/null 2>&1; then
    SCREENS=$(xrandr --query | awk '/ connected/ { n++ } END { print n + 0 }')
    REFRESH_HZ=$(xrandr --query | awk '/\*/ { for (i = 1; i <= NF; i++) if ($i ~ /\*/) { gsub(/[^0-9.]/, "", $i); print $i; exit } }')
    [ -n "$REFRESH_HZ" ] || REFRESH_HZ=unknown
  fi
  if [ -n "${DISPLAY:-}" ] && command -v xdpyinfo > /dev/null 2>&1; then
    X_VENDOR=$(xdpyinfo | awk -F: '/^vendor string/ { gsub(/^ +/, "", $2); print $2; exit }')
    GEOMETRY=$(xdpyinfo | awk '/dimensions:/ { print $2; exit }')
  fi

  if [ -d /dev/dri ] || [ -e /dev/nvidiactl ]; then
    GPU_DEVICE=present
  else
    GPU_DEVICE=absent
  fi

  GLX_RENDERER=unavailable
  GLX_ACCEL=unavailable
  if [ -n "${DISPLAY:-}" ] && command -v glxinfo > /dev/null 2>&1; then
    local info
    info=$(glxinfo -B 2>&1 || true)
    GLX_RENDERER=$(printf '%s\n' "$info" | awk -F': ' '/OpenGL renderer string/ { print $2; exit }')
    GLX_ACCEL=$(printf '%s\n' "$info" | awk -F': ' '/^    Accelerated/ { print $2; exit }')
    [ -n "$GLX_RENDERER" ] || GLX_RENDERER=unavailable
    [ -n "$GLX_ACCEL" ] || GLX_ACCEL=unavailable
  fi
}

print_env() {
  cat << EOF
display_server=$DISPLAY_SERVER
display=${DISPLAY:-unset}
wayland=${WAYLAND_DISPLAY:-unset}
x_vendor=$X_VENDOR
geometry=$GEOMETRY
screens=$SCREENS
refresh_hz=$REFRESH_HZ
compositor=$COMPOSITOR
compositing=$COMPOSITING
vblank_mode=$VBLANK
x_server=$XSERVER
gpu_device=$GPU_DEVICE
glx_renderer=$GLX_RENDERER
glx_accelerated=$GLX_ACCEL
seconds=$seconds
walk_timeout=$walk_timeout
EOF
}

append_reason() {
  local tool=$1
  local line=$2
  line=$(printf '%s' "$line" | tr '\n' ' ' | sed 's/  */ /g')
  if [ -n "${GPU_REASON:-}" ]; then
    GPU_REASON="$GPU_REASON. $tool: $line"
  else
    GPU_REASON="$tool: $line"
  fi
}

measure_gpu() {
  local window=$1
  GPU_PCT="N/A"
  GPU_TOOL="none"
  GPU_REASON=""

  if command -v nvidia-smi > /dev/null 2>&1; then
    if sample_nvidia "$window"; then
      return 0
    fi
  fi
  if command -v intel_gpu_top > /dev/null 2>&1; then
    if sample_intel "$window"; then
      return 0
    fi
  fi
  if command -v radeontop > /dev/null 2>&1; then
    if sample_radeon "$window"; then
      return 0
    fi
  fi
  if [ -z "$GPU_REASON" ]; then
    GPU_REASON="no radeontop, intel_gpu_top, or nvidia-smi on PATH"
  fi
}

sample_nvidia() {
  local window=$1
  local err total n value
  err=$(mktemp)
  total=0
  n=0
  local _
  for _ in $(seq 1 "$window"); do
    if value=$(nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader,nounits 2> "$err"); then
      value=$(printf '%s\n' "$value" | head -1 | tr -d ' ')
      case "$value" in
        '' | *[!0-9.]*) ;;
        *)
          total=$(awk -v a="$total" -v b="$value" 'BEGIN { print a + b }')
          n=$((n + 1))
          ;;
      esac
    fi
    sleep 1
  done
  if [ "$n" -eq 0 ]; then
    append_reason nvidia-smi "$(head -1 "$err")"
    rm -f "$err"
    return 1
  fi
  GPU_PCT=$(awk -v t="$total" -v n="$n" 'BEGIN { printf "%.1f", t / n }')
  GPU_TOOL=nvidia-smi
  GPU_REASON="mean of $n samples, first GPU"
  rm -f "$err"
  return 0
}

sample_intel() {
  local window=$1
  local err out parsed
  err=$(mktemp)
  out=$(mktemp)
  timeout "$((window + 3))" intel_gpu_top -J -s 1000 > "$out" 2> "$err" || true
  if [ ! -s "$out" ]; then
    append_reason intel_gpu_top "$(head -1 "$err")"
    rm -f "$err" "$out"
    return 1
  fi
  if ! parsed=$(
    python3 - "$out" << 'PY'
import json
import sys

path = sys.argv[1]
text = open(path, encoding="utf-8", errors="replace").read()
dec = json.JSONDecoder()
vals = []
i = 0
while i < len(text):
    while i < len(text) and text[i].isspace():
        i += 1
    if i >= len(text):
        break
    try:
        obj, end = dec.raw_decode(text, i)
    except json.JSONDecodeError:
        break
    i = end
    engines = obj.get("engines") or {}
    render = None
    for key, value in engines.items():
        if key.startswith("Render/3D") and isinstance(value, dict) and "busy" in value:
            render = float(value["busy"])
            break
    if render is not None:
        vals.append(render)
if not vals:
    sys.exit(2)
print(f"{sum(vals) / len(vals):.1f}")
PY
  ); then
    append_reason intel_gpu_top "stdout had no Render/3D busy field"
    rm -f "$err" "$out"
    return 1
  fi
  GPU_PCT=$parsed
  GPU_TOOL=intel_gpu_top
  GPU_REASON="mean Render/3D busy"
  rm -f "$err" "$out"
  return 0
}

sample_radeon() {
  local window=$1
  local err out parsed
  err=$(mktemp)
  out=$(mktemp)
  timeout "$((window + 3))" radeontop -d - -l "$window" > "$out" 2> "$err" || true
  if ! parsed=$(awk '
    /gpu [0-9.]+%/ {
      for (i = 1; i <= NF; i++) {
        if ($i == "gpu" && $(i + 1) ~ /^[0-9.]+%$/) {
          gsub(/%/, "", $(i + 1))
          sum += $(i + 1)
          n++
        }
      }
    }
    END {
      if (n == 0) exit 2
      printf "%.1f", sum / n
    }
  ' "$out"); then
    append_reason radeontop "$(head -1 "$err")"
    rm -f "$err" "$out"
    return 1
  fi
  GPU_PCT=$parsed
  GPU_TOOL=radeontop
  GPU_REASON="mean gpu field"
  rm -f "$err" "$out"
  return 0
}

proc_ticks() {
  awk -F')' '{ split($NF, a, " "); print a[12] + a[13] }' "/proc/$1/stat"
}

cpu_begin() {
  CPU_T0=$(date +%s.%N)
  COMP_T0=""
  X_T0=""
  if [ -n "$COMP_PID" ] && [ -r "/proc/$COMP_PID/stat" ]; then
    COMP_T0=$(proc_ticks "$COMP_PID")
  fi
  if [ -n "$X_PID" ] && [ -r "/proc/$X_PID/stat" ]; then
    X_T0=$(proc_ticks "$X_PID")
  fi
}

cpu_percent() {
  local before=$1
  local pid=$2
  local after elapsed
  if [ -z "$before" ] || [ -z "$pid" ] || [ ! -r "/proc/$pid/stat" ]; then
    echo "N/A"
    return
  fi
  after=$(proc_ticks "$pid")
  elapsed=$(awk -v a="$CPU_T0" -v b="$CPU_T1" 'BEGIN { printf "%.6f", b - a }')
  awk -v d="$((after - before))" -v hz="$CLK_TCK" -v e="$elapsed" 'BEGIN {
    if (e <= 0) { print "N/A"; exit }
    printf "%.1f", (100 * d) / (hz * e)
  }'
}

cpu_end() {
  CPU_T1=$(date +%s.%N)
  COMP_CPU=$(cpu_percent "$COMP_T0" "$COMP_PID")
  X_CPU=$(cpu_percent "$X_T0" "$X_PID")
}

log_lines() {
  awk 'END { print NR + 0 }' "$1"
}

mask_since() {
  local log=$1
  local from=$2
  awk -v from="$from" 'NR > from && /mask_rebuild:/ { n++ } END { print n + 0 }' "$log"
}

rate() {
  local count=$1
  local window=$2
  awk -v c="$count" -v s="$window" 'BEGIN { if (s <= 0) print "N/A"; else printf "%.2f", c / s }'
}

emit() {
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$@" | tee -a "$out/rows.tsv"
}

launch_app() {
  local log=$1
  need_bin
  # Scratch HOME so the bench does not write the user's settings. X11 looks
  # for authority under HOME when XAUTHORITY is unset, and that file is not
  # copied into the scratch directory.
  if [ -z "${XAUTHORITY:-}" ] && [ -f "${HOME}/.Xauthority" ]; then
    export XAUTHORITY="${HOME}/.Xauthority"
  fi
  SCRATCH_HOME=$(mktemp -d)
  FIDGET_TRACE_MASK_REBUILD=1 \
    FIDGET_TRACE_FRAMES=1 \
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
    if grep -q 'overlay: [0-9]* display' "$log" 2> /dev/null; then
      return 0
    fi
    if ! kill -0 "$APP_PID" 2> /dev/null; then
      return 1
    fi
    sleep 0.5
  done
  return 1
}

park_pointer() {
  if [ -n "${DISPLAY:-}" ] && command -v xdotool > /dev/null 2>&1; then
    xdotool mousemove 2 2 > /dev/null 2>&1 || true
  fi
}

sprite_center() {
  local log=$1
  awk '
    /frame:/ { line = $0 }
    END {
      if (line == "") exit 1
      if (!match(line, /pos\([-0-9.]+,[-0-9.]+\)/)) exit 1
      pos = substr(line, RSTART + 4, RLENGTH - 5)
      split(pos, feet, ",")
      if (!match(line, /sprite\([-0-9]+,[-0-9]+\)/)) exit 1
      art = substr(line, RSTART + 7, RLENGTH - 8)
      split(art, corner, ",")
      printf "%d %d\n", feet[1], (corner[2] + feet[2]) / 2
    }
  ' "$log"
}

capture_shot() {
  local dest=$1
  [ -n "$dest" ] || return 0
  [ -n "${DISPLAY:-}" ] || return 0
  command -v ffmpeg > /dev/null 2>&1 || return 0
  command -v xfce4-terminal > /dev/null 2>&1 || return 0
  local panel
  panel=$(mktemp)
  cat > "$panel" << 'EOF'
echo "GPU tools during idle perched"
echo
if command -v intel_gpu_top >/dev/null 2>&1; then
  echo "== intel_gpu_top =="
  timeout 2 intel_gpu_top -J -s 300 || true
  echo
fi
if command -v radeontop >/dev/null 2>&1; then
  echo "== radeontop =="
  timeout 2 radeontop -d - -l 1 || true
  echo
fi
if command -v nvidia-smi >/dev/null 2>&1; then
  echo "== nvidia-smi =="
  nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader || true
  echo
fi
if command -v glxinfo >/dev/null 2>&1; then
  echo "== glxinfo -B =="
  glxinfo -B || true
fi
sleep 600
EOF
  xfce4-terminal --disable-server --display="$DISPLAY" --geometry=40x16 \
    --hide-menubar --hide-scrollbar --hide-borders \
    --title=fidget-gpu-probe -e "bash $panel" > /dev/null 2>&1 &
  PANEL_PID=$!
  sleep 6
  local geom x y
  geom=$(xdotool search --onlyvisible --name fidget-gpu-probe getwindowgeometry --shell 2> /dev/null | head -20 || true)
  x=$(printf '%s\n' "$geom" | awk -F= '/^X=/ { print $2; exit }')
  y=$(printf '%s\n' "$geom" | awk -F= '/^Y=/ { print $2; exit }')
  if [ -n "$x" ] && [ -n "$y" ]; then
    ffmpeg -y -loglevel error -f x11grab -video_size 280x320 -i "${DISPLAY}+${x},${y}" -frames:v 1 "$dest" || true
  fi
  echo "shot=$dest"
}

sample_row() {
  local name=$1
  local log=$2
  local window=$3
  local notes=$4
  local from calls hz started elapsed left walks
  from=$(log_lines "$log")
  started=$(date +%s)
  cpu_begin
  measure_gpu "$window"
  elapsed=$(($(date +%s) - started))
  left=$((window - elapsed))
  if [ "$left" -gt 0 ]; then
    sleep "$left"
  fi
  cpu_end
  if [ "$name" = baseline ]; then
    calls=N/A
    hz=N/A
  else
    calls=$(mask_since "$log" "$from")
    hz=$(rate "$calls" "$window")
  fi
  if [ "$name" = walking ] || [ "$name" = walking-over ]; then
    walks=$(awk -v from="$from" 'NR > from && / (walk|ballwalk)#/ { n++ } END { print n + 0 }' "$log")
    notes="$notes, walk_frames=$walks"
  fi
  emit "$name" "$GPU_PCT" "$GPU_TOOL" "$calls" "$hz" "$COMP_CPU" "$X_CPU" "$notes. $GPU_REASON"
}

run_baseline() {
  local log="$out/baseline.log"
  : > "$log"
  sample_row baseline "$log" "$seconds" "no fidget"
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
  park_pointer
  sleep 1
}

run_idle() {
  run_with_overlay idle
  capture_shot "$shot"
  sample_row idle "$out/idle.log" "$seconds" "pointer at 2,2"
  stop_app
  if [ -n "$PANEL_PID" ]; then
    kill -TERM "$PANEL_PID" 2> /dev/null || true
    PANEL_PID=""
  fi
}

run_walking() {
  local log="$out/walking.log"
  run_with_overlay walking
  local deadline
  deadline=$((SECONDS + walk_timeout))
  local saw=0
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
    emit walking N/A none N/A N/A N/A N/A "no walk or ballwalk frame within ${walk_timeout}s"
    stop_app
    return 0
  fi
  sample_row walking "$log" "$seconds" "pointer away, walk frame seen"
  local center x y
  if center=$(sprite_center "$log"); then
    x=${center%% *}
    y=${center##* }
    xdotool mousemove "$x" "$y" > /dev/null 2>&1 || true
    sleep 1
    sample_row walking-over "$log" 5 "pointer on sprite center ${x},${y}"
  else
    emit walking-over N/A none N/A N/A N/A N/A "walk frame seen but sprite center was not in the log"
  fi
  stop_app
}

run_chat() {
  local log="$out/chat.log"
  run_with_overlay chat
  local center x y
  if ! center=$(sprite_center "$log"); then
    emit chat N/A none N/A N/A N/A N/A "no frame line to aim a double-click"
    stop_app
    return 0
  fi
  x=${center%% *}
  y=${center##* }
  xdotool mousemove "$x" "$y" > /dev/null 2>&1 || true
  xdotool click 1 > /dev/null 2>&1 || true
  sleep 0.12
  xdotool click 1 > /dev/null 2>&1 || true
  local _ opened=0
  for _ in $(seq 1 20); do
    if grep -q 'Summon' "$log"; then
      opened=1
      break
    fi
    sleep 0.25
  done
  if [ "$opened" -ne 1 ]; then
    emit chat N/A none N/A N/A N/A N/A "double-click at ${x},${y} did not log Summon"
    stop_app
    return 0
  fi
  sleep 1
  sample_row chat "$log" "$seconds" "Summon logged, pointer left on sprite"
  stop_app
}

run_multi() {
  if [ "$SCREENS" -lt 2 ]; then
    emit multi N/A none N/A N/A N/A N/A "host has ${SCREENS} display"
    return 0
  fi
  run_with_overlay multi
  local reported
  reported=$(sed -n 's/^overlay: \([0-9]*\) display.*/\1/p' "$out/multi.log" | head -1)
  sample_row multi "$out/multi.log" "$seconds" "xrandr screens=$SCREENS overlay displays=${reported:-unknown}"
  stop_app
}

run_hidden() {
  local log="$out/hidden.log"
  if ! command -v xfce4-terminal > /dev/null 2>&1; then
    emit hidden N/A none N/A N/A N/A N/A "no xfce4-terminal to cover the display"
    return 0
  fi
  run_with_overlay hidden
  # One per monitor: with a display left free, the Character moves there instead
  # of hiding. Each terminal goes fullscreen on the monitor it opens on.
  local origins
  origins=$(xrandr --listmonitors 2> /dev/null | awk 'NR > 1 { sub(/^[^+]*/, "", $3); print $3 }')
  for origin in ${origins:-+0+0}; do
    xfce4-terminal --disable-server --fullscreen --hide-menubar --hide-scrollbar \
      --geometry="80x24$origin" --title=fidget-bench-cover \
      -e "sleep $((seconds + 10))" > /dev/null 2>&1 &
    COVER_PIDS+=("$!")
  done
  local _ hidden=0
  for _ in $(seq 1 20); do
    if grep -q 'presence: hidden' "$log"; then
      hidden=1
      break
    fi
    sleep 0.25
  done
  if [ "$hidden" -ne 1 ]; then
    emit hidden N/A none N/A N/A N/A N/A "fullscreen terminal did not log presence hidden"
    stop_covers
    stop_app
    return 0
  fi
  sleep 1
  sample_row hidden "$log" "$seconds" "presence hidden under fullscreen terminal"
  stop_covers
  stop_app
}

probe_host
{
  echo "scenario	gpu_pct	gpu_tool	mask_calls	mask_hz	compositor_cpu_pct	xserver_cpu_pct	notes"
} | tee "$out/rows.tsv" > /dev/null

case "$scenario" in
  env)
    print_env
    ;;
  baseline)
    print_env
    run_baseline
    ;;
  idle)
    print_env
    run_idle
    ;;
  walking)
    print_env
    run_walking
    ;;
  chat)
    print_env
    run_chat
    ;;
  multi)
    print_env
    run_multi
    ;;
  hidden)
    print_env
    run_hidden
    ;;
  matrix)
    print_env
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
