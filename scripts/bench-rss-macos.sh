#!/usr/bin/env bash
# Sample a running fidget's resident set, macOS only. WKWebView helpers are
# launchd's children, so this diffs WebKit helpers before and after launch.
# Usage: scripts/bench-rss-macos.sh [--bin PATH] [--settle N] [--seconds N] [--interval N] [--out FILE] [--research]

# Compare runs on peak physical footprint, which only rises; RSS drops as a busy
# machine reclaims an idle character's pages. Note roster, displays and sprite
# state beside the number (docs/research/memory-rss-and-multi-monitor.md).

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

settle=3
seconds=10
interval=2
out=""
bin="target/debug/fidget"

while [ $# -gt 0 ]; do
  case "$1" in
    --bin) bin="$2" && shift 2 ;;
    --settle) settle="$2" && shift 2 ;;
    --seconds) seconds="$2" && shift 2 ;;
    --interval) interval="$2" && shift 2 ;;
    --out) out="$2" && shift 2 ;;
    --research) settle=300 && seconds=300 && interval=5 && shift ;;
    *) echo "unknown argument: $1" >&2 && exit 2 ;;
  esac
done

[ -x "$bin" ] || {
  echo "no $bin — run: (cd src-tauri && cargo build --bin fidget)" >&2
  exit 2
}
out="${out:-$(mktemp -t fidget-rss).tsv}"
log="$out.app.log"

# Everything WebKit is already running belongs to some other application.
before=$(pgrep -f com.apple.WebKit || true)

"./$bin" > "$log" 2>&1 &
app=$!
trap 'kill "$app" 2>/dev/null' EXIT INT TERM

# Release keeps stderr in the process log and does not copy it back
# (process_log::init), so the overlay line never reaches $log. Append the
# bytes written after launch. A debug build already printed the line.
process_log="${HOME}/Library/Application Support/fidget/process.log"
process_log_at=0
[ -f "$process_log" ] && process_log_at=$(wc -c < "$process_log" | tr -d ' ')
pull_process_log() {
  [ -f "$process_log" ] || return 0
  now=$(wc -c < "$process_log" | tr -d ' ')
  [ "$now" -gt "$process_log_at" ] || return 0
  tail -c +"$((process_log_at + 1))" "$process_log" >> "$log"
  process_log_at=$now
}

# The overlays are what allocate; sampling before they exist measures a
# half-started app. The line is the one place the app says how many it made.
for _ in $(seq 30); do
  pull_process_log
  grep -q 'overlay: [0-9]* display' "$log" && break
  sleep 1
done
pull_process_log
displays=$(sed -n 's/.*overlay: \([0-9][0-9]*\) display.*/\1/p' "$log" | head -1)
[ -n "$displays" ] || {
  echo "the app never reported its overlays; see $log and $process_log" >&2
  exit 1
}

helpers=$(comm -13 <(echo "$before" | sort) <(pgrep -f com.apple.WebKit | sort))
pids=$(echo "$app $helpers" | tr '\n' ' ' | xargs)

# One WebContent per display, plus the GPU and Networking processes. Any other
# count means another WebKit application started a helper in the same few
# seconds and the set difference caught it: rerun on a quieter machine.
expected=$((displays + 2))
found=$(echo "$helpers" | grep -c .)
[ "$found" -eq "$expected" ] ||
  echo "warning: $found WebKit helpers, expected $expected — another application's are in this set" >&2

echo "displays: $displays   pids: $pids"
echo "settling ${settle}s, then sampling ${seconds}s every ${interval}s -> $out"
sleep "$settle"
printf 'epoch\ttotal_kb\t%s\n' "$(echo "$pids" | tr ' ' '\t')" > "$out"

end=$(($(date +%s) + seconds))
while [ "$(date +%s)" -lt "$end" ]; do
  # A helper that died reports nothing, so the row is short rather than wrong.
  rss=$(ps -o rss= -p "$(echo "$pids" | tr ' ' ',')" 2> /dev/null | tr -d ' ')
  total=$(echo "$rss" | awk '{s += $1} END {print s}')
  printf '%s\t%s\t%s\n' "$(date +%s)" "$total" "$(echo "$rss" | tr '\n' '\t')" >> "$out"
  sleep "$interval"
done

# BSD awk has no asort, so the ordering is sort's and the arithmetic is awk's.
median() { sort -n | awk '{t[n++] = $1} END {printf "%.0f", t[int(n / 2)] / 1024}'; }

awk -F'\t' 'NR > 1 {print $2}' "$out" | sort -n |
  awk '{t[n++] = $1}
    END {
      printf "total   samples: %d   min: %.0f MB   median: %.0f MB   max: %.0f MB\n",
        n, t[0] / 1024, t[int(n / 2)] / 1024, t[n - 1] / 1024
    }'

# Per process, because the per-display cost is one WebContent and nothing else.
# `vmmap` reads the peak while the process is still alive; after the kill below
# there is nothing left to ask.
column=3
for pid in $pids; do
  printf '  %-6s %-28s rss median: %4s MB   footprint peak: %s\n' "$pid" \
    "$(ps -o comm= -p "$pid" 2> /dev/null | xargs -n1 basename)" \
    "$(cut -f "$column" "$out" | tail -n +2 | median)" \
    "$(vmmap --summary "$pid" 2> /dev/null |
      sed -n 's/^Physical footprint (peak): *//p')"
  column=$((column + 1))
done
