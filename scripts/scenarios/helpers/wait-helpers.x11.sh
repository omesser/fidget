# shellcheck shell=bash
# X11 wait helpers: the shared core plus an inotify file wait and AT-SPI waits.
# shellcheck source=wait-helpers.sh
. "$(dirname "${BASH_SOURCE[0]}")/wait-helpers.sh"

# Wait for a file to exist: inotifywait blocks on the directory's create
# event; without it, the core's wait on the app log.
wait_for_file() { # <seconds> <file path>
  fidget_alive || return 1
  [ -f "$2" ] && return 0
  command -v inotifywait > /dev/null 2>&1 || {
    wait_until "$1" test -f "$2"
    return
  }
  if timeout "$1" inotifywait -q -m -e create,moved_to --format %f "$(dirname "$2")" 2> /dev/null |
    grep -qxFm1 "$(basename "$2")" || [ -f "$2" ]; then
    return 0
  fi
  dump_fidget_threads
  return 1
}

# Wait for an AT-SPI dump to match a pattern. ax-wait-linux.py checks again
# on each AT-SPI change event from Fidget, so nothing polls. With <file>, the
# dump that matched is saved there.
wait_for_ax_dump() { # <seconds> <pid> <window title> <pattern> [<file>]
  fidget_alive || return 1
  # shellcheck disable=SC2154
  if python3 "${root}/scripts/ax-wait-linux.py" "$2" "$3" "$1" "$4" ${5:+"$5"} 2> /dev/null; then
    return 0
  fi
  dump_fidget_threads
  return 1
}

# Answer a permission ask by pressing the named button (Allow or Reject) in
# Chat once it shows. Gives up after 3 s.
answer_permission() { # <pid> <button name>
  wait_for_ax_dump 3 "$1" BMO "button\|$2" || return 1
  python3 "${root}/scripts/ax-window-linux.py" press "$1" BMO "$2" 2> /dev/null
}
