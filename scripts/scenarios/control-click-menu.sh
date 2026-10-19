#!/usr/bin/env bash
# Scenario: control-click-menu (macOS)
# On screen: launches Fidget as BMO with a fixture Harness. The scenario finds
#   the sprite, sends a Control-click to its centre, waits for the menu to
#   appear, and reads its items. One screenshot of the open menu, cropped to
#   the menu's bounds.
#   Fidget quits when the scenario ends.
# Input: one Control-click on the sprite (synthetic, via CGEvent), held while
#   the menu is read; Escape closes the menu before the release. When the app
#   draws no menu at all, the leaf clicks again, three times at most.
# Duration: a few seconds; 20 s at most when a Control-click has to be repeated.
# Grants: Screen Recording and Accessibility for the terminal that runs it.
# Asserts: the Control-click lands as Menu, and the open menu's Accessibility
#   items include Chat…, Settings… and Quit.
#
# Usage: control-click-menu.sh --go <fidget binary> <fidget test binary>
# Without --go it prints this header, which is the takeover prompt, and exits 2.
set -euo pipefail

if [ "${1:-}" != --go ]; then
  sed -n '2,/^[^#]/s/^# \{0,1\}//p' "$0" || true
  exit 2
fi
bin=${2:?usage: control-click-menu.sh --go <fidget binary> <fidget test binary>}
test_bin=${3:?usage: control-click-menu.sh --go <fidget binary> <fidget test binary>}
test_bin=$(cd "$(dirname "$test_bin")" && pwd)/$(basename "$test_bin")
root=$(cd "$(dirname "$0")/../.." && pwd)
. "$root/scripts/scenarios/wait-helpers.mac.sh"
out="${TMPDIR:-/tmp}/fidget-scenario-control-click-menu-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools"
seed_tour_shown "$out/home"
log="$out/app.log"

fail() {
  echo "FAIL: $*" >&2
  echo "evidence in $out" >&2
  exit 1
}

[ -x "$tools/window-id" ] || swiftc -O "$root/scripts/scenarios/window-id.swift" -o "$tools/window-id"
[ "$tools/ax" -nt "$root/scripts/ax-settings.swift" ] || swiftc -O "$root/scripts/ax-settings.swift" -o "$tools/ax"

harness="$root/scripts/scenarios/fixture-harness.sh $test_bin script=nop"
[ "$(wc -w <<< "$harness")" -eq 3 ] || fail "a path in the Harness line holds a space: $harness"

start_ms=$(now_ms)
env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'stop_fidget; pkill -f "script=nop" || true' EXIT INT TERM

# The overlay spans the display, so its centre is not the sprite. The newest
# frame trace line says where the sprite is drawn, in global points.
wait_for_trace 3 "$log" '^frame: .* sprite\(' || fail "Fidget traced no frame; see $log"
# A click on a walking sprite lands where it was, so wait for it to rest.
wait_for_rest "$log" || fail "the sprite never came to rest; see $log"
read -r w h < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"

# The app opens the tray menu and, in about one Control-click in five, draws no
# menu at all: `popup_menu_at` returns with no menu window and the frame traces
# keep coming through the click (#1651). Nothing can read a menu that is not
# drawn, so the leaf clicks again and says so in the dump. Only that failure is
# retried; a menu window that drew and read no items is this leaf's own bug.
attempt=0
while :; do
  attempt=$((attempt + 1))
  menu_dump="$out/menu.ax.txt"
  [ "$attempt" -eq 1 ] || menu_dump="$out/menu.ax.$attempt.txt"
  # A Poke can leave the sprite walking, so a retried click waits for rest
  # before the newest frame line is trusted.
  [ "$attempt" -eq 1 ] || wait_for_rest "$log" || fail "the sprite never came to rest after the retried click; see $log"
  read -r x y < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$log" | tail -1) || fail "no frame trace in $log"
  cx=$((x + w / 2))
  cy=$((y + h / 2))
  echo "ok: sprite at ($x,$y) size ${w}x${h}, centre ($cx,$cy)"

  # Hold the Control-down while the menu is read: a release before the menu
  # tracks lands on the overlay as a Poke. Escape closes it, then the release.
  if swift - "$pid" "$cx" "$cy" "$out/menu.png" > "$menu_dump" 2>&1 << 'SWIFT'
import AppKit
import ApplicationServices

let args = CommandLine.arguments
guard let pid = pid_t(args[1]), let x = Double(args[2]), let y = Double(args[3]) else { exit(2) }
let point = CGPoint(x: x, y: y)
func mouse(_ type: CGEventType) {
    let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left)
    event?.flags = .maskControl
    event?.post(tap: .cghidEventTap)
}
func key(_ code: CGKeyCode) {
    for down in [true, false] {
        CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: down)?.post(tap: .cghidEventTap)
    }
}
// A synthetic mouse event with no Control key pair behind it leaves the
// session's Control flag set, and every later synthetic click inherits it.
// One flagless move through the HID state is what clears it (#1503).
// ponytail: no macOS event marks a session flag change, so the read below
// repeats every 20 ms for at most a second. It costs that second only on a
// machine where the state never settles; drop it for a notification if one
// ever arrives.
func clearFlags() {
    if let source = CGEventSource(stateID: .hidSystemState),
        let at = CGEvent(source: nil)?.location,
        let moved = CGEvent(
            mouseEventSource: source, mouseType: .mouseMoved, mouseCursorPosition: at,
            mouseButton: .left)
    {
        moved.flags = []
        moved.post(tap: .cghidEventTap)
    }
    // The state settles a beat after the event, so give it one before saying.
    let deadline = Date().addingTimeInterval(1)
    func sessionFlags() -> CGEventFlags { CGEventSource.flagsState(.combinedSessionState) }
    while sessionFlags().contains(.maskControl) && Date() < deadline { usleep(20_000) }
    if sessionFlags().contains(.maskControl) {
        FileHandle.standardError.write(
            "input-state: session flags \(sessionFlags().rawValue) still carry Control\n"
                .data(using: .utf8)!)
    }
}
func value(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var out: CFTypeRef?
    return AXUIElementCopyAttributeValue(element, name as CFString, &out) == .success ? out : nil
}

// The popped-up menu is no child of the application element, and the element
// AXMenuOpened hands over answers nothing in one run in three — role, frame and
// children all come back empty while the app is tracking the menu it just drew
// — so the menu is read the way it is drawn: the menu-layer window is the
// frame, and the rows under that window are the items (#1503).
// 100 is above NSStatusWindowLevel (25), where Chat sits, and above the
// overlay's NSFloatingWindowLevel (3); the tray menu's NSPopUpMenuWindowLevel
// (101) is the only window up there.
func menuWindow() -> CGRect? {
    let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: AnyObject]] ?? []
    return info.first {
        ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && (($0[kCGWindowLayer as String] as? Int) ?? 0) >= 100
    }.flatMap { CGRect(dictionaryRepresentation: $0[kCGWindowBounds as String] as! CFDictionary) }
}
// Wait on AXMenuOpened / AXMenuClosed from the app, never a sleep: the run
// loop returns on the notification, or after the seconds given.
var seen: Set<String> = []
var observer: AXObserver?
// Every notification the app posts, counted, so a read can wait for the app to
// finish making the menu without watching the clock.
var tick = 0
AXObserverCreate(pid, { _, _, name, _ in
    seen.insert(name as String)
    tick += 1
}, &observer)
if let observer {
    let app = AXUIElementCreateApplication(pid)
    for name in [
        kAXMenuOpenedNotification, kAXMenuClosedNotification, kAXCreatedNotification,
        kAXWindowCreatedNotification, kAXFocusedUIElementChangedNotification,
        kAXFocusedWindowChangedNotification, kAXLayoutChangedNotification,
        kAXMovedNotification, kAXResizedNotification,
    ] {
        AXObserverAddNotification(observer, app, name as CFString, nil)
    }
    CFRunLoopAddSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(observer), .defaultMode)
}
func waitFor(_ name: String, seconds: Double) {
    let deadline = Date().addingTimeInterval(seconds)
    while !seen.contains(name) && Date() < deadline {
        CFRunLoopRunInMode(.defaultMode, deadline.timeIntervalSinceNow, true)
    }
}
// AXMenuOpened lands while the app is still making the menu: the window is not
// listed yet when the notification arrives, so the window list is read again on
// each notification the app posts after it and the menu is read the moment it
// is there. The 1.5 s is the ceiling for an app that says nothing more, never
// the wait.
func menuRect(seconds: Double) -> (CGRect?, Int) {
    let started = Date()
    let deadline = started.addingTimeInterval(seconds)
    var handled = tick
    while true {
        if let rect = menuWindow() {
            return (rect, Int(Date().timeIntervalSince(started) * 1000))
        }
        if Date() >= deadline { return (nil, Int(seconds * 1000)) }
        while tick == handled && Date() < deadline {
            CFRunLoopRunInMode(.defaultMode, deadline.timeIntervalSinceNow, true)
        }
        handled = tick
    }
}
mouse(.leftMouseDown)
waitFor(kAXMenuOpenedNotification, seconds: 3)
let (rect, waited) = menuRect(seconds: 1.5)
var items: [String] = []
if let rect {
    let system = AXUIElementCreateSystemWide()
    for y in stride(from: rect.minY + 4, to: rect.maxY, by: 6) {
        var hit: AXUIElement?
        guard AXUIElementCopyElementAtPosition(system, Float(rect.midX), Float(y), &hit) == .success,
            let hit, value(hit, kAXRoleAttribute) as? String == "AXMenuItem",
            let title = value(hit, kAXTitleAttribute) as? String, !title.isEmpty, items.last != title
        else { continue }
        items.append(title)
    }
}
FileHandle.standardError.write(
    ("menu-read: \(waited)ms, \(items.count) item(s)\n").data(using: .utf8)!)
// The two ways the read can disappoint, named in the dump rather than left to a
// bare exit code.
if rect == nil {
    FileHandle.standardError.write("no menu window \(waited)ms after the Control-click\n".data(using: .utf8)!)
} else if items.isEmpty {
    FileHandle.standardError.write("the menu window drew no items\n".data(using: .utf8)!)
}
// The menu's own bounds, never the display: evidence can end up attached to a
// public PR, and the rest of the screen is the owner's.
if let rect {
    let shot = Process()
    shot.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    shot.arguments = ["-x", "-R", "\(Int(rect.minX)),\(Int(rect.minY)),\(Int(rect.width)),\(Int(rect.height))", args[4]]
    try? shot.run()
    shot.waitUntilExit()
}
key(53)
// Release only once the menu has closed, so the release cannot pick a row.
waitFor(kAXMenuClosedNotification, seconds: 2)
mouse(.leftMouseUp)
clearFlags()
// 3 is "the app drew no menu", the one failure a second click answers to
// (#1651). An empty read of a menu that did draw is this leaf's own bug, and a
// retry would hide it, so it exits 1.
if items.isEmpty { exit(rect == nil ? 3 : 1) }
items.forEach { print($0) }
exit(0)
SWIFT
  then
    break
  else
    code=$?
  fi
  [ "$code" -eq 3 ] || fail "the menu read failed (exit $code); see $menu_dump"
  if [ "$attempt" -ge 3 ]; then
    fail "the app drew no menu window after $attempt Control-clicks; see $menu_dump"
  fi
  echo "retry: the Control-click drew no menu window (#1651); clicking again (attempt $((attempt + 1)) of 3)" >&2
  echo "retry: that read is in $menu_dump" >&2
done

wait_for_trace 2 "$log" '^verbs: .*\[Menu\]' || fail "the Control-click did not land as Menu; see $log"
echo "ok: the Control-click landed as Menu"
! grep -q '^input-state:' "$menu_dump" || fail "the Control-click left the session's Control flag set; see $menu_dump"

for item in "Chat…" "Settings…" "Quit"; do
  grep -qxF "$item" "$menu_dump" || fail "no '$item' in the menu; see $menu_dump"
  echo "ok: the menu holds $item"
done

echo "PASS: evidence in $out ($(($(now_ms) - start_ms))ms)"
