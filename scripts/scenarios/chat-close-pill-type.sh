#!/usr/bin/env bash
# Scenario: chat-close-pill-type (macOS)
# On screen: Fidget as BMO with a fixture Harness that holds its first turn.
#   TextEdit takes the keyboard and the sprite is clicked with no pill open.
#   The pill opens on hover and takes typing with no click: before Chat ever
#   opens, after Chat opens and closes, with TextEdit in front. It closes by
#   Enter and Escape; TextEdit quits under it; a click-through moves the caret.
# Input: cursor moves, clicks, Cmd+W, Enter, Escape, and typed words.
# Duration: about 20 s; every step gives up 3 s after its trigger.
# Grants: Accessibility, and Automation of TextEdit and System Events,
#   for the terminal that runs it.
# Asserts: a sprite click leaves the word in TextEdit with no pill draft, and
#   each pill that opens takes the typed word with no click (#1565), each way
#   of closing it hands the keyboard back, and a click-through moves the caret.
#
# Usage: chat-close-pill-type.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: chat-close-pill-type.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: chat-close-pill-type.sh --go <fidget binary> <fidget test binary>}
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.mac.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-chat-close-pill-type-$(date +%Y%m%d-%H%M%S)-XXXX")
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
data="$out/home/Library/Application Support/fidget"
mkdir -p "$data" "$tools"
# The app's stderr goes to process.log in its data folder, one timestamp per line.
log="$data/process.log" marks="$out/harness.log"
: > "$marks"
: > "$log"
seed_tour_shown "$out/home"

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

# Runs a helper tool, stopping it and its children after <seconds>, so a
# stuck osascript or AX call cannot hang the run.
capped() { # <seconds> <command...>
  local secs=$1 cmd dog status=0
  shift
  "$@" &
  cmd=$!
  (
    sleep "$secs"
    process_tree "$cmd" | xargs kill 2> /dev/null
  ) 2> /dev/null &
  dog=$!
  wait "$cmd" || status=$?
  process_tree "$dog" | xargs kill 2> /dev/null || true
  return "$status"
}

# Runs <command> as the trigger, then waits for one more <ERE> line in <file>.
wait_for_next() { # <seconds> <file> <ERE> <command...>
  local secs=$1 file=$2 pattern=$3 before
  shift 3
  before=$(line_count "$file" "$pattern")
  capped 3 "$@" >> "$out/helpers.log" 2>&1 || fail "could not run $*"
  wait_for_count "$secs" "$file" "$pattern" "$before"
}

build_tool() { # <name> <source>
  [ "$tools/$1" -nt "$2" ] || swiftc -O "$2" -o "$tools/$1" || fail "could not build $1"
}
build_tool ax "$root/scripts/ax-settings.swift"
build_tool warp-cursor "$root/scripts/warp-cursor.swift"
build_tool click-cursor "$root/scripts/click-cursor.swift"

# Case 7 quits TextEdit, which would take unsaved work with it.
! pgrep -x TextEdit > /dev/null || fail "TextEdit is running. Quit it first: this run opens and quits TextEdit itself."
close_textedit() {
  if pgrep -x TextEdit > /dev/null; then
    capped 3 osascript -e 'tell application "TextEdit" to close every document saving no' > /dev/null 2>&1 || true
    capped 3 osascript -e 'tell application "TextEdit" to quit saving no' > /dev/null 2>&1 || true
    if pgrep -x TextEdit > /dev/null; then pkill -x TextEdit 2> /dev/null || true; fi
  fi
}
trap close_textedit EXIT
trap 'exit 130' INT TERM

# Preflight, before Fidget starts: helpers post input only with an Accessibility
# grant, and macOS drops synthetic clicks and keys from an ungranted terminal.
if ! answer=$(capped 3 "$tools/ax" menus $$ 2>&1); then
  case "$answer" in
    *"no Accessibility grant"*) fail "Accessibility is not granted. Grant it in System Settings > Privacy & Security > Accessibility for this terminal, then run again." ;;
    *) fail "the Accessibility check did not answer: $answer" ;;
  esac
fi

# -1743 is a refused Automation grant; no answer in 3 s is an unanswered prompt.
# TextEdit opens on these files. Started with no document, a cold TextEdit
# shows its Open panel, which takes the keyboard from any new document.
one=scenario-one.txt two=scenario-two.txt
: > "$out/$one"
printf abc > "$out/$two"
capped 3 open -g -a TextEdit "$out/$one" || fail "could not open $one in TextEdit"
for probe in "TextEdit:count documents" "System Events:count processes"; do
  app=${probe%%:*}
  if ! answer=$(capped 3 osascript -e "tell application \"$app\" to ${probe#*:}" 2>&1); then
    case "$answer" in
      *-1743*) why="Automation access to $app is denied" ;;
      *) why="no answer from $app in 3 s (an Automation prompt may be waiting)" ;;
    esac
    fail "$why. Grant it in System Settings > Privacy & Security > Automation for this terminal, then run again."
  fi
done

# BMO whose Behaviors stand rather than walk. An app switch wakes the Director,
# and greet's patrol walked the Character out from under a hover.
still="$out/characters/bmo/character.manifest"
mkdir -p "$out/characters" && cp -R "$root/characters/bmo" "$out/characters/"
sed -E '/^play = /s/"walk"/"idle"/g' "$root/characters/bmo/character.manifest" > "$still"
! grep -qE '^play = .*"walk"' "$still" || fail "BMO still walks in $still"

# FIDGET_HARNESS splits on whitespace, so no path in it may hold a space.
# `slow` holds the first turn, so a startup wake neither speaks nor opens Chat.
harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=slow count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$out/characters" \
  "$bin" > "$out/app.log" 2>&1 &
pid=$!
trap 'stop_fidget; pkill -f "count=$marks" || true; close_textedit' EXIT

frame='^[0-9]+ frame: '
draft='^[0-9]+ overlay: draft .*text='

wait_for_count 3 "$log" "$frame" 0 || fail "no frame within 3 s of launch; see $log and $out/app.log"

sprite_center() {
  local size at
  size=$(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1)
  at=$(sed -nE 's/^[0-9]+ frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$log" | tail -1)
  if [ -z "$size" ] || [ -z "$at" ]; then fail "no sprite size or position in $log"; fi
  read -r sw sh <<< "$size"
  read -r sx sy <<< "$at"
  cx=$((sx + sw / 2)) cy=$((sy + sh / 2))
}

# Leave the sprite and come back; the pill opens after HOVER_DELAY_MS (1.5 s)
# and takes the caret by itself.
open_pill() { # <case>
  sprite_center
  capped 3 "$tools/warp-cursor" "$((cx + 4 * sw))" "$cy" --event >> "$out/helpers.log" 2>&1 || fail "$1: could not move the cursor away"
  wait_for_next 3 "$log" "$draft" "$tools/warp-cursor" "$cx" "$cy" --event ||
    fail "$1: the pill did not open; see $log"
}

type_into_pill() { # <word> <case>
  wait_for_next 3 "$log" "${draft}\"[^\"]*$1\"" "$tools/ax" type "$pid" "$1" ||
    fail "$2: \"$1\" never reached the pill (#1565); see $log"
  echo "ok: $2: the pill took \"$1\" with no click"
}

front_name() { capped 3 osascript -e 'tell application "System Events" to get name of first process whose frontmost is true'; }
textedit_front() { [ "$(front_name)" = TextEdit ]; }
# A cold TextEdit can answer `activate` before it is in front; typing then goes
# to whatever is, such as the terminal running this.
# TextEdit's key window: typing lands there, not in document 1.
textedit_window() { capped 3 osascript -e 'tell application "System Events" to get name of front window of process "TextEdit"'; }
# The title may hide the extension, so it matches the name without it.
textedit_front_on() { textedit_front && case "$(textedit_window)" in "${1%.txt}"*) ;; *) return 1 ;; esac }
wait_textedit_front() { # <case> <document name>
  wait_until 3 textedit_front_on "$2" ||
    fail "$1: TextEdit with $2 did not come to the front (front: $(front_name), window: $(textedit_window))"
}
# The text of TextEdit's focused document, read through Accessibility (Apple
# events timed out after typing).
text_front() { capped 2 "$tools/ax" value "$(pgrep -x TextEdit | head -1)" 2> /dev/null; }
wait_text() { # <case> <check command...>
  local case=$1
  shift
  wait_until 3 "$@" && return 0
  text_front > /dev/null || fail "$case: could not read TextEdit's front document"
  return 1
}

# 1. No pill open, TextEdit holds the keyboard: a sprite click must not take it.
# Typing and leaving finish well inside the 1.5 s hover that would open a pill.
capped 3 osascript -e 'tell application "TextEdit" to activate' > "$out/textedit.txt" 2>&1 ||
  fail "could not bring TextEdit forward"
wait_textedit_front sprite-click "$one"
# The Character falls in at startup, so a center read mid-fall misses by the click.
landed() { grep ' frame: ' "$log" | tail -1 | grep -qv ' Falling '; }
wait_until 5 landed || fail "the Character never landed; see $log"
sprite_center
capped 3 "$tools/click-cursor" "$cx" "$cy" 1 > "$out/sprite-click.txt" 2>&1 || fail "could not click the sprite"
capped 3 "$tools/ax" type "$pid" four >> "$out/helpers.log" 2>&1 || fail "could not type"
capped 3 "$tools/warp-cursor" "$((cx + 4 * sw))" "$cy" --event >> "$out/helpers.log" 2>&1 || fail "could not move the cursor away"
typed() { [ "$(text_front)" = four ]; }
wait_text sprite-click typed ||
  fail "a sprite click took the keyboard from TextEdit (front: $(front_name), window: $(textedit_window)); see $log"
! grep -qE "${draft}\"[^\"]*four\"" "$log" || fail "a sprite click sent typing to Fidget; see $log"
echo "ok: a sprite click left typing in TextEdit"

# 2. Before Chat ever opens.
open_pill before-chat
type_into_pill one before-chat

# 3. Chat opened by a Summon, then closed.
sprite_center
wait_for_next 3 "$log" '^[0-9]+ chat: chat-[^ ]+ is open$' "$tools/click-cursor" "$cx" "$cy" 2 ||
  fail "Chat did not open; see $log"
wait_for_next 3 "$log" '^[0-9]+ chat: chat-[^ ]+ is closed$' osascript \
  -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true" \
  -e 'tell application "System Events" to keystroke "w" using command down' ||
  fail "Chat did not close; see $log"
open_pill after-chat
echo "ok: after-chat: a single cursor move opened the pill"
type_into_pill two after-chat

# 4. TextEdit in front when the pill opens. A minimized Chat lets the pill back.
sprite_center
wait_for_next 3 "$log" '^[0-9]+ chat: chat-[^ ]+ is open$' "$tools/click-cursor" "$cx" "$cy" 2 ||
  fail "Chat did not reopen; see $log"
wait_for_next 3 "$log" '^[0-9]+ chat: chat-[^ ]+ is minimized$' "$tools/ax" minimize "$pid" BMO true ||
  fail "Chat did not minimize; see $log"
capped 3 osascript -e 'tell application "TextEdit" to activate' > "$out/textedit-front.txt" 2>&1 || fail "could not bring TextEdit forward"
wait_textedit_front other-app "$one"
open_pill other-app
type_into_pill three other-app

textedit_ends() { case "$(text_front)" in *"$1") ;; *) return 1 ;; esac }
typing_back_in_textedit() { # <word> <case>
  capped 3 "$tools/ax" type "$pid" "$1" >> "$out/helpers.log" 2>&1 || fail "$2: could not type"
  wait_text "$2" textedit_ends "$1" || fail "$2: \"$1\" did not reach TextEdit after the pill closed; see $log"
  ! grep -qE "${draft}\"[^\"]*$1\"" "$log" || fail "$2: the pill kept the keyboard; see $log"
  echo "ok: $2: typing went back to TextEdit"
}
closed='^[0-9]+ overlay: draft .* closed$'

# 5. Enter sends the pill's line and closes it; the keyboard goes back to TextEdit.
wait_for_next 3 "$log" "$closed" "$tools/ax" key "$pid" return || fail "enter: the pill did not close; see $log"
typing_back_in_textedit six enter

# 6. Escape closes the pill the same way.
open_pill escape
type_into_pill seven escape
wait_for_next 3 "$log" "$closed" "$tools/ax" key "$pid" escape || fail "escape: the pill did not close; see $log"
typing_back_in_textedit eight escape

# 7. The remembered app quits while the pill is open: closing the pill steps
# Fidget aside for the next app instead of keeping the keyboard.
open_pill app-quit
# A quit that gets no answer in 3 s ends TextEdit by signal: either way it quits.
capped 3 osascript -e 'tell application "TextEdit" to quit saving no' > "$out/quit.txt" 2>&1 ||
  pkill -x TextEdit || fail "could not quit TextEdit"
wait_for_next 3 "$log" "$closed" "$tools/ax" key "$pid" escape || fail "app-quit: the pill did not close; see $log"
front_pid() { capped 3 osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true'; }
not_front() { [ "$(front_pid)" != "$pid" ]; }
wait_until 3 not_front || fail "app-quit: Fidget kept the keyboard after its app quit; see $log"
echo "ok: app-quit: Fidget stepped aside"

# 8. A click through a transparent part of the overlay moves TextEdit's caret
# to the click: the word lands before the document's text, which only that
# click can cause (a fresh document's caret is at the end).
capped 3 open -a TextEdit "$out/$two" > "$out/textedit2.txt" 2>&1 || fail "could not reopen TextEdit"
wait_textedit_front click-away "$two"
open_pill click-away
type_into_pill nine click-away
bounds=$(capped 3 osascript -e 'tell application "System Events" to get position of front window of process "TextEdit"') ||
  fail "no TextEdit window position"
IFS=', ' read -r left top <<< "$bounds"
# Click the first line's start, past the title bar and the text inset. That
# blurs the pill, which reports its draft again: still "nine", unfocused.
# macOS capitalizes the word as the field loses focus ("Nine").
mark=$(wc -l < "$log")
wait_for_next 3 "$log" "${draft}\"[Nn]ine\" focused=false" "$tools/click-cursor" "$((left + 8))" "$((top + 40))" 1 ||
  fail "click-away: the pill did not keep \"nine\" after the click; see $log"
capped 3 "$tools/ax" type "$pid" ten >> "$out/helpers.log" 2>&1 || fail "click-away: could not type"
caret_moved() { [ "$(text_front)" = tenabc ]; }
wait_text click-away caret_moved || fail "click-away: the click did not put TextEdit's caret at the click; see $log"
! grep -qE "${draft}\"[^\"]*ten\"" "$log" || fail "click-away: the pill took typing back; see $log"
! tail -n +"$((mark + 1))" "$log" | grep -qE "$closed" || fail "click-away: the pill closed after the click; see $log"
echo "ok: click-away: the caret moved to the click, typing followed, and the pill kept \"nine\""

echo "PASS: evidence in $out"
