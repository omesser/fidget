# shellcheck shell=bash
# Shared wait core for the macOS and X11 scenario leaves. Each wait starts at
# its trigger, returns as soon as its event lands, and gives up after the
# seconds it is given. A dead Fidget ($pid) fails it.

# Milliseconds since the epoch. BSD date has no %N, so ask perl, which ships
# with macOS.
now_ms() {
  perl -MTime::HiRes=time -e 'printf "%d\n", time * 1000'
}

fidget_alive() {
  [ -z "${pid:-}" ] || kill -0 "$pid" 2> /dev/null
}

# tail -F follows a file through kqueue (BSD) or inotify (GNU). GNU tail
# treats some file systems as remote (overlayfs, NFS) and falls back to a
# poll every second, so ask it for 50 ms there; it stops with Fidget, too.
follow() { # <first line: +1 for the whole file, 0 for new lines> <file>
  if tail --version > /dev/null 2>&1; then
    tail -s 0.05 ${pid:+--pid="$pid"} -n "$1" -F "$2" 2> /dev/null
  else
    tail -n "$1" -F "$2" 2> /dev/null
  fi
}

# Feed a file's lines to a reader until the reader exits or the seconds run
# out. A timer kills the reader, so nothing here re-reads or polls.
wait_on_lines() { # <seconds> <+1|0> <file> <reader...>
  local seconds=$1
  follow_from=$2 follow_file=$3
  shift 3
  wait_on_source "$seconds" follow_wait_file "$@"
}
follow_wait_file() { follow "$follow_from" "$follow_file"; }

# Feed a source command's lines (a log tail or an OS event stream) to a
# reader until the reader exits or the seconds run out.
wait_on_source() { # <seconds> <source command> <reader...>
  local seconds=$1 source=$2 fifo source_pid reader_pid timer_pid result
  shift 2
  fidget_alive || return 1
  fifo=$(mktemp -u "${TMPDIR:-/tmp}/fidget-wait.XXXXXX")
  mkfifo "$fifo"
  # Its shell reports the tail this kills; that stop is expected.
  "$source" > "$fifo" 2> /dev/null &
  source_pid=$!
  "$@" < "$fifo" &
  reader_pid=$!
  { sleep "$seconds" && kill "$reader_pid"; } > /dev/null 2>&1 &
  timer_pid=$!
  wait "$reader_pid" 2> /dev/null && result=0 || result=1
  pkill -P "$timer_pid" 2> /dev/null || true
  pkill -P "$source_pid" 2> /dev/null || true
  kill "$timer_pid" "$source_pid" 2> /dev/null || true
  wait "$timer_pid" "$source_pid" 2> /dev/null || true
  rm -f "$fifo"
  return "$result"
}

# Wait for a line matching an extended regex to land in a file.
wait_for_line() { # <seconds> <file> <pattern>
  wait_on_lines "$1" +1 "$2" grep -qEm1 "$3"
}

# Wait for a trace line in the app log or process.log.
wait_for_trace() { # <seconds> <log file> <pattern>
  wait_for_line "$@"
}

# Wait for a line in the fixture Harness marks file.
wait_for_marks() { # <seconds> <marks file> <pattern>
  wait_for_line "$@"
}

# Fidget is up once it writes its `overlay:` line, which lands before it
# attaches a Harness. A wait for a harness mark starts here, so the mark gets
# the whole budget instead of sharing it with the app's launch.
wait_for_up() { # <seconds> <app log>
  wait_for_line "$1" "$2" '^overlay:'
}

# Wait for a file to exist. tail -F cannot see an empty file appear, so this
# checks again on each new line of the app log ($log), Fidget's own clock.
wait_for_file() { # <seconds> <file path>
  wait_until "$1" test -f "$2"
}

# Check the last frame trace line for a state pattern.
in_state() { # <log> <state pattern>
  grep '^frame: ' "$1" 2> /dev/null | tail -1 | grep -qE " ($2) "
}

# Wait until a command succeeds, for window position or layer, which have no
# OS event. It runs now and on each new line of $log (or $wait_log).
# Accessibility waits use OS events instead: see wait_for_ax_dump.
wait_until() { # <seconds> <command...>
  local seconds=$1
  shift
  "$@" > /dev/null 2>&1 && return 0
  wait_on_lines "$seconds" 0 "${wait_log:-${log:?wait_until needs \$log}}" probe_each_line "$@" || "$@" > /dev/null 2>&1
}

# Read lines and run the command after each, until it succeeds.
probe_each_line() { # <command...>
  while read -r _; do
    "$@" > /dev/null 2>&1 && return 0
  done
  return 1
}

# Wait until a file holds more than <count> lines matching a pattern, for a
# line that repeats (a second minimize, a second Poke). Take the count before
# the trigger.
wait_for_count() { # <seconds> <file> <pattern> <count before the trigger>
  wait_on_lines "$1" +1 "$2" count_past "$3" "$4"
}

# Read lines until more than <count> match the pattern.
count_past() { # <pattern> <count>
  local line seen=0
  while IFS= read -r line; do
    if [[ $line =~ $1 ]]; then
      seen=$((seen + 1))
      [ "$seen" -gt "$2" ] && return 0
    fi
  done
  return 1
}

# Lines matching a pattern in a file so far, for wait_for_count.
line_count() { # <file> <pattern>
  grep -cE "$2" "$1" 2> /dev/null || true
}

# Wait for a process and its children to exit. With flock, wait on <lock>,
# which they hold through an inherited fd even after the pid is gone; without
# it (macOS), caffeinate -w watches the pid through kqueue. True once gone.
wait_for_exit() { # <seconds> <pid> <lock>
  if command -v flock > /dev/null && [ -e "${3:-}" ]; then
    flock -w "$1" "$3" true
    return
  fi
  kill -0 "$2" 2> /dev/null || return 0
  local pid=$2 # wait_on_source fails at once if this pid is already gone
  exit_watched=$2
  wait_on_source "$1" watch_exit read -r
}
# No caffeinate (Linux without the lock) prints nothing, so the wait fails.
watch_exit() {
  caffeinate -w "$exit_watched" 2> /dev/null && echo gone
}

# Fidget and every process under it, deepest first. The Harness runs in its
# own process group, so killing Fidget alone leaves it (and npx, codex-acp or
# the fixture) behind.
process_tree() { # <pid>
  local child
  for child in $(pgrep -P "$1" 2> /dev/null); do process_tree "$child"; done
  echo "$1"
}

# Cleanup for an EXIT/INT/TERM trap: stop Fidget's whole tree.
stop_fidget() {
  [ -n "${pid:-}" ] || return 0
  process_tree "$pid" | xargs kill 2> /dev/null || true
}

# True when the newest five frame lines all show the sprite resting
# (Grounded or Perched, not walking) in one spot. Frames land every draw, so
# this needs no second look after a pause.
sprite_at_rest() { # <log>
  grep '^frame: ' "$1" 2> /dev/null | tail -5 | awk '
    !/ (Grounded|Perched) / || / walk#/ { bad = 1 }
    { match($0, /sprite\(-?[0-9]+,-?[0-9]+\)/); at = substr($0, RSTART, RLENGTH); if (n++ && at != last) bad = 1; last = at }
    END { exit (bad || n < 5) }'
}

# Wait for the sprite to rest. Each frame where it moved (walking, climbing or
# falling) is progress and restarts the 3 s window, so a long walk is fine.
wait_for_rest() { # <log>
  local flag result
  flag=$(mktemp "${TMPDIR:-/tmp}/fidget-rest.XXXXXX")
  while :; do
    sprite_at_rest "$1" && result=0 && break
    : > "$flag"
    wait_on_lines 3 0 "$1" rest_or_moved "$1" "$flag" || { result=1 && break; }
    [ -s "$flag" ] || { result=0 && break; }
  done
  rm -f "$flag"
  return "$result"
}
# Reader for wait_for_rest: exits on rest, or on a move after noting it.
rest_or_moved() { # <log> <flag file>
  local line spot last
  last=$(grep '^frame: ' "$1" 2> /dev/null | tail -1 | grep -oE 'sprite\(-?[0-9]+,-?[0-9]+\)')
  while IFS= read -r line; do
    [[ $line == frame:* ]] || continue
    sprite_at_rest "$1" && return 0
    spot=$(grep -oE 'sprite\(-?[0-9]+,-?[0-9]+\)' <<< "$line")
    if [ -n "$last" ] && [ "$spot" != "$last" ]; then
      echo moved > "$2"
      return 0
    fi
    last=$spot
  done
  return 1
}
