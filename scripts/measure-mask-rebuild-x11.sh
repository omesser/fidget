#!/usr/bin/env bash
# Repeatable measurement of X11 mask rebuild frequency and cost. Stops after N
# mask updates; the deadline is only a failure guard.

set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

bin="target/debug/fidget"
scenario="idle"
target_updates=100
deadline_sec=60

usage() {
  cat << 'EOF'
Usage: measure-mask-rebuild-x11.sh [--bin PATH] [SCENARIO] [TARGET_UPDATES] [DEADLINE]

Measure X11 mask rebuild frequency and cost across scenarios.
Stops after N mask updates (event-driven), with deadline as failure guard.

Scenarios:
  idle       Sprite perched, cursor away (expect 0 updates)
  walk       Sprite walking under cursor (tests position-only optimization)
  fast       Fast animation (BMO react at 10fps) under cursor
  large      Black Mage at scale=3 (fewer opaque pixels, tests scale cost)

Target updates: Number of mask updates to capture (default: 100)
Deadline: Maximum seconds to wait for target updates (default: 60)

Environment:
  Linux VM under X11 (Xvfb or physical X server)
  Compositor optional but recommended for production parity

Output:
  - Full rebuilds vs position-only rebuilds
  - Full rebuild ratio (full_rebuilds / total_updates, lower is better)
  - Average CPU time per rebuild
  - Peak rebuild time

Numbers from a VM are labeled indicative. Physical hardware timing differs.

Examples:
  # Idle baseline (stops immediately, expect 0 updates)
  ./scripts/measure-mask-rebuild-x11.sh idle 0 10

  # Walking under cursor (100 updates, position-only should dominate)
  ./scripts/measure-mask-rebuild-x11.sh walk 100 60

  # Fast animation
  ./scripts/measure-mask-rebuild-x11.sh fast 100 60

  # Large scaled sprite
  ./scripts/measure-mask-rebuild-x11.sh large 100 60
EOF
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --bin)
      bin="$2"
      shift 2
      ;;
    --help | -h) usage ;;
    --) shift && break ;;
    --*)
      echo "Unknown option: $1" >&2
      exit 2
      ;;
    *) break ;;
  esac
done

if [ $# -gt 0 ]; then
  scenario="$1"
  shift
fi
if [ $# -gt 0 ]; then
  target_updates="$1"
  shift
fi
if [ $# -gt 0 ]; then
  deadline_sec="$1"
  shift
fi

case "$scenario" in
  idle | walk | fast | large) ;;
  *)
    echo "Unknown scenario: $scenario" >&2
    echo "Run with --help for usage" >&2
    exit 2
    ;;
esac

[ -x "$bin" ] || {
  echo "Binary not found: $bin" >&2
  echo "Build with: (cd src-tauri && cargo build --bin fidget)" >&2
  exit 2
}

log=$(mktemp -t fidget-mask-measure-XXXXXX.log)
cleanup() {
  if [ -n "${app_pid:-}" ]; then
    kill -TERM "$app_pid" 2> /dev/null || true
    wait "$app_pid" 2> /dev/null || true
  fi
  rm -f "$log"
}
trap cleanup EXIT INT TERM

echo "=== X11 Mask Rebuild Measurement ==="
echo "Scenario: $scenario"
echo "Target updates: $target_updates"
echo "Deadline: ${deadline_sec}s"
echo "Binary: $bin"
echo ""

export FIDGET_TRACE_MASK_REBUILD=1
case "$scenario" in
  large)
    export FIDGET_INSTANCES="Black Mage"
    echo "Character: Black Mage (37×33@3x, ~500 opaque pixels)"
    ;;
  *)
    export FIDGET_INSTANCES="BMO"
    echo "Character: BMO (126×128@1x, ~6300 opaque pixels)"
    ;;
esac

"$bin" > "$log" 2>&1 &
app_pid=$!

echo -n "Waiting for overlay ready..."
last_size=0
stall_count=0
for _ in $(seq 120); do
  if grep -q 'overlay: [0-9]* display' "$log" 2> /dev/null; then
    echo " ready"
    break
  fi
  current_size=$(wc -c < "$log" 2> /dev/null || echo "0")
  if [ "$current_size" -eq "$last_size" ]; then
    stall_count=$((stall_count + 1))
    if [ "$stall_count" -gt 10 ]; then
      echo " STALLED"
      echo "App stalled during startup. Log tail:"
      tail -20 "$log"
      exit 1
    fi
  else
    stall_count=0
  fi
  last_size=$current_size
  sleep 0.25
done

if ! grep -q 'overlay: [0-9]* display' "$log" 2> /dev/null; then
  echo " TIMEOUT"
  echo "App failed to start. Log tail:"
  tail -20 "$log"
  exit 1
fi

echo -n "Waiting for mask trace..."
for _ in $(seq 40); do
  if grep -q 'mask_rebuild:' "$log" 2> /dev/null; then
    echo " trace active"
    break
  fi
  sleep 0.25
done

if ! grep -q 'mask_rebuild:' "$log" 2> /dev/null; then
  echo " FAILED"
  echo "No mask_rebuild trace appeared. Check FIDGET_TRACE_MASK_REBUILD."
  tail -20 "$log"
  exit 1
fi

case "$scenario" in
  walk | fast)
    echo ""
    echo "Note: This scenario requires cursor over sprite during measurement."
    echo "      Starting measurement immediately..."
    ;;
  idle)
    if [ "$target_updates" -eq 0 ]; then
      echo "Idle scenario with target 0: measurement complete (no updates expected)"
    else
      echo "Starting measurement..."
    fi
    ;;
  large)
    echo "Starting measurement..."
    ;;
esac

measure_start=$(date +%s)
echo "Waiting for $target_updates mask updates (deadline: ${deadline_sec}s)..."

while true; do
  update_count=$(grep -c 'mask_rebuild:' "$log" 2> /dev/null || echo "0")

  if [ "$update_count" -ge "$target_updates" ]; then
    measure_end=$(date +%s)
    measure_elapsed=$((measure_end - measure_start))
    echo "Captured $update_count mask updates in ${measure_elapsed}s"
    break
  fi

  now=$(date +%s)
  elapsed=$((now - measure_start))
  if [ "$elapsed" -ge "$deadline_sec" ]; then
    echo "DEADLINE EXCEEDED: Only $update_count updates in ${deadline_sec}s"
    echo "Expected $target_updates updates. Stopping measurement."
    measure_end=$now
    measure_elapsed=$elapsed
    break
  fi

  sleep 0.1
done

kill -TERM "$app_pid" 2> /dev/null || true
wait "$app_pid" 2> /dev/null || true
app_pid=""

echo ""
echo "=== Results ($update_count mask updates in ${measure_elapsed}s) ==="
echo ""

if [ "$update_count" -eq 0 ]; then
  echo "Total mask updates: 0"
  echo ""
  case "$scenario" in
    idle)
      echo "✓ Expected: idle with cursor away shows no updates"
      ;;
    *)
      echo "⚠ Unexpected: this scenario should show updates if cursor was over sprite"
      echo "  Check that cursor was positioned over the sprite during measurement"
      ;;
  esac
  echo ""
  echo "Log: $log (kept for inspection)"
  trap - EXIT INT TERM
  exit 0
fi

grep 'mask_rebuild:' "$log" |
  sed -n 's/.*mask_rebuild: \([0-9]*\)x\([0-9]*\) @\([0-9]*\)x scale, \([0-9]*\) opaque pixels, \([0-9.]*\) ms\(.*\)/\1 \2 \3 \4 \5 \6/p' \
    > /tmp/mask_rebuild_parsed.$$

position_only=$(grep -c '(position-only)' /tmp/mask_rebuild_parsed.$$ 2> /dev/null || echo "0")
full_rebuilds=$((update_count - position_only))

echo "Total mask updates: $update_count"
echo "  Full rebuilds: $full_rebuilds (mask/facing/scale changed, pixmap regenerated)"
echo "  Position-only: $position_only (reused cached pixmap)"
echo ""

rebuild_ratio=$(awk "BEGIN {printf \"%.2f\", $full_rebuilds / $update_count}")
echo "Full rebuild ratio: $rebuild_ratio (lower is better; 0.00 = all reuse)"
if [ "$measure_elapsed" -gt 0 ]; then
  echo "Rate: $(awk "BEGIN {printf \"%.1f\", $update_count / $measure_elapsed}") updates/sec"
fi
echo ""

awk '{
  time = $5
  sum += time
  if (NR == 1 || time > max) max = time
  if (NR == 1 || time < min) min = time
  count++
} END {
  if (count > 0) {
    printf "Rebuild time:\n"
    printf "  Average: %.3f ms\n", sum/count
    printf "  Min: %.3f ms\n", min
    printf "  Max: %.3f ms\n", max
  }
}' /tmp/mask_rebuild_parsed.$$

rm -f /tmp/mask_rebuild_parsed.$$

echo ""
echo "Opaque pixel count:"
if [ -f /tmp/mask_rebuild_parsed.$$ ]; then
  awk '{print $4}' /tmp/mask_rebuild_parsed.$$ | sort -n | uniq -c |
    awk '{printf "  %d pixels: %d rebuild(s)\n", $2, $1}'
fi

echo ""
echo "--- Interpretation ---"
case "$scenario" in
  idle)
    if [ "$update_count" -eq 0 ]; then
      echo "✓ Idle scenario: 0 updates confirms cursor-over gating works"
    else
      echo "⚠ Idle scenario: unexpected updates (cursor may have crossed sprite)"
    fi
    ;;
  walk)
    if [ "$full_rebuilds" -gt 0 ]; then
      pct=$(awk "BEGIN {printf \"%.0f\", 100.0 * $full_rebuilds / $update_count}")
      echo "Full rebuilds: ${pct}% of updates"
      if [ "$pct" -lt 20 ]; then
        echo "✓ Optimization working: majority reuse cached pixmap"
      else
        echo "⚠ Higher than expected rebuild ratio"
        echo "  Walking with consistent animation should show mostly position-only"
      fi
    fi
    echo ""
    echo "Walking under cursor triggers updates at motion rate."
    echo "With optimization, most should be position-only (cheap)."
    ;;
  fast | large)
    echo "Fast/large scenarios test rebuild cost at different opaque counts and FPS."
    echo "Full rebuild ratio depends on how often animation frames repeat."
    ;;
esac

echo ""
echo "Environment: Linux VM under Xvfb (numbers are indicative)"
echo "Full log: $log (kept for inspection)"
echo ""

trap - EXIT INT TERM
