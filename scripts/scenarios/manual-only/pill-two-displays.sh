#!/usr/bin/env bash
# Scenario: pill-two-displays (macOS)
# On screen: Fidget with two BMO Instances, one dragged to the second display;
#   TextEdit opens on a file and takes the keyboard. Each pill opens by a hover
#   (no click, no typing into it) and both close before anything is typed.
#   Fidget quits when the scenario ends.
# Input: cursor warps, a slow sprite drag to the other display, one Escape
#   keypress, and one typed word at the end.
# Duration: about 60 s; 120 s at most.
# Grants: Accessibility, and Automation of TextEdit and System Events, for the
#   terminal that runs it.
# Asserts: two overlays hold one open pill each, the first pill close hands the
#   keyboard back to no one while the second display's pill is still open, the
#   last close restores the remembered app rather than stepping aside,
#   and the typed word lands in TextEdit.
#
# Usage: pill-two-displays.sh --go <fidget binary> <fidget test binary>
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: pill-two-displays.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: pill-two-displays.sh --go <fidget binary> <fidget test binary>}
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-pill-two-displays-$(date +%Y%m%d-%H%M%S)-XXXX")
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
data="$out/home/Library/Application Support/fidget"
mkdir -p "$data" "$tools"
log="$data/process.log" marks="$out/harness.log"
: > "$log"
: > "$marks"
seed_tour_shown "$out/home"

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

# Runs a helper tool, stopping it and its children after <seconds>, so a stuck
# osascript or AX call cannot hang the run.
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
# The ending flows quit TextEdit, which would take unsaved work with it, so
# refuse before doing any work and end only the TextEdit this run starts, by pid.
textedit_pid=
textedit_opened=()
# open has already returned. One lookup; a miss fails here instead of polling.
# pgrep exits 1 when nothing matches, which under pipefail would hide that.
capture_textedit_pid() {
  textedit_pid=$(pgrep -x TextEdit | head -1) || true
  [ -n "$textedit_pid" ] || fail "TextEdit did not start"
}
refuse_running_textedit() {
  ! pgrep -x TextEdit > /dev/null ||
    fail "TextEdit is running. Quit it first: this run opens and quits TextEdit itself."
}
refuse_running_textedit
close_textedit() {
  [ -n "${textedit_pid:-}" ] || return 0
  close_opened_documents "$textedit_pid" || true
}
trap close_textedit EXIT
trap 'exit 130' INT TERM

build_tool ax "$root/scripts/ax-settings.swift"
build_tool warp-cursor "$root/scripts/warp-cursor.swift"
build_tool click-cursor "$root/scripts/click-cursor.swift"
build_tool throw-sprite "$root/scripts/scenarios/helpers/throw-sprite.swift"

# Preflight, before Fidget starts: helpers post input only with an Accessibility
# grant, and macOS drops synthetic clicks and keys from an ungranted terminal.
if ! answer=$(capped 3 "$tools/ax" menus $$ 2>&1); then
  case "$answer" in
    *"no Accessibility grant"*) fail "Accessibility is not granted. Grant it in System Settings > Privacy & Security > Accessibility for this terminal, then run again." ;;
    *) fail "the Accessibility check did not answer: $answer" ;;
  esac
fi

# One Apple event to each app the run drives; a refusal is a denied grant.
one=scenario-one.txt
: > "$out/$one"
# The builds and the preflight are long enough for the user to open TextEdit.
refuse_running_textedit
capped 3 open -g -a TextEdit "$out/$one" || fail "could not open $one in TextEdit"
textedit_opened+=("$out/$one")
capture_textedit_pid
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

# BMO whose Behaviors stand rather than walk, so no patrol moves a sprite the
# drag or a hover just aimed at. Its speak-phrases are off too: a bubble grabs
# the window key mid-run, which is what the pill waits are racing.
still="$out/characters/bmo/character.manifest"
mkdir -p "$out/characters" && cp -R "$root/characters/bmo" "$out/characters/"
sed -E \
  -e '/^play = /s/"walk"/"idle"/g' \
  -e '/^(near_reaction|rush_reaction) = /s/ = .*/ = "react"/' \
  -e '/^\[behaviors\.(report|greet)\]$/,/^$/s/^weight = .*/weight = 0/' \
  -e '/^\[behaviors\.(report|greet)\]$/,/^$/s/^when = .*/when = "idle over 9m"/' \
  "$root/characters/bmo/character.manifest" > "$still"
! grep -qE '^play = .*"walk"' "$still" || fail "BMO still walks in $still"
! grep -qE '^(near_reaction) = "speak"' "$still" || fail "BMO still speaks to the cursor in $still"
! sed -n '/^\[behaviors\.greet\]$/,/^$/p' "$still" | grep -qE '^weight = [1-9]' ||
  fail "BMO can still greet in $still"

# Two Instances share one process; the pill focus work runs inside it.
# `slow` holds the first turn, so a startup wake neither speaks nor opens Chat.
# count= gives this run's Harness a unique command line for the kill on exit.
harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=slow count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_INSTANCES="bmo:One,bmo:Two" \
  FIDGET_DIRECTOR_WAKE_SECS=600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_HOVER_DELAY_MS=300 \
  FIDGET_CHARACTERS="$out/characters" \
  "$bin" > "$out/app.log" 2>&1 &
pid=$!
trap 'stop_fidget; pkill -f "count=$marks" || true; close_textedit' EXIT

# Readiness: the covers lines name the display frames the run needs. Every
# process.log line carries its own timestamp prefix, so patterns keep `^[0-9]+ `.
wait_for_line 6 "$log" '^[0-9]+ overlay: ' || fail "no overlay start within 6 s of launch; see $log and $out/app.log"
two_overlays() { grep -qE '^[0-9]+ overlay: 2 display\(s\)' "$log"; }
wait_until 5 two_overlays || fail "a dual-display Mac is required; 2 display(s) never logged"

# Display frames from the covers lines, top-left-origin logical points.
covers_line() { # <index> -> w h x y
  grep -E "^[0-9]+ overlay: overlay-$1 covers " "$log" | tail -1 |
    sed -nE 's/^[0-9]+ overlay: overlay-[0-9]+ covers ([0-9]+)x([0-9]+) at \((-?[0-9]+),(-?[0-9]+)\)$/\1 \2 \3 \4/p'
}
read -r dw0 dh0 dx0 dy0 < <(covers_line 0) || fail "no covers line for display 0; see $log"
read -r dw1 dh1 dx1 dy1 < <(covers_line 1) || fail "no covers line for display 1; see $log"

# The sprite size, for hover and press points.
read -r sw sh < <(sed -nE 's/^[0-9]+ overlay: [0-9]+ display\(s\); sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1)
[ -n "$sw" ] || fail "no sprite size in $log"

# Parking points: the top strip of a display, where the pointer sits clear of
# both sprites (which rest near the floor).
park_display1() { capped 3 "$tools/warp-cursor" "$((dx1 + 200))" "$((dy1 + 80))" --event >> "$out/helpers.log" 2>&1; }
park_display0() { capped 3 "$tools/warp-cursor" "$((dx0 + dw0 - 200))" "$((dy0 + 80))" --event >> "$out/helpers.log" 2>&1; }

sprite_at() { # <id> -> global x y, top-left of the sprite
  grep -E "^[0-9]+ frame: .* $1\$" "$log" | tail -1 |
    sed -nE 's/.*sprite\((-?[0-9]+),(-?[0-9]+)\).*/\1 \2/p'
}

in_frame() { # <x> <y> <fx> <fy> <fw> <fh>
  awk -v x="$1" -v y="$2" -v fx="$3" -v fy="$4" -v fw="$5" -v fh="$6" \
    'BEGIN { exit !(x >= fx && x <= fx + fw && y >= fy && y <= fy + fh) }'
}

at_rest_for() { # <id>: the id's last five frames rest in one spot
  grep -E "^[0-9]+ frame: .* $1\$" "$log" | tail -5 | awk '
    !/ (Grounded|Perched) / || / (walk|fall|climb)#/ { bad = 1 }
    { match($0, /sprite\(-?[0-9]+,-?[0-9]+\)/); at = substr($0, RSTART, RLENGTH); if (n++ && at != last) bad = 1; last = at }
    END { exit (bad || n < 5) }'
}

wait_id_rest() { # <seconds> <id> <case>
  wait_until "$1" at_rest_for "$2" || fail "$3: never rested; see $log"
}

# Frames start later than the overlay lines when a fresh HOME compiles, so
# gate the rest waits on the first frame line.
wait_for_line 30 "$log" '^[0-9]+ frame: ' || fail "no frame line within 30 s of launch; see $log"
wait_id_rest 10 "$(grep -E '^[0-9]+ frame: ' "$log" | awk '{print $NF}' | head -1)" first-sprite ||
  fail "the first sprite never rested; see $log"
mapfile -t ids < <(grep -E '^[0-9]+ frame: ' "$log" | awk '{print $NF}' | sort -u)
[ "${#ids[@]}" -eq 2 ] || fail "two Instances are not drawing; ids: ${ids[*]:-none}"
ida=${ids[0]} idb=${ids[1]}
read -r ax_y ay < <(sprite_at "$ida") || fail "no frame for $ida"
read -r bx by < <(sprite_at "$idb") || fail "no frame for $idb"

# Drag one Instance to display 1 only when both started on display 0. A slow
# drag releases with no throw velocity, so the sprite stands where the pointer
# does: 60 steps at 40 ms is about 2.4 s of travel.
if in_frame "$ax_y" "$ay" "$dx1" "$dy1" "$dw1" "$dh1"; then
  echo "ok: $ida already stands on display 1; no drag"
elif in_frame "$bx" "$by" "$dx1" "$dy1" "$dw1" "$dh1"; then
  echo "ok: $idb already stands on display 1; no drag"
else
  if in_frame "$ax_y" "$ay" "$dx0" "$dy0" "$dw0" "$dh0"; then
    drag_id=$ida
  else
    drag_id=$idb
  fi
  read -r sx sy < <(sprite_at "$drag_id")
  # Press the art's middle, like the sibling scenario does: a corner press does
  # not grab.
  px=$((sx + sw / 2))
  py=$((sy + sh / 2))
  dx=$(((dx1 + dw1 / 4) - px))
  capped 30 "$tools/throw-sprite" "$px" "$py" "$dx" 60 0.04 >> "$out/input.log" 2>&1 ||
    fail "could not drag the sprite to display 1; see $out/input.log"
  wait_id_rest 14 "$drag_id" "post-drag $drag_id" || fail "$drag_id never rested after the drag"
  read -r nx ny < <(sprite_at "$drag_id")
  in_frame "$nx" "$ny" "$dx1" "$dy1" "$dw1" "$dh1" ||
    fail "$drag_id did not end on display 1 (dropped at $nx,$ny); see $log"
  echo "ok: dragged $drag_id onto display 1"
fi

# The sprite on display 0 owns the first pill; the display-1 sprite the second.
pill_a_id='' pill_b_id=''
for id in "$ida" "$idb"; do
  read -r x y < <(sprite_at "$id")
  if in_frame "$x" "$y" "$dx1" "$dy1" "$dw1" "$dh1"; then
    pill_b_id=$id
  else
    pill_a_id=$id
  fi
done
if [ -z "$pill_a_id" ] || [ -z "$pill_b_id" ]; then
  fail "one Instance per display did not settle: display1 sprite missing"
fi
echo "ok: $pill_a_id stands on display 0 and $pill_b_id on display 1"

# The drag left the pointer on the dragged sprite, and a hover would have opened
# its pill while TextEdit was being focused. Park clear of both before that.
park_display0 || fail "could not park the pointer after the drag"

# TextEdit in front, per the issue's steps: the user focuses TextEdit first.
capped 3 osascript -e 'tell application "TextEdit" to activate' > "$out/textedit.txt" 2>&1 ||
  fail "could not bring TextEdit forward"
front_name() { capped 3 osascript -e 'tell application "System Events" to get name of first process whose frontmost is true'; }
textedit_front() { [ "$(front_name)" = TextEdit ]; }
wait_until 3 textedit_front || fail "TextEdit did not come to the front (front: $(front_name))"

# The pill opens on a hover alone. The park (defined beside the other parking
# points) holds the pointer clear of both sprites so the dwell arms fresh on the
# enter; then the warp onto the sprite dwells through the 300 ms hover delay.
open_pill_on() { # <id> <park> <case>
  local at
  "$2"
  at=$(sprite_at "$1")
  read -r sx sy <<< "$at"
  wait_for_next 5 "$log" "^[0-9]+ overlay: draft instance=$1 .* focused=true$" \
    "$tools/warp-cursor" "$((sx + sw / 2))" "$((sy + sh / 2))" --event ||
    fail "$3: the pill did not open; see $log"
}

open_pill_on "$pill_a_id" park_display1 a-first
open_pill_on "$pill_b_id" park_display0 b-second
echo "ok: both pills are open, one per display"

# The second pill takes the keyboard, the first page blurs, and its empty pill
# closes without a hand-back: another display's pill is still open. A close
# prints its hand-back before its close report, so the first close's hand-back
# would land above its closed line: the window opens at the second pill's open
# instead, where both pills are up, and allows exactly one hand-back in it —
# the last close's Restore — so an early Stay, StepAside or remembered=0 fails.
closed_b="^[0-9]+ overlay: draft instance=$pill_b_id closed\$"
closed_a="^[0-9]+ overlay: draft instance=$pill_a_id closed\$"
wait_for_line 5 "$log" "$closed_a" || fail "the first pill did not close on the blur; see $log"
opened_b=$(grep -nE "^[0-9]+ overlay: draft instance=$pill_b_id .* focused=true$" "$log" |
  tail -1 | cut -d: -f1)
wait_for_next 5 "$log" "$closed_b" "$tools/ax" key "$pid" escape ||
  fail "the second pill did not close on Escape; see $log"
end_b=$(grep -nE "$closed_b" "$log" | tail -1 | cut -d: -f1)
restores=$(sed -n "$opened_b,$end_b p" "$log" |
  grep -cE '^[0-9]+ app: hand back Restore holding=true remembered=[1-9][0-9]* window=0 alive=true$' || true)
handed=$(sed -n "$opened_b,$end_b p" "$log" | grep -cE '^[0-9]+ app: hand back' || true)
[ "$handed" -eq 1 ] ||
  fail "the close phase logged $handed hand-back lines; expected only the last close's Restore"
[ "$restores" -eq 1 ] ||
  fail "the one hand-back in the close phase was not the last close's Restore; got: $(sed -n "$opened_b,$end_b p" "$log" | grep -E '^[0-9]+ app: hand back')"
echo "ok: the first close handed the keyboard back to no one"

# Both pills are closed: the hand-back must restore the app that was in front,
# not step aside for whatever happens to be there now.
last_restore() { grep -qE '^[0-9]+ app: hand back Restore holding=true remembered=[1-9][0-9]* window=0 alive=true$' "$log"; }
wait_until 5 last_restore ||
  fail "the last close did not restore the remembered app; got: $(grep -E '^[0-9]+ app: hand back' "$log" | tail -3)"
echo "ok: the last close restored the remembered app"

# The typed word ends up in TextEdit; read the way the sibling scenario reads.
text_front() { capped 2 "$tools/ax" value "$(pgrep -x TextEdit | head -1)" 2> /dev/null; }
textedit_ends() { case "$(text_front)" in *"$1") ;; *) return 1 ;; esac }
capped 3 "$tools/ax" type "$pid" wander >> "$out/helpers.log" 2>&1 || fail "could not type"
wait_until 3 textedit_ends wander ||
  fail "\"wander\" did not reach TextEdit after both pills closed (front: $(front_name)); see $log"
! grep -qE 'draft instance=[^ ]+ text="[^"]*wander"' "$log" ||
  fail "a pill took \"wander\" after both had closed; see $log"
echo "ok: typing went back to TextEdit"

echo "PASS: evidence in $out"
