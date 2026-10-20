#!/usr/bin/env bash
# Scenario: launcher-dies-at-startup (macOS)
# On screen: launches Fidget as BMO with a fixture Harness whose first launch
#   aborts before initialize. The menu bar icon's menu opens and its Chat… row
#   is pressed, so Chat opens and takes focus. Chat is resized to 420 and 320
#   points, then Codex is pressed. Three screenshots. Fidget quits at the end.
# Input: one real click on the menu bar icon; AXPress on Chat… and Codex, a
#   click only if one refuses it (three at most); AX resizes; no keys.
# Duration: a few seconds, 10 s at most.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: what only a live run can: the tray's Chat… row opens the Harness
#   error landing; at 420 and 320 its Error output and Command boxes end inside
#   the window and Error output starts above the composer; Codex, a re-pick
#   under FIDGET_HARNESS, launches the Harness again at once. Copy, labels and
#   capture are chat-landing-format.test.js's and the Rust tests'.
#
# Usage: launcher-dies-at-startup.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: launcher-dies-at-startup.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: launcher-dies-at-startup.sh --go <fidget binary> <fidget test binary>}
# Absolute: the Harness spawns in the data folder, so a relative path misses.
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-launcher-dies-at-startup-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools"
seed_tour_shown "$out/home"
log="$out/app.log" marks="$out/harness.log"
: > "$marks"

fail() {
  printf 'FAIL: %s\nevidence in %s\n' "$*" "$out" >&2
  exit 1
}

[ -x "$tools/window-id" ] || swiftc -O "$root/scripts/scenarios/window-id.swift" -o "$tools/window-id"
[ "$tools/ax" -nt "$root/scripts/ax-settings.swift" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"
build_ax_events

# `abort-first` prints a dyld line and aborts on its first spawn, then answers.
harness="$out/launcher.sh"
# shellcheck disable=SC2016 # "$1" and "$@" belong to the wrapper.
printf '#!/bin/sh\n[ "$1" = --version ] && { echo fake-acp-agent 1.0.0; exit 0; }\nexec %q %s "$@"\n' \
  "$test_bin" "harness::tests::fake_acp_agent --exact --nocapture --test-threads=1 script=abort-first count=$marks" > "$harness"
chmod +x "$harness"
[ "$(wc -w <<< "$harness")" -eq 1 ] || fail "FIDGET_HARNESS splits on spaces, and $harness holds one"

# No ambient wake: one would respawn the Harness and pass the re-pick for it.
start_ms=$(now_ms)
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'stop_fidget; pkill -f "count=$marks" || true' EXIT INT TERM

spawns() { grep -cx spawn "$marks" || true; }
respawned() { [ "$(spawns)" -ge 2 ]; }
# Fixed strings: the launcher is an absolute path, and a path is not a regex.
dump_has() { # <name> <text>...: true when the dump holds any of them
  local f=$out/$1.ax.txt p
  shift
  "$tools/ax" dump "$pid" BMO > "$f" 2>&1 || return 1
  for p; do grep -qF "$p" "$f" && return 0; done
  return 1
}
shot() { screencapture -x -o -l "$("$tools/window-id" "$pid" BMO)" "$out/$1.png"; }
rect() { tr , ' ' <<< "${1##*|}"; }                          # <dump line>: its frame as "x y w h"
box() { grep -E '^AXStaticText\|' "$2" | grep -m1 -F "$1"; } # <text> <dump>

reflowed() { # <width>
  local aw
  "$tools/ax" dump "$pid" BMO frames > "$out/$1.ax.txt" 2>&1 || return 1
  read -r _ _ aw _ <<< "$(rect "$(grep -m1 '^AXWebArea|' "$out/$1.ax.txt")")"
  [ -n "$aw" ] && [ "$aw" -le "$1" ]
}
name=$harness dyld="dyld[0]: Library not loaded"
fits() { # <width>: both boxes end inside Chat, and Error output starts above the composer
  local w=$1 f=$out/$1.ax.txt wx got x y ww fold text row
  "$tools/ax" size "$pid" BMO "$w" 560 > "$out/$w.frame.txt" 2>&1 || fail "$w: resize failed; see $out/$w.frame.txt"
  read -r wx _ got _ < <(tr , ' ' < "$out/$w.frame.txt")
  [ "$got" -eq "$w" ] || fail "$w: Chat is $got points wide after the resize"
  # The resize lands before WebKit reflows, so wait for the page to fit the new width.
  wait_for_ax 2 "$pid" reflowed "$w" || fail "$w: the page never reflowed to fit; see $f"
  shot "failed-$w"
  for text in "$dyld" "|$harness|"; do
    row=$(box "$text" "$f") || fail "$w: no box holds '$text' in $f"
    read -r x _ ww _ <<< "$(rect "$row")"
    [ $((x + ww)) -le $((wx + got + 1)) ] || fail "$w: a box ends at $((x + ww)), past the window edge at $((wx + got))"
  done
  # The composer covers the log's foot, so the fold is its top, not the window's.
  read -r _ y _ _ <<< "$(rect "$(box "$dyld" "$f")")"
  row=$(grep -m1 '^AXTextArea|||Nothing can answer yet|' "$f") || fail "$w: no composer in $f"
  read -r _ fold _ _ <<< "$(rect "$row")"
  [ "$y" -lt "$fold" ] || fail "$w: Error output starts at y $y, under the composer at $fold"
  echo "ok: $w: both boxes inside the window, Error output at y $y above the composer at $fold"
}

wait_for_trace 3 "$log" 'exited before initialize' || fail "the launcher never died before initialize; see $log"
[ "$(spawns)" -eq 1 ] || fail "want one spawn before Chat opens, the fixture saw $(spawns)"
"$tools/ax" open "$pid" Chat BMO > "$out/open.txt" 2>&1 || fail "the menu's Chat row did not open Chat; see $out/open.txt"
wait_for_ax 3 "$pid" dump_has failed "|Harness couldn't start|" || fail "Chat shows no Harness error landing; see $out/failed.ax.txt"
echo "ok: the tray's Chat row opened the Harness error landing"
for w in 420 320; do fits "$w"; done

[ "$(spawns)" -eq 1 ] || fail "the Harness launched again before the re-pick ($(spawns) spawns)"
"$tools/ax" press-button "$pid" Codex 1 BMO > "$out/press.txt" 2>&1 || fail "could not press Codex on the landing; see $out/press.txt"
wait_until 3 respawned || fail "the re-pick did not launch the Harness again; see $log"
wait_for_ax 3 "$pid" dump_has live "|$name · session " "|$name · no session yet|" ||
  fail "after the re-pick the mind line names no live Harness; see $out/live.ax.txt"
shot live
echo "ok: the re-pick launched it again at once: $(grep -oEm1 '· (session [^|]+|no session yet)' "$out/live.ax.txt")"
echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
