#!/usr/bin/env bash
# RSS alone does not compare two runs on a busy machine; VmHWM only ever rises.
# Compare on VmHWM, read the RSS series for shape, and record the roster,
# display count and what the sprite was doing beside the number.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

settle=3
seconds=10
interval=2
out=""
bin="target/debug/fidget"

while [ $# -gt 0 ]; do
  case "$1" in
    --settle) settle="$2" && shift 2 ;;
    --seconds) seconds="$2" && shift 2 ;;
    --interval) interval="$2" && shift 2 ;;
    --out) out="$2" && shift 2 ;;
    --bin) bin="$2" && shift 2 ;;
    --research) settle=300 && seconds=300 && interval=5 && shift ;;
    *) echo "unknown argument: $1" >&2 && exit 2 ;;
  esac
done

[ -x "$bin" ] || {
  echo "no $bin — run: (cd src-tauri && cargo build --bin fidget)" >&2
  exit 2
}
out="${out:-$(mktemp -t fidget-rss-XXXXXX).tsv}"
log="$out.app.log"

"./$bin" > "$log" 2>&1 &
app=$!
trap 'kill -TERM "$app" 2>/dev/null; sleep 1; kill -KILL "$app" 2>/dev/null' EXIT INT TERM

# The overlays are what allocate, so sampling before they exist measures a
# half-started app.
for _ in $(seq 30); do
  grep -q 'overlay: [0-9]* display' "$log" && break
  sleep 1
done
displays=$(sed -n 's/^overlay: \([0-9]*\) display.*/\1/p' "$log" | head -1)
if [ -z "$displays" ]; then
  if grep -qi "error\|failed\|cannot" "$log" 2> /dev/null; then
    echo "app failed to start; see $log" >&2
    cat "$log" >&2
    exit 1
  fi
  echo "the app never reported its overlays; see $log" >&2
  exit 1
fi

sleep 2
# WebKitGTK helpers are children of the main pid, so the resident set is that tree.
children=$(pgrep -P "$app" 2> /dev/null || true)
pids=$(echo "$app" | cat - <(echo "$children") | tr '\n' ' ' | xargs)

pid_count=$(echo "$pids" | wc -w)

echo "displays: $displays   main pid: $app   total processes: $pid_count"
echo "pids: $pids"
echo "settling ${settle}s, then sampling ${seconds}s every ${interval}s -> $out"

ps -p "$app" -o pid,ppid,comm,args 2> /dev/null || true
for child in $children; do
  ps -p "$child" -o pid,ppid,comm,args 2> /dev/null || true
done

sleep "$settle"
printf 'epoch\ttotal_kb\t%s\n' "$(echo "$pids" | tr ' ' '\t')" > "$out"

end=$(($(date +%s) + seconds))
while [ "$(date +%s)" -lt "$end" ]; do
  rss=""
  for pid in $pids; do
    if [ -f "/proc/$pid/status" ]; then
      pid_rss=$(awk '/^VmRSS:/ {print $2}' "/proc/$pid/status" 2> /dev/null || echo "0")
      rss="$rss$pid_rss "
    else
      rss="${rss}0 "
    fi
  done
  total=$(echo "$rss" | awk '{s = 0; for (i = 1; i <= NF; i++) s += $i; print s}')
  printf '%s\t%s\t%s\n' "$(date +%s)" "$total" "$(echo "$rss" | tr ' ' '\t')" >> "$out"
  sleep "$interval"
done

# BSD awk compatibility: no asort, so ordering is sort's.
median() { sort -n | awk '{t[n++] = $1} END {printf "%.0f", t[int(n / 2)] / 1024}'; }

awk -F'\t' 'NR > 1 {print $2}' "$out" | sort -n |
  awk '{t[n++] = $1}
    END {
      printf "total   samples: %d   min: %.0f MB   median: %.0f MB   max: %.0f MB\n",
        n, t[0] / 1024, t[int(n / 2)] / 1024, t[n - 1] / 1024
    }'

column=3
for pid in $pids; do
  comm=$(ps -p "$pid" -o comm= 2> /dev/null | xargs || echo "gone")
  rss_med=$(cut -f "$column" "$out" 2> /dev/null | tail -n +2 | median)
  if [ -f "/proc/$pid/status" ]; then
    vmhwm=$(awk '/^VmHWM:/ {print $2 / 1024 " MB"}' "/proc/$pid/status" 2> /dev/null || echo "N/A")
  else
    vmhwm="N/A (process exited)"
  fi
  printf '  %-6s %-28s rss median: %4s MB   peak (VmHWM): %s\n' "$pid" "$comm" "$rss_med" "$vmhwm"
  column=$((column + 1))
done

kill -TERM "$app" 2> /dev/null
sleep 1
kill -KILL "$app" 2> /dev/null
trap - EXIT INT TERM

echo ""
echo "Log written to: $log"
echo "TSV written to: $out"
