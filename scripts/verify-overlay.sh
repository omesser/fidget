#!/usr/bin/env bash
# Machine-checkable verification for the overlay window and the frame loop.
# Not a cargo test: every check needs a real desktop, window server and running
# app, so it is slow, macOS-only, and cannot run in CI.

# Usage: scripts/verify-overlay.sh [--keep]
#   --keep   leave the app running afterwards, with tracing on

# Unattended: export FIDGET_DIRECTOR_API_KEY to avoid Keychain prompts.
# Capturable by default (ADR-0024); export FIDGET_CAPTURABLE=0 to test the
# hide-from-captures setting instead.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

KEEP=0
[ "${1:-}" = "--keep" ] && KEEP=1

# Exact-path match, as in crates/verify/src/gesture.rs's stray_pid: this
# checkout's binary path names only the instance this script starts.
BIN_PATH="$(pwd)/target/debug/fidget"
stray_pid() { pgrep -f "$BIN_PATH" 2> /dev/null | head -1; }

# An orphaned always-on-top overlay has no controls; --keep spares only the
# app, killed by its pid. Props go by pattern: the trap is set before any of
# them starts.
trap 'pkill -f perch-window.swift 2> /dev/null;
      [ "$KEEP" = "1" ] || [ -z "${APP_PID:-}" ] || kill "$APP_PID" 2> /dev/null' EXIT INT TERM

STAMP=$(date +%Y%m%d-%H%M%S)
OUT=".verify/$STAMP"
mkdir -p "$OUT"

now_ms() { python3 -c 'import time;print(int(time.time()*1000))'; }

# Waits for a log to say something, rather than sleeping a guessed interval:
# every timing here belongs to the app or the window server, not to us.
await() { # $1=file  $2=grep -E pattern  $3=attempts, a quarter-second each
  for _ in $(seq 1 "$3"); do
    grep -qE "$2" "$1" 2> /dev/null && return 0
    perl -e 'select(undef,undef,undef,0.25)'
  done
  return 1
}

diagnose_no_frames() { # $1=log file
  local log="$1"
  echo "  (the sprite never perched - the checks below will say so)"

  if grep -q '^overlay:' "$log" 2> /dev/null; then
    echo "  Likely cause: app is blocked on a Keychain dialog (Director API key prompt)."
    echo "  For unattended runs, export FIDGET_DIRECTOR_API_KEY before launching."

    if pgrep -q SecurityAgent 2> /dev/null; then
      echo "  (SecurityAgent process is running, confirming a security prompt is active)"
    fi
  fi
}

echo "Building..."
if ! cargo build 2>&1 | tail -3; then
  echo "FAIL: build"
  exit 1
fi

# A Perch to aim the sprite at. It has to exist before the app does: the fall
# takes under a second, and a window that arrives afterwards is above the
# sprite, which is not a surface from below.
swift scripts/inspect-window.swift > "$OUT/desktop.json" 2> "$OUT/desktop.err" || {
  echo "FAIL: inspector"
  cat "$OUT/desktop.err"
  exit 1
}

RECTS=$(
  python3 - "$OUT/desktop.json" << 'PY'
import json, sys

# Where the app puts the sprite, computed the way `snapshot::starting_position`
# does: the middle of the *usable* frame of the first display, so the menu bar
# and Dock shift this start point the same way. CGGetActiveDisplayList lists main first.
u = json.load(open(sys.argv[1]))["displays"][0]["usable"]
sprite_x = u["x"] + u["w"] / 2
sprite_y = u["y"] + u["h"] / 2

# Offsets are fractions of the room between the sprite and the floor, not fixed
# points: a fixed drop calibrated to one display puts a prop's top edge past the
# bottom of a shorter screen, where macOS refuses to place a titled window.

# Assumed: room for the Perch to step down 80 points, roughly 350 usable points.
room = u["h"] / 2

# Wide enough that a Character which wanders while it waits is still on the
# window when it moves: the sprite walks a couple of hundred points before the
# first step, and walking off the end would read as a ride it failed to keep.
width = min(1200.0, u["w"])
left = int(sprite_x - width / 2)

# The Perch.
print(left, int(sprite_y + room / 2), int(width), 240)
# The furniture, between the sprite and the Perch, so the sprite falls through it.
print(left, int(sprite_y + room / 6), int(width), 120)
PY
)
PERCH_RECT=$(echo "$RECTS" | sed -n 1p)
OVER_RECT=$(echo "$RECTS" | sed -n 2p)

# A window the sprite must NOT land on. The Dock and menu bar are not Perches,
# but real furniture has its top edge at the top of the screen, where a falling
# sprite never meets it. A prop at the Dock's own window level does.
DOCK_LEVEL=20
echo "Opening a prop at window level $DOCK_LEVEL at $OVER_RECT to fall through..."
# shellcheck disable=SC2086  # four separate arguments, deliberately
swift scripts/perch-window.swift $OVER_RECT $DOCK_LEVEL > "$OUT/over.log" 2>&1 &
OVER_PID=$!
await "$OUT/over.log" '^\{' 40 || {
  echo "FAIL: elevated prop window never reported its bounds"
  cat "$OUT/over.log"
  exit 1
}

echo "Opening a prop window at $PERCH_RECT to perch on..."
# shellcheck disable=SC2086  # four separate arguments, deliberately
swift scripts/perch-window.swift $PERCH_RECT > "$OUT/perch.log" 2>&1 &
PERCH_PID=$!
await "$OUT/perch.log" '^\{' 40 || {
  echo "FAIL: prop window never reported its bounds"
  cat "$OUT/perch.log"
  exit 1
}

# Nothing this script started should be running yet: a match here is a
# stray, not ours, so it is named and failed rather than killed.
STRAY_PID=$(stray_pid)
if [ -n "$STRAY_PID" ]; then
  echo "FAIL: $BIN_PATH is already running (pid $STRAY_PID); stop it before running verify-overlay.sh"
  exit 1
fi
FIDGET_TRACE_HITTEST=1 FIDGET_TRACE_FRAMES=1 \
  "$BIN_PATH" > "$OUT/app.log" 2>&1 &
APP_PID=$!

for _ in $(seq 1 60); do
  grep -q '^overlay:' "$OUT/app.log" 2> /dev/null && break
  kill -0 "$APP_PID" 2> /dev/null || {
    echo "FAIL: app exited"
    cat "$OUT/app.log"
    exit 1
  }
  perl -e 'select(undef,undef,undef,0.25)'
done

# Land on the prop, then wait for a step it takes *after* landing, so the check
# never races the app's startup.
await "$OUT/app.log" '^frame: [0-9]+ Perched' 60 ||
  diagnose_no_frames "$OUT/app.log"

# It has done its job by now: the sprite has fallen past it, and it is drawn
# above the overlay, so leaving it up would put it over the screenshot below.
kill "$OVER_PID" 2> /dev/null

swift scripts/inspect-window.swift > "$OUT/window.json" 2> "$OUT/window.err" ||
  {
    echo "FAIL: inspector"
    cat "$OUT/window.err"
  }

PERCH_LINES=$(grep -c '^{' "$OUT/perch.log")
for _ in $(seq 1 40); do
  [ "$(grep -c '^{' "$OUT/perch.log")" -gt "$PERCH_LINES" ] && break
  perl -e 'select(undef,undef,undef,0.25)'
done
perl -e 'select(undef,undef,undef,1.5)' # long enough to fall the step and settle

echo "Closing the prop window..."
CLOSED_MS=$(now_ms)
kill "$PERCH_PID" 2> /dev/null
perl -e 'select(undef,undef,undef,2.5)' # long enough to notice, fall and settle

echo ""
echo "Frame loop:"
python3 - "$OUT" "$CLOSED_MS" << 'PY'
import json, re, sys

out, closed_ms = sys.argv[1], int(sys.argv[2])

# The first step happens while still perched, so the idle 10Hz poll applies.
# The slack covers one extra tick plus pkill and this script's timestamps.
POLL_MS, SLACK_MS = 100, 150

frames = [
    (int(m[1]), m[2], float(m[3]), float(m[4]))
    for m in (
        re.match(r"frame: (\d+) (\w+) pos\((-?\d+),(-?\d+)\)", line)
        for line in open(f"{out}/app.log")
    )
    if m
]
steps = [json.loads(line) for line in open(f"{out}/perch.log") if line.startswith("{")]
displays = json.load(open(f"{out}/desktop.json"))["displays"]

fails = []

def check(ok, label, detail=""):
    print(f"  {'PASS' if ok else 'FAIL'}  {label}{'  ' + detail if detail else ''}")
    if not ok:
        fails.append(label)

def first(predicate, of=frames):
    return next((f for f in of if predicate(f)), None)

if not frames or not steps:
    print("  FAIL  the app traced no frames" if not frames
          else "  FAIL  the prop window reported nothing")
    sys.exit(1)

descent = [f for f in frames if f[1] == "Falling"][:10]
check(len(descent) >= 5 and descent[-1][3] > descent[0][3] + 10,
      "the sprite falls under gravity",
      f"{descent[0][3]:.0f} -> {descent[-1][3]:.0f} over {len(descent)} frames")
check(all(b[3] > a[3] for a, b in zip(descent, descent[1:])),
      "every frame of a fall is lower than the last")

# The prop's own reported top edge, from the window server rather than from what
# it was asked for: a titled window's frame is taller than its content rect.
landed = first(lambda f: f[1] == "Perched" and abs(f[3] - steps[0]["y"]) <= 1)
check(landed is not None,
      "it comes to rest on a real window's top edge",
      f"window top y={steps[0]['y']:.0f}")

# A buried prop has the same top edge as a visible one, so the position check
# alone passes whether or not the landing could have happened. Asked over every
# report from the first frame, since a burial is exactly what stops the landing.
under_test = [s for s in steps if frames[0][0] <= s["at_ms"] <= closed_ms]
buried = [s for s in under_test if s.get("depth") != 0]
if buried:
    depth = buried[0].get("depth")
    detail = "off the ordinary window list" if depth is None else f"{depth:.0f} deep"
    detail += f", {buried[0]['at_ms'] - frames[0][0]:.0f}ms into the run"
else:
    detail = f"frontmost in all {len(under_test)} reports the app could see"
check(bool(under_test) and not buried,
      "the Perch stays in front of every other window", detail)

step = first(lambda s: landed and s["at_ms"] > landed[0], steps[1:])
if step is None:
    check(False, "the prop window stepped down while the sprite was perched")
else:
    followed = first(lambda f: f[0] >= step["at_ms"] and f[1] == "Perched"
                    and abs(f[3] - step["y"]) <= 1)
    check(followed is not None and followed[0] - step["at_ms"] <= POLL_MS + SLACK_MS,
          "a slowly moved window is ridden within about one poll interval",
          f"{followed[0] - step['at_ms']:.0f}ms" if followed else "never followed")
    fell = first(lambda f: landed and followed and landed[0] < f[0] < followed[0]
                 and f[1] == "Falling")
    check(fell is None,
          "the sprite rides the step rather than falling onto the new edge",
          f"fell at {fell[0]:.0f}ms" if fell else "stayed perched")

dropped = first(lambda f: f[0] >= closed_ms and f[1] == "Falling")
check(dropped is not None and dropped[0] - closed_ms <= POLL_MS + SLACK_MS,
      "a window that closes drops the sprite within about one poll interval",
      f"{dropped[0] - closed_ms:.0f}ms" if dropped else "never dropped")

tail = frames[-10:]
check(all(f[2:] == tail[0][2:] for f in tail) and tail[-1][1] in ("Grounded", "Perched"),
      "and comes to rest again",
      f"{tail[-1][1]} at ({tail[-1][2]:.0f},{tail[-1][3]:.0f})")
check(tail[-1][3] > steps[0]["y"],
      "lower than the window it had been perched on")

# The usable floor, not the display's bottom edge: a screen reserves a strip
# for the Dock and the sprite rests on its near edge, not behind it. An
# inequality against the display bottom would pass either way; this is an equality.
usable = [d["usable"] for d in displays]
floors = [u["y"] + u["h"] for u in usable
          if u["x"] <= tail[-1][2] <= u["x"] + u["w"]]
check(any(abs(tail[-1][3] - floor) <= 1 for floor in floors),
      "comes to rest on the usable floor rather than behind the Dock",
      "floors " + ", ".join(f"{floor:.0f}" for floor in floors))

# The Dock and the menu bar are not Perches. The prop at the Dock's own window
# level stood in the sprite's way on the first fall: it had to pass through.
over = next((json.loads(line) for line in open(f"{out}/over.log")
             if line.startswith("{")), None)
if over is None or over.get("layer", 0) == 0:
    check(False, "the elevated prop reported an elevated window level",
          f"layer={over.get('layer') if over else 'nothing reported'}")
else:
    check(first(lambda f: f[1] == "Perched" and abs(f[3] - over["y"]) <= 1) is None,
          "a window above the application level is not a Perch",
          f"layer {over['layer']:.0f} top y={over['y']:.0f}")
    check(first(lambda f: f[1] == "Falling" and f[3] > over["y"] + 1) is not None,
          "the sprite falls straight through it",
          "it was still falling below that edge")

# The desktop's own furniture: menu bar, Dock, status items, Notification Centre.
furniture = json.load(open(f"{out}/desktop.json"))["elevated"]
stood_on = [w for w in furniture
            for f in frames
            if f[1] in ("Perched", "Grounded") and abs(f[3] - w["y"]) <= 1
            and w["x"] <= f[2] <= w["x"] + w["w"]]
check(not stood_on,
      "the desktop's own furniture is never stood on",
      f"{len(furniture)} elevated windows"
      if not stood_on else f"stood on {stood_on[0]['owner']}")

print(f"\n{'FAILED: ' + ', '.join(fails) if fails else 'Frame loop checks passed.'}")
sys.exit(1 if fails else 0)
PY
STATUS=$?

lsappinfo list 2> /dev/null | grep -A 4 '"fidget"' > "$OUT/lsappinfo.txt"

# The sprite's own box, never a whole display: evidence can end up on a public
# PR, and the rest of the screen is the owner's. Whether the sprite shows in it
# depends on FIDGET_CAPTURABLE.
echo "Capturing a screenshot of the sprite..."
read -r SW SH < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$OUT/app.log" | head -1)
read -r SX SY < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$OUT/app.log" | tail -1)
if [ -n "${SW:-}" ] && [ -n "${SX:-}" ]; then
  screencapture -x -R "$SX,$SY,$SW,$SH" "$OUT/sprite.png" 2> /dev/null ||
    echo "  (capture failed - is Screen Recording granted to your terminal?)"
else
  echo "  (no sprite position in the frame trace, so no screenshot)"
fi

python3 - "$OUT" << 'PY'
import json, os, re, struct, sys

out = sys.argv[1]
data = json.load(open(f"{out}/window.json"))
displays, windows = data["displays"], data["windows"]
fails = []

def check(ok, label, detail=""):
    print(f"  {'PASS' if ok else 'FAIL'}  {label}{'  ' + detail if detail else ''}")
    if not ok:
        fails.append(label)

print("\nDisplays:")
for d in displays:
    print(f"  {d['w']:.0f}x{d['h']:.0f} at ({d['x']:.0f},{d['y']:.0f})")


def bounds(r):
    return (r["x"], r["y"], r["w"], r["h"])


print("\nChecks:")
# One overlay per display: macOS gives each display its own Space and draws a
# window spanning two on only one. Counted over distinct rectangles, because a
# mirrored pair is two CGGetActiveDisplayList entries and one NSScreen overlay.
screens = {bounds(d) for d in displays}
check(len(windows) == len(screens),
      "one overlay window per display",
      f"{len(windows)} windows, {len(screens)} distinct displays")
if not windows:
    sys.exit(1)

# Identical rules on every one of them. A second window is a second place for
# the level and the on-screen flag to disagree, which is the whole risk of
# having more than one.
check(all(w["onscreen"] for w in windows), "every overlay is on screen")
levels = {w["layer"] for w in windows}
check(levels == {3}, "every overlay is at floating level", f"levels={sorted(levels)}")

# The screen-share half of the hide rules, and the only part a machine can
# check. Default is capturable (ADR-0024): NSWindowSharingReadOnly is 1,
# None is 0. FIDGET_CAPTURABLE's off words (model::switch_from) force 0.
sharing = {w["sharing"] for w in windows}
off_words = {"0", "off", "false", "no"}
expected = 0 if os.environ.get("FIDGET_CAPTURABLE", "").strip().lower() in off_words else 1
label = "excluded from" if expected == 0 else "capturable for"
check(sharing == {expected}, f"every overlay is {label} screen capture",
      f"sharing={sorted(sharing)}")

# The origin has to match too: the right area of the wrong display is the same
# disappearance. Matched by set, not pairwise: mirrored displays report the same
# rectangle, and which overlay answers which is not a fact worth asserting.
uncovered = [d for d in displays if bounds(d) not in {bounds(w) for w in windows}]
check(not uncovered,
      "every display has an overlay covering it whole",
      f"{len(uncovered)} uncovered: " + ", ".join(
          f"{d['w']:.0f}x{d['h']:.0f} at ({d['x']:.0f},{d['y']:.0f})" for d in uncovered))

astray = [w for w in windows if bounds(w) not in {bounds(d) for d in displays}]
check(not astray,
      "no overlay covers anything but a whole display",
      ", ".join(f"{w['w']:.0f}x{w['h']:.0f} at ({w['x']:.0f},{w['y']:.0f})" for w in astray))

ls = open(f"{out}/lsappinfo.txt").read()
check('type="UIElement"' in ls, "accessory app: no Dock tile or switcher entry")

log = open(f"{out}/app.log").read()
check(re.search(r"^character: ", log, re.M) is not None, "app loaded a Character Package")
check(re.search(r"^overlay: ", log, re.M) is not None, "app reported its geometry on startup")
check("hit-test:" in log, "hit-test trace is running")
check("frame:" in log, "frame trace is running")

shots = [f for f in sorted(os.listdir(out)) if f.endswith(".png")]
for s in shots:
    d = open(f"{out}/{s}", "rb").read()
    pw, ph = struct.unpack(">II", d[16:24])
    print(f"  SHOT  {s}  {pw}x{ph}")
check(len(shots) > 0, "captured at least one screenshot")

print(f"\n{'FAILED: ' + ', '.join(fails) if fails else 'All machine checks passed.'}")
print(f"Artifacts: {out}")
sys.exit(1 if fails else 0)
PY
OVERLAY_STATUS=$?
[ "$OVERLAY_STATUS" -ne 0 ] && STATUS=1

# Hit-test pipeline, end to end, against the real art. The cursor is moved onto
# the resting sprite and off it again. Two cases against the *same* sprite
# isolate the alpha lookup: its centre is drawn, its top-left corner is not.
echo ""
echo "Hit-test pipeline:"

BEFORE=$(swift scripts/cursor-position.swift)
SPRITE_AT=$(
  python3 - "$OUT" << 'PY'
import re, sys, time
out = sys.argv[1]

def read_tail_frames(log_path, n=10):
    with open(log_path) as f:
        lines = f.readlines()
    return [
        (m[1], m[2])
        for line in lines
        if (m := re.match(r"^frame: .* sprite\((-?\d+),(-?\d+)\)", line))
    ][-n:]

log_path = f"{out}/app.log"
still_found = False
for attempt in range(40):
    frames = read_tail_frames(log_path, n=5)
    if len(frames) >= 3 and all(f == frames[0] for f in frames):
        pos = frames[0]
        still_found = True
        break
    time.sleep(0.25)

if not still_found:
    sys.exit(1)

size = re.search(r"sprite (\d+)x(\d+)", open(log_path).read())
if not size:
    sys.exit(1)
# Already in the shared point space, which is the space the cursor is warped in.
print(int(pos[0]), int(pos[1]), *size.groups())
PY
) || SPRITE_AT=""

probe() { # $1=offset into the art  $2=label  $3=expected HIT|miss
  local x y line landed
  x=$(($(echo "$SPRITE_AT" | cut -d' ' -f1) + $1))
  y=$(($(echo "$SPRITE_AT" | cut -d' ' -f2) + $1))

  landed=$(swift scripts/warp-cursor.swift "$x" "$y")
  if [ "$landed" != "$x $y" ]; then
    echo "  SKIP  $2 - the cursor could not be placed at $x $y (landed at $landed)"
    return
  fi

  for _ in $(seq 1 40); do
    line=$(grep 'hit-test:' "$OUT/app.log" | tail -1)
    if echo "$line" | grep -q "cursor($x,$y)"; then
      break
    fi
    perl -e 'select(undef,undef,undef,0.1)'
  done

  if ! echo "$line" | grep -q "cursor($x,$y)"; then
    echo "  FAIL  $2 (no hit-test line for cursor($x,$y))"
    echo "        last line: $line"
    HIT_FAILED=1
  elif echo "$line" | grep -q "$3 "; then
    echo "  PASS  $2"
  else
    echo "  FAIL  $2 (expected $3)"
    echo "        $line"
    HIT_FAILED=1
  fi
}

HIT_FAILED=0
if [ -z "$SPRITE_AT" ]; then
  echo "  SKIP  the sprite never stood still for the required consecutive frames"
  echo "        (cannot run hit-test checks on a moving position)"
else
  # Half the art's size is its centre, which is drawn; offset 0 is its
  # top-left corner, which is not.
  probe $(($(echo "$SPRITE_AT" | cut -d' ' -f3) / 2)) "cursor over drawn pixels swallows clicks" "HIT"
  probe 0 "cursor over transparent pixels passes clicks through" "miss"
  # shellcheck disable=SC2086  # an x and a y, deliberately split
  swift scripts/warp-cursor.swift $BEFORE > /dev/null
fi

[ "$HIT_FAILED" = "1" ] && STATUS=1

# The other side of the grip: a Perch that outruns the sprite leaves it behind
# to fall. Needs an app run of its own: the sprite is on the floor by now, and
# a window that opens under a resting sprite is above it, not a surface from below.
echo ""
echo "Grip:"
# shellcheck disable=SC2086  # four separate arguments, deliberately
swift scripts/perch-window.swift --fast $PERCH_RECT > "$OUT/fling.log" 2>&1 &
FLING_PID=$!
await "$OUT/fling.log" '^\{' 40 || {
  echo "FAIL: the flinging prop window never reported its bounds"
  cat "$OUT/fling.log"
  exit 1
}

# Our recorded pid, not a stray: the Grip run gets a clean log of its own.
kill "$APP_PID" 2> /dev/null
wait "$APP_PID" 2> /dev/null
FIDGET_TRACE_FRAMES=1 "$BIN_PATH" > "$OUT/grip.log" 2>&1 &
APP_PID=$!
await "$OUT/grip.log" '^frame: [0-9]+ Perched' 60 ||
  echo "  (the sprite never perched - the checks below will say so)"

# The fling is one move rather than a series, so this waits for the one report
# that follows it: the prop's second line.
for _ in $(seq 1 80); do
  [ "$(grep -c '^{' "$OUT/fling.log")" -ge 2 ] && break
  perl -e 'select(undef,undef,undef,0.25)'
done
perl -e 'select(undef,undef,undef,2.5)' # long enough to be left behind, fall and settle
kill "$FLING_PID" 2> /dev/null

python3 - "$OUT" << 'GRIP'
import json, re, sys

out = sys.argv[1]

# The sprite is perched and its Perch is still when the fling comes, so the
# window list is read at the idle poll. Same slack as the riding half.
POLL_MS, SLACK_MS = 100, 150

frames = [
    (int(m[1]), m[2], float(m[3]), float(m[4]))
    for m in (
        re.match(r"frame: (\d+) (\w+) pos\((-?\d+),(-?\d+)\)", line)
        for line in open(f"{out}/grip.log")
    )
    if m
]
moves = [json.loads(line) for line in open(f"{out}/fling.log") if line.startswith("{")]

fails = []

def check(ok, label, detail=""):
    print(f"  {'PASS' if ok else 'FAIL'}  {label}{'  ' + detail if detail else ''}")
    if not ok:
        fails.append(label)

def first(predicate):
    return next((f for f in frames if predicate(f)), None)

if not frames or len(moves) < 2:
    print("  FAIL  the app traced no frames" if not frames
          else "  FAIL  the prop window never flung itself")
    sys.exit(1)

# The frame before the fling rather than any landing: a Character wanders, and
# one that walked off the end before the fling fell for its own reasons, which
# would read as a grip that let go.
fling = moves[1]
standing = next((f for f in reversed(frames) if f[0] < fling["at_ms"]), None)
landed = standing if standing and standing[1] == "Perched" \
    and abs(standing[3] - moves[0]["y"]) <= 1 else None
check(landed is not None,
      "the sprite is standing on the prop when it is flung",
      f"window top y={moves[0]['y']:.0f}" if landed
      else f"{standing[1]} at ({standing[2]:.0f},{standing[3]:.0f})" if standing
      else "no frame before the fling")

left = first(lambda f: landed and f[0] >= fling["at_ms"] and f[1] == "Falling")
check(left is not None and left[0] - fling["at_ms"] <= POLL_MS + SLACK_MS,
      "a window moved faster than the grip is not ridden",
      f"{left[0] - fling['at_ms']:.0f}ms" if left else "never let go")

# Falling on its own would also describe a sprite that rode the fling down and
# lost its footing afterwards. What says it was left behind is that it reached
# the new edge by falling to it rather than being carried there.
carried = first(lambda f: f[0] >= fling["at_ms"] and f[1] == "Perched"
                and abs(f[3] - fling["y"]) <= 1 and (left is None or f[0] < left[0]))
caught = first(lambda f: left and f[0] > left[0] and f[1] == "Perched"
               and abs(f[3] - fling["y"]) <= 1)
check(carried is None and caught is not None,
      "and falls onto the edge that outran it rather than riding to it",
      f"carried there {carried[0] - fling['at_ms']:.0f}ms after the fling" if carried
      else f"landed again at y={fling['y']:.0f}" if caught else "never landed again")

print(f"\n{'FAILED: ' + ', '.join(fails) if fails else 'Grip checks passed.'}")
sys.exit(1 if fails else 0)
GRIP
GRIP_STATUS=$?
[ "$GRIP_STATUS" -ne 0 ] && STATUS=1

echo ""
echo "Still needs a human (docs/DEVELOPMENT.md > Manual Verification Checklist):"
echo "  that the window server honours the flag - a click really lands underneath,"
echo "  and typing elsewhere really survives a click on the sprite."

if [ "$KEEP" = "1" ]; then
  printf '\n'
  echo "App still running (pid $APP_PID) for the manual checks, with tracing on."
  echo "Watch the decisions:  tail -f $OUT/app.log"
  echo "Stop it:              kill $APP_PID"
fi
exit $STATUS
