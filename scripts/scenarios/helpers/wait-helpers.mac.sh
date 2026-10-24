# shellcheck shell=bash
# macOS wait helpers: the shared core plus Accessibility waits and tour seeding.
# Source after setting root and tools (holding the built `ax`), then set pid.
# shellcheck source=wait-helpers.sh
. "$(dirname "${BASH_SOURCE[0]}")/wait-helpers.sh"

# True when an Accessibility dump of a window matches a pattern. Given a file,
# each dump lands there, so a wait that matched leaves the matching dump.
ax_dump_matches() { # <pid> <window title> <pattern> [<file>]
  if [ -n "${4:-}" ]; then
    # shellcheck disable=SC2154
    "$tools/ax" dump "$1" "$2" > "$4" 2>&1
    grep -qE "$3" "$4"
  else
    "$tools/ax" dump "$1" "$2" 2> /dev/null | grep -qE "$3"
  fi
}

# Run an Accessibility check now and again on each AXObserver notification
# from the app (printed by ax-events) until it succeeds or the seconds run out.
wait_for_ax() { # <seconds> <pid> <command...>
  local seconds=$1
  # Built by the leaf at startup, beside ax: see build_ax_events.
  # shellcheck disable=SC2154
  [ -x "$tools/ax-events" ] || return 1
  ax_events_pid=$2
  shift 2
  "$@" > /dev/null 2>&1 && return 0
  wait_on_source "$seconds" ax_events probe_each_line "$@"
}
# Build ax-events at startup, where the leaf builds its other tools.
build_ax_events() {
  # shellcheck disable=SC2154
  [ "$tools/ax-events" -nt "$root/scripts/ax-events.swift" ] ||
    swiftc -O "$root/scripts/ax-events.swift" -o "$tools/ax-events"
}
ax_events() { exec "$tools/ax-events" "$ax_events_pid" 2> /dev/null; }

# Wait for an Accessibility dump of a window to match a pattern.
wait_for_ax_dump() { # <seconds> <pid> <window title> <pattern> [<file>]
  wait_for_ax "$1" "$2" ax_dump_matches "$2" "$3" "$4" ${5:+"$5"}
}

# Answer a permission ask by pressing its button (Allow or Reject) in Chat,
# which takes the Character's name. Gives up after 3 s.
answer_permission() { # <pid> <button name> [Chat window title]
  local wpid=$1 button=$2 title=${3:-BMO}
  wait_for_ax_dump 3 "$wpid" "$title" "^AXButton\|$button\|" || return 1
  "$tools/ax" press-button "$wpid" "$button" 1 "$title" > /dev/null 2>&1
}

# Seed settings so the first-run tour counts as shown and never covers the
# windows a scenario reads. <home> is the private HOME the scenario launches
# Fidget with.
seed_tour_shown() { # <home>
  mkdir -p "$1/Library/Application Support/fidget"
  echo '{"first_run_tour_shown": true}' > "$1/Library/Application Support/fidget/settings.json"
}

# The leaf bounds every Apple event at 3s. A direct call would hold the trap.
textedit_osascript() {
  if declare -F capped > /dev/null; then
    capped 3 osascript "$@"
  else
    osascript "$@"
  fi
}

# Quit and signal pid only when TextEdit reports zero documents after these
# paths close. A failed count leaves TextEdit running: another document may
# still be open. Returns 0 clear, 1 blocked, 2 unknown.
close_run_documents() { # <pid> <path>...
  [ -n "${1:-}" ] || return 0
  local pid=$1 path escaped count status=0
  shift
  for path in "$@"; do
    escaped=$path
    escaped=${escaped//\\/\\\\}
    escaped=${escaped//\"/\\\"}
    textedit_osascript -e "tell application \"TextEdit\" to close (every document whose path is \"$escaped\") saving no" > /dev/null 2>&1 || true
  done
  # local count=$(...) would look successful when the count failed.
  count=$(textedit_osascript -e 'tell application "TextEdit" to count every document') || status=$?
  if [ "$status" -ne 0 ]; then
    echo "TextEdit's documents could not be listed, so this run left it running." >&2
    return 2
  fi
  if [ "$count" != 0 ]; then
    echo "TextEdit has another document open, so this run left it running." >&2
    return 1
  fi
  textedit_osascript -e 'tell application "TextEdit" to quit saving no' > /dev/null 2>&1 || true
  kill "$pid" 2> /dev/null || true
}

# bash 3.2 with set -u rejects "${array[@]}" when the array is empty.
close_opened_documents() { # <pid>
  # shellcheck disable=SC2154 # the leaf owns textedit_opened
  if [ "${#textedit_opened[@]}" -gt 0 ]; then
    close_run_documents "$1" "${textedit_opened[@]}"
  else
    close_run_documents "$1"
  fi
}
