#!/usr/bin/env bash
# Scenario: see-through-chat (X11)
# On screen: nothing on your display. A private 1280x800 Xvfb display with
#   xfwm4 (compositing off), Fidget as BMO with Chat and Settings open, three
#   launches; picom starts after the first.
# Input: none.
# Duration: about 6 s; every wait gives up 3 s after its trigger.
# Grants: Xvfb, xfwm4, xprop, xdotool, xwininfo (x11-utils), picom and python3.
# Asserts: Chat and Settings are opaque (depth 24) with no compositor (#1732),
#   see-through (depth 32) under picom, and opaque under picom with
#   WEBKIT_DISABLE_DMABUF_RENDERER=1 when the WebKitGTK Fidget loaded, as its
#   log names it, is 2.54 or newer (#1734).
#
# Usage: see-through-chat.x11.sh --go <fidget binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: see-through-chat.x11.sh --go <fidget binary>}
for tool in Xvfb xfwm4 xprop xdotool xwininfo picom python3 stdbuf flock; do
  command -v "$tool" > /dev/null 2>&1 || {
    echo "SKIP: $tool is not installed." >&2
    exit 2
  }
done
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"
out=$(mktemp -d "${TMPDIR:-/tmp}/fidget-scenario-see-through-chat-x11-$(date +%Y%m%d-%H%M%S)-XXXX")
mkdir -p "$out/characters"
results="$out/results.txt"
: > "$results"
started=()

fail() {
  echo "FAIL: $*" | tee -a "$results" >&2
  echo "evidence in $out" >&2
  exit 1
}
ok() { echo "ok: $*" | tee -a "$results"; }
lock="$out/run.lock"
exec 9> "$lock"
flock -n 9 || fail "another run holds $lock"
# TERM to the tree of each process this run started, up to 3 s for all of them
# to exit, then kill -9 on what is left of those trees. Only by PID.
cleanup() {
  local tree=() p
  exec 9>&-
  for p in "${started[@]}"; do mapfile -t -O "${#tree[@]}" tree < <(process_tree "$p"); done
  [ "${#tree[@]}" -gt 0 ] || return 0
  kill "${tree[@]}" 2> /dev/null || true
  wait_for_exit 3 "${tree[0]}" "$lock" || kill -9 "${tree[@]}" 2> /dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

exec {ready}< <(
  Xvfb -displayfd 1 -screen 0 1280x800x24 -nolisten tcp 2> "$out/xvfb.log" &
  echo "pid $!"
  wait "$!"
)
read -r -t 3 _ xvfb <&"$ready" || fail "Xvfb did not start"
started+=("$xvfb")
read -r -t 3 n <&"$ready" || fail "Xvfb gave no display in 3 s; see $out/xvfb.log"
export DISPLAY=":$n"

# xprop -spy prints the root's properties now and on each change: an event stream.
root_events() { stdbuf -oL xprop -root -spy; }
xfwm4 --compositor=off > "$out/xfwm4.log" 2>&1 &
started+=($!)
wait_on_source 3 root_events grep -qEm1 '^_NET_SUPPORTING_WM_CHECK\(.*window id' ||
  fail "xfwm4 did not start in 3 s"
exec 9>&-

cp -R "$root/characters/bmo" "$out/characters/"
sed -E '/^play = /s/"walk"/"idle"/g' "$root/characters/bmo/character.manifest" > "$out/characters/bmo/character.manifest"

# The colour depth of the mapped window titled exactly <title>.
depth_of() { # <title>
  local id
  id=$(xdotool search --onlyvisible --name "^$1\$" 2> /dev/null | head -1 | grep .) || return 1
  xwininfo -id "$id" | sed -nE 's/^ *Depth: ([0-9]+)$/\1/p'
}
both_mapped() { depth_of BMO > /dev/null && depth_of Settings > /dev/null; }
# Launches Fidget with Chat and Settings open and checks both windows' depth.
expect_depth() { # <case> <depth> [VAR=value...]
  local case=$1 want=$2 got
  shift 2
  mkdir -p "$out/$case/.local/share/fidget"
  echo '{"first_run_tour_shown": true}' > "$out/$case/.local/share/fidget/settings.json"
  log="$out/$case/.local/share/fidget/process.log"
  : > "$log"
  env -u WEBKIT_DISABLE_DMABUF_RENDERER -u WEBKIT_DISABLE_COMPOSITING_MODE \
    HOME="$out/$case" FIDGET_DIRECTOR_WAKE_SECS=600 FIDGET_TRACE_FRAMES=1 \
    FIDGET_OPEN_CHAT=1 FIDGET_OPEN_SETTINGS=1 \
    FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$out/characters" "$@" \
    "$bin" > "$out/$case.log" 2>&1 &
  # shellcheck disable=SC2034 # read by stop_fidget and the waits
  pid=$!
  # Searched again on each root change, _NET_CLIENT_LIST among them.
  wait_on_source 3 root_events probe_each_line both_mapped ||
    fail "$case: Chat and Settings did not both map in 3 s; see $out/$case.log"
  for title in BMO Settings; do
    got=$(depth_of "$title")
    [ "$got" = "$want" ] || fail "$case: $title has depth $got, want $want"
  done
  ok "$case: Chat and Settings at depth $want"
}

start_ms=$(now_ms)
expect_depth no-compositor 24
# Fidget's frame trace is the clock the compositor check runs on.
picom --backend xrender > "$out/picom.log" 2>&1 &
started+=($!)
wait_until 3 python3 "$root/scripts/scenarios/helpers/no-compositor-clip.py" cm ||
  fail "picom did not take _NET_WM_CM_S0 in 3 s; see $out/picom.log"
stop_fidget
expect_depth picom 32
stop_fidget
# The WebKitGTK the binary loaded, not the system's: an AppImage bundles its own.
webkit=$(sed -nE 's/^([0-9]+ )?webkit: WebKitGTK ([0-9.]+)$/\2/p' "$log" | head -1)
[ -n "$webkit" ] || fail "Fidget logged no WebKitGTK version; see $log"
ok "Fidget loaded WebKitGTK $webkit"
# WebKitGTK 2.54's non-accelerated renderer repaints only damage (#1734).
if printf '2.54\n%s\n' "$webkit" | sort -CV; then dmabuf=24; else dmabuf=32; fi
expect_depth picom-dmabuf-off "$dmabuf" WEBKIT_DISABLE_DMABUF_RENDERER=1
stop_fidget

echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)" | tee -a "$results"
