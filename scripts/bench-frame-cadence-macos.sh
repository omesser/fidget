#!/usr/bin/env bash
# Display-frame cadence, dropped frames and interpolation lag on macOS (#426).
# Launches fidget with FIDGET_TRACE_FRAMES (Engine ticks) and
# FIDGET_TRACE_CADENCE (the overlay's display frames), samples a window, and
# reduces it with scripts/frame-cadence.mjs into one report per scenario.
#
# idle: BMO held still by a copy whose only Behavior is `fidget`. It fails if
#   a walk frame lands in the window anyway. Idle runs BMO whatever
#   FIDGET_CHARACTERS says, so FIDGET_INSTANCES must name BMO.
# walking: sampled from the first walk frame.
# load: walking with one `yes` per core running from launch.
# idle-quiet: idle with FIDGET_TRACE_FRAMES off, so the loop's own tick
#   counter says whether the per-tick `frame:` print slows the Engine.
#
# A/B: pass --bin twice. One binary's still-tick rate drifts by several Hz over
# minutes, so each round runs every scenario on both, B first on even rounds,
# and prints each side's mean over --rounds (default 3) and B minus A.
#
# Every scenario launches fidget on the live desktop, so it refuses to run
# unless FIDGET_BENCH_GREEN_LIGHT=1 says the operator agreed to it.

set -euo pipefail
cd "$(dirname "$0")/.."

seconds=20
walk_timeout=180
out=""
bins=()
rounds=""
scenario=""

usage() {
  cat >&2 << EOF
Usage: $0 <idle|idle-quiet|walking|load|matrix> [--seconds N] [--walk-timeout N] [--bin PATH] [--out DIR]
       $0 <scenario> --bin A --bin B [--rounds N] [...]

A second --bin runs A/B: both binaries per scenario, alternating, for N rounds.
Every scenario launches fidget and needs FIDGET_BENCH_GREEN_LIGHT=1.
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
      bins+=("$2")
      shift 2
      ;;
    --rounds)
      rounds="$2"
      shift 2
      ;;
    --out)
      out="$2"
      shift 2
      ;;
    --help | -h) usage ;;
    idle | idle-quiet | walking | load | matrix)
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
[ "${#bins[@]}" -gt 0 ] || bins=("${FIDGET_VERIFY_BIN:-target/debug/fidget}")
[ "${#bins[@]}" -le 2 ] || usage
if [ "${#bins[@]}" -eq 1 ]; then
  [ -z "$rounds" ] || usage
else
  rounds="${rounds:-3}"
  case "$rounds" in '' | 0 | *[!0-9]*) usage ;; esac
fi
if [ "${FIDGET_BENCH_GREEN_LIGHT:-}" != 1 ]; then
  echo "$scenario launches fidget on the live desktop. Set FIDGET_BENCH_GREEN_LIGHT=1 once the operator has agreed." >&2
  exit 2
fi
for bin in "${bins[@]}"; do
  [ -x "$bin" ] || {
    echo "no $bin; run: (cd src-tauri && cargo build --bin fidget)" >&2
    exit 2
  }
done
bin=${bins[0]}

out="${out:-$(mktemp -d /tmp/fidget-cadence-bench-XXXXXX)}"
mkdir -p "$out"

APP_PID=""
SCRATCH_HOME=""
LOG_PATH=""
BURNERS=()

stop_app() {
  # The scratch home holds the only release copy of the trace.
  harvest_process_log "${LOG_PATH:-}"
  if [ -n "$APP_PID" ]; then
    kill -TERM "$APP_PID" 2> /dev/null || true
    sleep 0.3
    kill -KILL "$APP_PID" 2> /dev/null || true
    wait "$APP_PID" 2> /dev/null || true
  fi
  APP_PID=""
  [ -z "$SCRATCH_HOME" ] || rm -rf "$SCRATCH_HOME"
  SCRATCH_HOME=""
}

stop_burners() {
  [ "${#BURNERS[@]}" -eq 0 ] || kill "${BURNERS[@]}" 2> /dev/null || true
  BURNERS=()
}

# Release process_log::init dup2s stderr onto process.log, so the terminal
# redirect stays empty. Debug already teed the same lines into the log.
harvest_process_log() {
  local log=$1 found
  [ -n "$log" ] && [ -f "$log" ] || return 0
  [ -n "${SCRATCH_HOME:-}" ] || return 0
  grep -qE '^(frame|cadence): ' "$log" 2> /dev/null && return 0
  found=$(find "$SCRATCH_HOME" -name process.log -type f 2> /dev/null | head -1 || true)
  [ -n "$found" ] || return 0
  node scripts/frame-cadence.mjs unwrap "$found" >> "$log"
}

trap 'stop_app; stop_burners' EXIT

now_ms() {
  perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'
}

cpu_count() {
  if [ "$(uname -s)" = Darwin ]; then
    sysctl -n hw.ncpu
  else
    nproc
  fi
}

launch_app() {
  local log=$1 frames=$2 characters=$3
  # X11 reads its authority file from HOME, so export that path first.
  if [ -z "${XAUTHORITY:-}" ] && [ -f "${HOME}/.Xauthority" ]; then
    export XAUTHORITY="${HOME}/.Xauthority"
  fi
  SCRATCH_HOME=$(mktemp -d)
  FIDGET_DIRECTOR_API_KEY=bench-placeholder \
    FIDGET_DIRECTOR=0 \
    FIDGET_TRACE_FRAMES="$frames" \
    FIDGET_TRACE_CADENCE=1 \
    FIDGET_INSTANCES="${FIDGET_INSTANCES:-BMO}" \
    FIDGET_CHARACTERS="$characters" \
    HOME="$SCRATCH_HOME" \
    "$bin" > "$log" 2>&1 &
  APP_PID=$!
}

# FIDGET_DIRECTOR=0 still runs the Static Director, which walks BMO on its
# patrol. The idle scenarios run a copy of BMO whose only weighted Behavior is
# `fidget`, which plays `idle`. Prints the search path holding the copy.
still_characters() {
  local dir="$out/still-characters"
  rm -rf "$dir"
  mkdir -p "$dir"
  cp -R characters/bmo "$dir/bmo"
  awk '/^\[/ { section = $0 }
    /^weight = / && section ~ /^\[behaviors\./ && section != "[behaviors.fidget]" { $0 = "weight = 0" }
    1' characters/bmo/character.manifest > "$dir/bmo/character.manifest"
  echo "$dir"
}

# Release traces are in process.log until harvest copies them. The terminal
# file stays empty, so a wait on only that file kills the app before it lands.
log_has() {
  local pattern=$1 log=$2 found
  grep -qE "$pattern" "$log" 2> /dev/null && return 0
  [ -n "${SCRATCH_HOME:-}" ] || return 1
  found=$(find "$SCRATCH_HOME" -name process.log -type f 2> /dev/null | head -1 || true)
  [ -n "$found" ] && grep -qE "$pattern" "$found"
}

# The sprite spawns mid-air; wait for it to land so a fall is not sampled.
wait_landed() {
  local log=$1 _
  for _ in $(seq 1 80); do
    log_has 'frame: .* (Grounded|Perched) ' "$log" && return 0
    kill -0 "$APP_PID" 2> /dev/null || return 1
    sleep 0.25
  done
  return 1
}

wait_walk() {
  local log=$1 deadline=$((SECONDS + walk_timeout))
  while [ "$SECONDS" -lt "$deadline" ]; do
    log_has ' (walk|ballwalk)#' "$log" && return 0
    kill -0 "$APP_PID" 2> /dev/null || return 1
    sleep 0.5
  done
  return 1
}

# The second argument names the run's files, so A/B rounds do not overwrite each other.
run() {
  local name=$1 tag=${2:-$1}
  local log="$out/$tag.log"
  LOG_PATH="$log"
  if [ "$name" = load ]; then
    for _ in $(seq 1 "$(cpu_count)"); do
      yes > /dev/null &
      BURNERS+=($!)
    done
  fi
  local idle=0 characters="${FIDGET_CHARACTERS:-$PWD/characters}"
  if [ "${name%-quiet}" = idle ]; then
    idle=1
    characters=$(still_characters)
  fi
  if [ "$name" = idle-quiet ]; then
    launch_app "$log" 0 "$characters"
    # No `frame:` lines to watch for a landing, so give the fall a fixed 5 s.
    sleep 5
  else
    launch_app "$log" 1 "$characters"
  fi
  if [ "$name" != idle-quiet ] && ! wait_landed "$log"; then
    echo "$name: the sprite never landed; see $log" >&2
    stop_app
    stop_burners
    return 1
  fi
  sleep 2
  if [ "$idle" = 0 ] && ! wait_walk "$log"; then
    echo "$name: no walk frame within ${walk_timeout}s; see $log" >&2
    stop_app
    stop_burners
    return 1
  fi
  local from to walks
  from=$(now_ms)
  sleep "$seconds"
  to=$(now_ms)
  # The overlay sends its frames once a second, so the last batch is still in flight.
  sleep 1.5
  stop_app
  stop_burners
  # idle-quiet traces no `frame:` lines, so it has no walk frames to count.
  walks=untraced
  [ "$name" = idle-quiet ] || walks=$(awk -v f="$from" -v t="$to" '/^frame: / && $2 >= f && $2 <= t && / (walk|ballwalk)#/ { n++ } END { print n + 0 }' "$log")
  {
    echo "## $tag"
    echo
    echo "window: $from..$to ms, walk frames: $walks"
    echo
    node scripts/frame-cadence.mjs "$log" --from "$from" --to "$to" --json "$out/$tag.json"
  } > "$out/$tag.md"
  cat "$out/$tag.md"
  echo
  if [ "$name" = idle ] && [ "$walks" -gt 0 ]; then
    echo "$name: BMO walked $walks frames in the window, so this is not an idle sample; see $log" >&2
    return 1
  fi
}

if [ "$(uname -s)" = Darwin ]; then
  machine=$(sysctl -n hw.model)
  os="$(sw_vers -productVersion) ($(sw_vers -buildVersion))"
  refresh_hz=$(system_profiler SPDisplaysDataType 2> /dev/null | sed -n 's/.*@ \([0-9.]*\)Hz.*/\1/p' | tr '\n' ' ')
else
  machine=$(uname -m)
  os=$(awk -F= '/^PRETTY_NAME=/ { gsub(/"/, "", $2); print $2; exit }' /etc/os-release)
  refresh_hz=$(xrandr --query 2> /dev/null | awk '/\*/ { for (i = 1; i <= NF; i++) if ($i ~ /\*/) { gsub(/[^0-9.]/, "", $i); print $i; exit } }')
fi
cat << EOF
machine=$machine
os=$os
refresh_hz=$refresh_hz
cpus=$(cpu_count)
EOF
if [ "${#bins[@]}" -eq 1 ]; then
  echo "bin=$bin"
  echo "git_rev=$(git rev-parse --short HEAD 2> /dev/null || echo unknown)"
else
  printf 'bin_a=%s\nbin_b=%s\nrounds=%s\n' "${bins[0]}" "${bins[1]}" "$rounds"
fi
echo "seconds=$seconds"
echo

scenarios=("$scenario")
[ "$scenario" != matrix ] || scenarios=(idle idle-quiet walking load)

if [ "${#bins[@]}" -eq 1 ]; then
  for name in "${scenarios[@]}"; do
    run "$name"
  done
else
  for round in $(seq 1 "$rounds"); do
    order=(a b)
    [ $((round % 2)) -eq 1 ] || order=(b a)
    for name in "${scenarios[@]}"; do
      for side in "${order[@]}"; do
        if [ "$side" = a ]; then bin=${bins[0]}; else bin=${bins[1]}; fi
        # A failed run drops out of the comparison instead of ending the other rounds.
        run "$name" "$name.$side$round" || rm -f "$out/$name.$side$round.json"
      done
    done
  done
  for name in "${scenarios[@]}"; do
    a=()
    b=()
    for round in $(seq 1 "$rounds"); do
      [ ! -f "$out/$name.a$round.json" ] || a+=("$out/$name.a$round.json")
      [ ! -f "$out/$name.b$round.json" ] || b+=("$out/$name.b$round.json")
    done
    if [ "${#a[@]}" -eq 0 ] || [ "${#b[@]}" -eq 0 ]; then
      echo "$name: no completed run on one side; see $out" >&2
      continue
    fi
    {
      echo "## $name, A/B: ${#a[@]} A and ${#b[@]} B runs of $rounds rounds"
      echo
      echo "A=${bins[0]} B=${bins[1]}"
      echo
      node scripts/frame-cadence.mjs compare --a "${a[@]}" --b "${b[@]}"
    } | tee "$out/$name.ab.md"
    echo
  done
fi

echo "out=$out"
