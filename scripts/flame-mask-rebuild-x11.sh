#!/usr/bin/env bash
# CPU flamegraph of the X11 click-through mask path (#428). Launches fidget
# with the pointer following the sprite, so the cursor-over gate lets mask
# updates through, and samples fidget and the X server until N updates land.

set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

bin="target/release/fidget"
character="BMO"
characters="$PWD/characters"
updates=200
freq=4999
deadline=120
out=""
profile=1

usage() {
  cat << 'EOF_USAGE'
Usage: flame-mask-rebuild-x11.sh [--bin PATH] [--character NAME]
         [--characters DIR] [--updates N] [--freq HZ] [--deadline SEC]
         [--out DIR] [--no-perf]

Runs fidget on $DISPLAY, moves the pointer onto the sprite on every frame
line, and stops after N mask_rebuild lines (the deadline is a failure guard).

With perf (the default) it samples fidget and the X server (user space,
cpu-clock at --freq, default 4999 Hz, DWARF stacks) and writes, under --out:
  flame.svg       every sampled stack
  mask.svg        fidget stacks through update_input_region
  server.svg      X server stacks
  folded.txt      the collapsed stacks behind them
--no-perf skips sampling, for timings that perf does not slow down.

Build with debug line tables so release frames have names:
  CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release -p fidget
Needs perf, xdotool, inferno-collapse-perf and inferno-flamegraph
(cargo install inferno); rustfilt (cargo install rustfilt) demangles.
EOF_USAGE
  exit 0
}

while [ $# -gt 0 ]; do
  case "$1" in
    --bin) bin="$2" && shift 2 ;;
    --character) character="$2" && shift 2 ;;
    --characters) characters="$2" && shift 2 ;;
    --updates) updates="$2" && shift 2 ;;
    --freq) freq="$2" && shift 2 ;;
    --deadline) deadline="$2" && shift 2 ;;
    --out) out="$2" && shift 2 ;;
    --no-perf) profile=0 && shift ;;
    --help | -h) usage ;;
    *) echo "Unknown argument: $1" >&2 && exit 2 ;;
  esac
done

need=(xdotool)
[ "$profile" -eq 1 ] && need+=(perf inferno-collapse-perf inferno-flamegraph)
for tool in "${need[@]}"; do
  command -v "$tool" > /dev/null || { echo "$tool is not on PATH" >&2 && exit 2; }
done
[ -n "${DISPLAY:-}" ] || { echo "DISPLAY is not set" >&2 && exit 2; }
[ -x "$bin" ] || { echo "Binary not found: $bin" >&2 && exit 2; }

follower="$(dirname "${BASH_SOURCE[0]}")/lib/follow-sprite.sh"
out=${out:-$(mktemp -d -t fidget-mask-flame-XXXXXX)}
# The app sends stderr, traces included, to the process log in its data
# folder, so that file is the log to read.
mkdir -p "$out/home/.local/share/fidget"
log="$out/home/.local/share/fidget/process.log"
: > "$log"

pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill -- "-$pid" "$pid" 2> /dev/null || true; done
  wait 2> /dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Counts lines matching $1 that reach the log after this call, up to $2 of
# them, for at most $3 seconds. Event-driven: grep returns on the Nth match.
count_new() {
  timeout "$3" tail -n 0 -F "$log" 2> /dev/null | grep -m "$2" -c -- "$1" || true
}

env -u FIDGET_DIRECTOR_API_KEY -u XAI_API_KEY -u XDG_DATA_HOME HOME="$out/home" FIDGET_DIRECTOR=0 \
  FIDGET_TRACE_MASK_REBUILD=1 FIDGET_TRACE_FRAMES=1 \
  FIDGET_CHARACTERS="$characters" FIDGET_INSTANCES="$character" \
  "$bin" > "$out/stdout.log" 2>&1 &
app=$!
pids+=("$app")

# The mask only updates while the pointer is over the art, so the pointer
# rides the middle of the sprite box wherever the sprite goes. The size
# comes from the overlay line; the follower runs in its own process group so
# cleanup stops its tail too.
ready=$(timeout 60 tail -n +1 -F "$log" 2> /dev/null | grep -m 1 -o 'overlay: [0-9]* display(s); sprite [0-9]*x[0-9]*' || true)
[[ "$ready" =~ sprite\ ([0-9]+)x([0-9]+) ]] || {
  echo "No overlay line within 60s. Log tail:" >&2
  tail -20 "$log" >&2
  exit 1
}
half_w=$((BASH_REMATCH[1] / 2))
half_h=$((BASH_REMATCH[2] / 2))
setsid "$follower" "$log" "$half_w" "$half_h" &
pids+=("$!")

first=$(count_new 'mask_rebuild:' 1 60)
[ "${first:-0}" -ge 1 ] || {
  echo "No mask_rebuild line within 60s. Log tail:" >&2
  tail -20 "$log" >&2
  exit 1
}

server=$(pgrep -o -f "^X[a-z]* ${DISPLAY%.*}( |$)" || true)
server_name=${server:+$(ps -o comm= -p "$server")}
if [ "$profile" -eq 1 ]; then
  targets=$app${server:+,$server}
  perf record -e cpu-clock:u -F "$freq" --call-graph dwarf,16384 -p "$targets" \
    -o "$out/perf.data" 2> "$out/perf.err" &
  perf_pid=$!
  pids+=("$perf_pid")
fi

from=$(wc -l < "$log")
start=$SECONDS
got=$(count_new 'mask_rebuild:' "$updates" "$deadline")
elapsed=$((SECONDS - start))

if [ "$profile" -eq 1 ]; then
  kill -INT "$perf_pid"
  wait "$perf_pid" 2> /dev/null || true
fi
kill -TERM "$app" 2> /dev/null || true
wait "$app" 2> /dev/null || true

tail -n +"$((from + 1))" "$log" | grep 'mask_rebuild:' > "$out/mask.log" || true
echo "character=$character updates=$got/$updates seconds=$elapsed out=$out"
awk '{
    match($0, /[0-9.]+ ms/); ms = substr($0, RSTART, RLENGTH - 3) + 0
    kind = /position-only/ ? "position-only" : "full"
    n[kind]++; sum[kind] += ms
    if (!(kind in lo) || ms < lo[kind]) lo[kind] = ms
    if (ms > hi[kind]) hi[kind] = ms
  } END {
    for (k in n) printf "%s: %d updates, mean %.3f ms, range %.3f to %.3f ms\n", k, n[k], sum[k] / n[k], lo[k], hi[k]
  }' "$out/mask.log"
grep -o '[0-9]* opaque pixels' "$out/mask.log" | sort -n | uniq -c | sort -rn | head -3

[ "$got" -ge "$updates" ] || { echo "Deadline: $got of $updates updates" >&2 && exit 1; }
[ "$profile" -eq 1 ] || exit 0

# --no-inline: resolving inline frames runs addr2line per address and takes
# minutes. Release inlines apply_input_mask, so update_input_region is the
# mask path's root. rustfilt, when present, turns v0 symbols into paths.
demangle="cat"
command -v rustfilt > /dev/null && demangle=rustfilt
perf script --no-inline -i "$out/perf.data" 2> /dev/null | inferno-collapse-perf --all | "$demangle" > "$out/folded.txt"
total=$(awk '{ n += $NF } END { print n + 0 }' "$out/folded.txt")
inferno-flamegraph --title "fidget mask updates, $character ($got in ${elapsed}s)" \
  < "$out/folded.txt" > "$out/flame.svg"
grep 'update_input_region' "$out/folded.txt" |
  inferno-flamegraph --title "fidget update_input_region, $character" > "$out/mask.svg" || true
if [ -n "$server" ]; then
  grep '^X[a-z]*;' "$out/folded.txt" |
    inferno-flamegraph --title "X server, $character" > "$out/server.svg" || true
fi

# On-CPU time per process, then per frame at or below each root (inclusive:
# a frame counts once per stack that passes through it). cpu-clock weights
# are nanoseconds; frames can hold spaces, so the weight is the last field.
ms() { awk '{ printf "%9.1f ms %s\n", $1 / 1e6, substr($0, index($0, " ") + 1) }'; }
echo "on-CPU total: $(echo "$total" | awk '{ printf "%.1f ms", $1 / 1e6 }')"
awk '{ split($0, f, ";"); n[f[1]] += $NF } END { for (p in n) print n[p], p }' \
  "$out/folded.txt" | sort -rn | ms
for root in update_input_region ${server_name:-}; do
  awk -v root="$root" '{
      c = $NF; stack = substr($0, 1, length($0) - length($NF) - 1)
      k = split(stack, f, ";"); below = 0; delete seen
      for (i = 1; i <= k; i++) {
        if (!below && (i == 1 ? f[i] == root : index(f[i], root) > 0)) below = 1
        if (below && !(f[i] in seen)) { seen[f[i]] = 1; n[f[i]] += c }
      }
    } END { for (x in n) print n[x], x }' "$out/folded.txt" | sort -rn | ms > "$out/top-$root.txt"
  echo "at or below $root:"
  head -15 "$out/top-$root.txt"
done
