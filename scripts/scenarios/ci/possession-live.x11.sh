#!/usr/bin/env bash
# Scenario: possession-live (X11)
# On screen: launches Fidget as BMO with a fixture Harness, and mousepad on a
#   scratch file. The scenario opens the sprite's menu, presses Possess, holds
#   Right and Space, clicks mousepad, clicks the paused card, and presses
#   Escape. Fidget and mousepad quit when the scenario ends. No screenshots.
# Input: one real right-click on the sprite, a click on Possess, Right and
#   Space held, a click in mousepad, the letter k, a click on the paused card,
#   and Escape. Through xdotool. Fidget treats Control-click as the menu
#   gesture only on macOS; here the right button is the menu gesture.
# Duration: about 15 s, 45 s at most.
# Grants: an X11 session, AT-SPI (python3-pyatspi), xdotool, mousepad and
#   inotifywait.
# Asserts: the menu's Possess row possesses the sprite; holding Right walks it
#   at least 8 px; the click away pauses it without launching the held Space;
#   k reaches mousepad; a click on the paused card returns the keyboard; Escape
#   releases possession. Frame waits go through wait_for_trace.
#
# Usage: possession-live.x11.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
# FIDGET_SCENARIO_TRACE and FIDGET_SCENARIO_LETTER check a saved log and the
# editor text, and skip the GUI.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: possession-live.x11.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: possession-live.x11.sh --go <fidget binary> <fidget test binary>}
root=$(cd "$(dirname "$0")/../../.." && pwd)
. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"
. "$root/scripts/scenarios/helpers/possession-live-check.sh"

fail() {
  echo "FAIL: $*" >&2
  if [ -n "${out:-}" ]; then
    echo "evidence in $out" >&2
  fi
  exit 1
}

if [ -n "${FIDGET_SCENARIO_TRACE:-}" ] || [ -n "${FIDGET_SCENARIO_LETTER:-}" ]; then
  if [ -z "${FIDGET_SCENARIO_TRACE:-}" ] || [ -z "${FIDGET_SCENARIO_LETTER:-}" ]; then
    fail "set both FIDGET_SCENARIO_TRACE and FIDGET_SCENARIO_LETTER"
  fi
  check_possession_live "$FIDGET_SCENARIO_TRACE" "$FIDGET_SCENARIO_LETTER" || exit 1
  echo "PASS: fixture trace"
  exit 0
fi

if [ -z "${DISPLAY:-}" ]; then
  echo "SKIP: DISPLAY is unset. X11 possession-live needs a session, or set FIDGET_SCENARIO_TRACE and FIDGET_SCENARIO_LETTER." >&2
  exit 2
fi
python3 -c 'import pyatspi' > /dev/null 2>&1 || {
  echo "SKIP: python3-pyatspi is not installed." >&2
  exit 2
}
for tool in xdotool mousepad inotifywait xprop stdbuf timeout; do
  command -v "$tool" > /dev/null 2>&1 || {
    echo "SKIP: $tool is not installed." >&2
    exit 2
  }
done

test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
out="${TMPDIR:-/tmp}/fidget-scenario-possession-live-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out/home/.local/share/fidget" "$out/editor-home"
log="$out/app.log" marks="$out/harness.log" doc="$out/possession-live-letter.txt"
: > "$marks"
printf 'mark ' > "$doc"
cat > "$out/home/.local/share/fidget/settings.json" << 'SETTINGS_EOF'
{"first_run_tour_shown": true}
SETTINGS_EOF

harness="$root/scripts/scenarios/helpers/fixture-harness.sh $test_bin script=nop count=$marks"
[ "$(wc -w <<< "$harness")" -eq 4 ] || fail "a path in the Harness line holds a space: $harness"

started=()
release_keys() { xdotool keyup Right space 2> /dev/null || true; }
cleanup() {
  release_keys
  stop_fidget
  if [ "${#started[@]}" -gt 0 ]; then
    kill "${started[@]}" 2> /dev/null || true
  fi
  pkill -f "count=$marks" || true
}
trap cleanup EXIT INT TERM

# xprop -spy is an event stream: the editor's map and focus land as root changes.
root_events() { stdbuf -oL xprop -root -spy; }
root_prop_matches() { # <property> <ERE>
  wait_on_source 3 root_events grep -qEm1 "^$1\\(.*$2"
}
HOME="$out/editor-home" mousepad --disable-server "$doc" > "$out/mousepad.log" 2>&1 &
started+=($!)
editor_window() { xdotool search --name "possession-live-letter\\.txt" 2> /dev/null | head -1 | grep .; }
wait_on_source 3 root_events probe_each_line editor_window || fail "mousepad did not map in 3 s"
editor=$(editor_window)
read -r screen_w _ < <(xdotool getdisplaygeometry)
xdotool windowsize "$editor" 420 280 windowmove "$editor" "$((screen_w - 460))" 40 > /dev/null
active_is() { # <window id>
  root_prop_matches _NET_ACTIVE_WINDOW "window id # $(printf '0x%x' "$1")(,|\$)"
}

# The editor's text: Ctrl+S, then the save's rename. A keyup that missed the
# editor would leave "mark " with no k.
editor_text() {
  active_is "$editor" || return 1
  local w line
  exec {w}< <(timeout 3 inotifywait -e moved_to --include '/possession-live-letter\.txt$' "$(dirname "$doc")" 2>&1)
  while read -r -t 3 line <&"$w" && [ "$line" != "Watches established." ]; do :; done
  [ "$line" = "Watches established." ] || {
    exec {w}<&-
    return 1
  }
  xdotool key ctrl+s
  read -r -t 3 line <&"$w" || {
    exec {w}<&-
    return 1
  }
  exec {w}<&-
  cat "$doc"
}

start_ms=$(now_ms)
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 FIDGET_TRACE_BUBBLE=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
wait_for_up "$log" || fail "Fidget never wrote its overlay line; see $log"
wait_for_trace 3 "$log" '^frame: .* sprite\(' || fail "Fidget traced no frame; see $log"
wait_for_rest "$log" || fail "the sprite never came to rest; see $log"
read -r w h < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r x y < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$log" | tail -1) || fail "no frame trace in $log"
cx=$((x + w / 2))
cy=$((y + h / 2))
echo "ok: sprite at ($x,$y) size ${w}x${h}, centre ($cx,$cy)"

python3 "$root/scripts/ax-window-linux.py" menu "$pid" "$cx" "$cy" Possess > "$out/menu.txt" 2> "$out/menu.err" ||
  fail "could not press Possess; see $out/menu.err"
wait_for_trace 3 "$log" '^menu: possession on' || fail "the Possess row did not possess the sprite; see $log"
wait_for_trace 3 "$log" '^overlay [^:]+: window focus' || fail "the possessed overlay never took focus; see $log"
echo "ok: possessed, and the overlay has the keyboard"

xdotool keydown Right keydown space
wait_for_trace 3 "$log" '^frame: .* walk#' || fail "the held Right did not walk the sprite; see $log"

blurs=$(line_count "$log" '^overlay [^:]+: window blur')
eval "$(xdotool getwindowgeometry --shell "$editor")"
# shellcheck disable=SC2153 # X, Y, WIDTH and HEIGHT come from the eval above
xdotool mousemove "$((X + WIDTH / 2))" "$((Y + HEIGHT / 2))" click 1
wait_for_count 3 "$log" '^overlay [^:]+: window blur' "$blurs" || fail "the overlay never lost focus to mousepad; see $log"
wait_until 3 sprite_at_rest "$log" || fail "the sprite kept walking after mousepad took focus; see $log"
check_possession_live "$log" "$doc" pause || fail "the pause check failed; see $log"
release_keys

active_is "$editor" || fail "mousepad did not take focus"
xdotool key k
typed=$(editor_text) || fail "mousepad did not save; see $out/mousepad.log"
printf '%s' "$typed" > "$out/letter.txt"
grep -q '[Kk]' "$out/letter.txt" || fail "the letter did not reach mousepad (it holds $(printf %q "$typed")); see $doc"

# The accessible box and the input shape do not share a pixel, so a click
# there falls through. The shape is the card the overlay is actually taking
# clicks on. The sprite's mask is the other part of that shape.
card_from_shape() { # <sprite x> <sprite y> <sprite w> <sprite h> <window...>
  python3 - "$@" << 'PY'
import ctypes, ctypes.util, sys
x11 = ctypes.CDLL(ctypes.util.find_library("X11"))
xext = ctypes.CDLL(ctypes.util.find_library("Xext"))
class Rect(ctypes.Structure):
    _fields_ = [("x", ctypes.c_short), ("y", ctypes.c_short),
                ("width", ctypes.c_ushort), ("height", ctypes.c_ushort)]
x11.XOpenDisplay.restype = ctypes.c_void_p
xext.XShapeGetRectangles.restype = ctypes.POINTER(Rect)
xext.XShapeGetRectangles.argtypes = [
    ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int,
    ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int)]
dpy = x11.XOpenDisplay(None)
sx, sy, sw, sh = (int(sys.argv[i]) for i in range(1, 5))
best = None
for wid_text in sys.argv[5:]:
    wid = int(wid_text, 0)
    for kind, name in ((0, "bounding"), (2, "input")):
        count, ordering = ctypes.c_int(), ctypes.c_int()
        rects = xext.XShapeGetRectangles(dpy, wid, kind, ctypes.byref(count), ctypes.byref(ordering))
        if not rects:
            print(f"window {wid} {name}: none", file=sys.stderr)
            continue
        for i in range(count.value):
            r = rects[i]
            area = int(r.width) * int(r.height)
            overlaps = not (r.x + r.width <= sx or sx + sw <= r.x or r.y + r.height <= sy or sy + sh <= r.y)
            print(f"window {wid} {name} {r.x},{r.y} {r.width}x{r.height} overlap={overlaps}", file=sys.stderr)
            # The paused card is a clickable hotspot, so it is an input rect
            # that does not cover the sprite. A 1x1 dot is not that rect.
            if kind != 2 or overlaps or area < 400:
                continue
            # The focus button sits in the card's bottom-right corner. The
            # middle of a live card is the key list, which does not take a click.
            if best is None or r.y < best[3]:
                best = (wid, r.x + r.width - 40, r.y + r.height - 20, r.y)
if best is None:
    sys.exit(1)
print(best[0], best[1], best[2])
PY
}
wait_for_ax_dump 3 "$pid" Fidget 'Paused|Click to focus' "$out/card.txt" ||
  fail "the paused card did not appear; see $log"
mapfile -t windows < <(xdotool search --pid "$pid" 2> /dev/null || true)
[ "${#windows[@]}" -gt 0 ] || fail "Fidget has no X window"
read -r sx sy < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$log" | tail -1)
card_from_shape "$sx" "$sy" "$w" "$h" "${windows[@]}" > "$out/card-point.txt" 2> "$out/card-shape.txt" ||
  fail "the input shape has no card; see $out/card-shape.txt"
read -r card_window card_x card_y < "$out/card-point.txt"
echo "ok: paused card at ($card_x,$card_y) on window $card_window"
# The shape already holds the card, so a hit-tested click lands on it.
focuses=$(line_count "$log" '^overlay [^:]+: window focus')
xdotool mousemove --sync "$card_x" "$card_y" click 1
wait_for_count 3 "$log" '^overlay [^:]+: window focus' "$focuses" || fail "the card click did not resume; see $log"
echo "ok: the card click resumed"

xdotool key Escape
wait_for_trace 3 "$log" '^possession: escape' || fail "Escape did not release possession; see $log"
check_possession_live "$log" "$out/letter.txt" || fail "the possession check failed; see $log"

echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
