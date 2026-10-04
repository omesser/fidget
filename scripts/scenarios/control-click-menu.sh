#!/usr/bin/env bash
# Scenario: control-click-menu (macOS)
# On screen: launches Fidget as BMO with a fixture Harness. The scenario finds
#   the sprite, sends a Control-click to its centre, waits for the menu to
#   appear, and reads its items. One screenshot of the open menu, cropped to
#   the menu's bounds.
#   Fidget quits when the scenario ends.
# Input: one Control-click on the sprite (synthetic, via CGEvent), held while
#   the menu is read; Escape closes the menu before the release.
# Duration: about 20 s, 1 min at most.
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
out="${TMPDIR:-/tmp}/fidget-scenario-control-click-menu-$(date +%Y%m%d-%H%M%S)"
tools="${TMPDIR:-/tmp}/fidget-scenario-tools"
mkdir -p "$out/home" "$tools"
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

env HOME="$out/home" \
  FIDGET_HARNESS="$harness" \
  FIDGET_DIRECTOR_WAKE_SECS=3600 \
  FIDGET_DIRECTOR_API_KEY=x FIDGET_CAPTURABLE=1 FIDGET_TRACE_FRAMES=1 \
  FIDGET_CHARACTER=bmo FIDGET_CHARACTERS="$root/characters" \
  "$bin" > "$log" 2>&1 &
pid=$!
trap 'kill "$pid" 2> /dev/null || true; pkill -f "script=nop" || true' EXIT

wait_for() { # <seconds> <command...>
  local n=$(($1 * 4))
  shift
  until "$@" > /dev/null 2>&1; do
    kill -0 "$pid" 2> /dev/null || fail "Fidget exited; see $log"
    n=$((n - 1))
    [ "$n" -gt 0 ] || return 1
    sleep 0.25
  done
}

# The overlay spans the display, so its centre is not the sprite. The newest
# frame trace line says where the sprite is drawn, in global points.
wait_for 15 grep -q '^frame: .* sprite(' "$log" || fail "Fidget traced no frame; see $log"
sleep 2
read -r w h < <(sed -nE 's/.*sprite ([0-9]+)x([0-9]+);.*/\1 \2/p' "$log" | head -1) || fail "no sprite size in $log"
read -r x y < <(sed -nE 's/^frame: .* sprite\((-?[0-9]+),(-?[0-9]+)\) .*/\1 \2/p' "$log" | tail -1) || fail "no frame trace in $log"
cx=$((x + w / 2))
cy=$((y + h / 2))
echo "ok: sprite at ($x,$y) size ${w}x${h}, centre ($cx,$cy)"

# Hold the Control-down while the menu is read: a release before the menu
# tracks lands on the overlay as a Poke. Escape closes it, then the release.
menu_dump="$out/menu.ax.txt"
swift - "$pid" "$cx" "$cy" "$out/menu.png" > "$menu_dump" 2>&1 << 'SWIFT' || fail "no open menu in the AX tree; see $menu_dump"
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
func value(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var out: CFTypeRef?
    return AXUIElementCopyAttributeValue(element, name as CFString, &out) == .success ? out : nil
}

// The popped-up menu is no child of the application element, so its items are
// read by hit test down the middle of its menu-layer window.
func menuWindow() -> CGRect? {
    let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: AnyObject]] ?? []
    return info.first {
        ($0[kCGWindowOwnerPID as String] as? pid_t) == pid && (($0[kCGWindowLayer as String] as? Int) ?? 0) >= 100
    }.flatMap { CGRect(dictionaryRepresentation: $0[kCGWindowBounds as String] as! CFDictionary) }
}
mouse(.leftMouseDown)
var items: [String] = []
for _ in 0..<20 where items.isEmpty {
    usleep(250_000)
    guard let rect = menuWindow() else { continue }
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
// The menu's own bounds, never the display: evidence can end up attached to a
// public PR, and the rest of the screen is the owner's.
if let rect = menuWindow() {
    let shot = Process()
    shot.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    shot.arguments = ["-x", "-R", "\(Int(rect.minX)),\(Int(rect.minY)),\(Int(rect.width)),\(Int(rect.height))", args[4]]
    try? shot.run()
    shot.waitUntilExit()
}
key(53)
usleep(200_000)
mouse(.leftMouseUp)
items.forEach { print($0) }
exit(items.isEmpty ? 1 : 0)
SWIFT

wait_for 3 grep -q '^verbs: .*\[Menu\]' "$log" || fail "the Control-click did not land as Menu; see $log"
echo "ok: the Control-click landed as Menu"

for item in "Chat…" "Settings…" "Quit"; do
  grep -qxF "$item" "$menu_dump" || fail "no '$item' in the menu; see $menu_dump"
  echo "ok: the menu holds $item"
done

echo "PASS: evidence in $out"
