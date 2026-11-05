// Drives possession on macOS through the HID stream, as a person would.
// possess <pid> <x> <y>: press right on the sprite, drag onto the menu's Possess
// row and release there.
// key <virtual key> down|up: post one keyboard edge, with no modifier flags.
// move <x> <y>: move the cursor there, so the overlay can hit-test it.
// Usage: possession-input possess <pid> <x> <y> | key <code> down|up | move <x> <y>

import AppKit
import ApplicationServices

func fail(_ message: String, _ code: Int32 = 1) -> Never {
    FileHandle.standardError.write(Data("\(message)\n".utf8))
    exit(code)
}

let args = CommandLine.arguments
guard args.count >= 2 else { fail("usage: possession-input possess|key ...", 2) }

if args[1] == "key" {
    guard args.count == 4, let code = CGKeyCode(args[2]), ["down", "up"].contains(args[3]),
        let event = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: args[3] == "down")
    else { fail("usage: possession-input key <code> down|up", 2) }
    // A real arrow carries Fn and NumericPad, and a page reads the arrow from
    // them. Every other flag stays clear, Control above all (#1503).
    event.flags = (123...126).contains(code) ? [.maskSecondaryFn, .maskNumericPad] : []
    event.post(tap: .cghidEventTap)
    // ponytail: the post is asynchronous, and exiting at once drops the key.
    // 50 ms is what ax-settings' typing waits; replace it if a flush call exists.
    usleep(50_000)
    exit(0)
}

// The overlay passes clicks through until a tick sees the cursor over the art,
// so a scenario moves here and waits for frames before it presses.
if args[1] == "move" {
    guard args.count == 4, let x = Double(args[2]), let y = Double(args[3]),
        let event = CGEvent(
            mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: CGPoint(x: x, y: y),
            mouseButton: .left)
    else { fail("usage: possession-input move <x> <y>", 2) }
    event.flags = []
    event.post(tap: .cghidEventTap)
    exit(0)
}

guard args[1] == "possess", args.count == 5, let pid = pid_t(args[2]), let x = Double(args[3]),
    let y = Double(args[4])
else { fail("usage: possession-input possess <pid> <x> <y>", 2) }
let point = CGPoint(x: x, y: y)

// A right-click, not a Control-click: a synthetic Control can stay in the
// session's flags, and a Control-arrow would switch Spaces (#1503).
func mouse(_ type: CGEventType) {
    let event = CGEvent(
        mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .right)
    event?.flags = []
    event?.post(tap: .cghidEventTap)
}

func value(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var out: CFTypeRef?
    return AXUIElementCopyAttributeValue(element, name as CFString, &out) == .success ? out : nil
}

// The pop-up menu window sits at layer 100 or above (see control-click-menu.sh).
func menuWindow() -> CGRect? {
    let info =
        CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: AnyObject]]
        ?? []
    return info.first {
        ($0[kCGWindowOwnerPID as String] as? pid_t) == pid
            && (($0[kCGWindowLayer as String] as? Int) ?? 0) >= 100
    }.flatMap { CGRect(dictionaryRepresentation: $0[kCGWindowBounds as String] as! CFDictionary) }
}

// Every app notification bumps tick, so each wait returns on an event or its
// deadline, never on a clock of its own.
var seen: Set<String> = []
var tick = 0
var observer: AXObserver?
AXObserverCreate(
    pid,
    { _, _, name, _ in
        seen.insert(name as String)
        tick += 1
    }, &observer)
if let observer {
    let app = AXUIElementCreateApplication(pid)
    for name in [
        kAXMenuOpenedNotification, kAXMenuClosedNotification, kAXCreatedNotification,
        kAXWindowCreatedNotification, kAXLayoutChangedNotification,
    ] {
        AXObserverAddNotification(observer, app, name as CFString, nil)
    }
    CFRunLoopAddSource(
        CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(observer), .defaultMode)
}

func waitUntil(seconds: Double, _ done: () -> Bool) -> Bool {
    let deadline = Date().addingTimeInterval(seconds)
    var handled = -1
    while true {
        if handled != tick {
            handled = tick
            if done() { return true }
        }
        if Date() >= deadline { return false }
        CFRunLoopRunInMode(.defaultMode, deadline.timeIntervalSinceNow, true)
    }
}

func possessRow(in rect: CGRect, step: Double = 6) -> AXUIElement? {
    let system = AXUIElementCreateSystemWide()
    for rowY in stride(from: rect.minY + 4, to: rect.maxY, by: step) {
        var hit: AXUIElement?
        guard
            AXUIElementCopyElementAtPosition(system, Float(rect.midX), Float(rowY), &hit)
                == .success, let hit, value(hit, kAXRoleAttribute) as? String == "AXMenuItem",
            value(hit, kAXTitleAttribute) as? String == "Possess"
        else { continue }
        return hit
    }
    return nil
}

// The menu column near the click, read through Accessibility. A menu opens
// beside the cursor, above it near the bottom of the display.
func possessRowNearClick() -> AXUIElement? {
    // Rows are about 22 points tall, so a 12-point step lands on each.
    possessRow(in: CGRect(x: point.x + 16, y: point.y - 600, width: 0, height: 1200), step: 12)
}

// What the window list shows above the overlay, for when the lookup misses.
func describeHighWindows() -> String {
    let info =
        CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: AnyObject]]
        ?? []
    return info.filter { (($0[kCGWindowLayer as String] as? Int) ?? 0) >= 4 }.map {
        "window: owner=\($0[kCGWindowOwnerName as String] ?? "?" as AnyObject) "
            + "pid=\($0[kCGWindowOwnerPID as String] ?? 0 as AnyObject) "
            + "layer=\($0[kCGWindowLayer as String] ?? 0 as AnyObject) "
            + "bounds=\($0[kCGWindowBounds as String] ?? "" as AnyObject)"
    }.joined(separator: "\n")
}

mouse(.rightMouseDown)
var row: AXUIElement?
var how = "window list"
// ponytail: a menu owned by another process posts no notification to this
// observer, so the wait also re-reads every 100 ms. Drop the slice once the
// owner is known and observed.
let deadline = Date().addingTimeInterval(3)
while row == nil && Date() < deadline {
    if let rect = menuWindow() {
        row = possessRow(in: rect)
        if row == nil { break }
    } else if let near = possessRowNearClick() {
        row = near
        how = "Accessibility near the click"
    } else {
        CFRunLoopRunInMode(.defaultMode, 0.1, true)
    }
}
FileHandle.standardError.write(Data("possess: found the row by \(how): \(row != nil)\n".utf8))
if row == nil { FileHandle.standardError.write(Data((describeHighWindows() + "\n").utf8)) }
// A person's press-drag-release: drag the held right button onto the row and
// release it there. AXPress picks the row without the overlay ever taking key.
func center(_ element: AXUIElement) -> CGPoint? {
    var origin = CGPoint.zero
    var size = CGSize.zero
    guard let position = value(element, kAXPositionAttribute),
        let extent = value(element, kAXSizeAttribute),
        AXValueGetValue(position as! AXValue, .cgPoint, &origin),
        AXValueGetValue(extent as! AXValue, .cgSize, &size)
    else { return nil }
    return CGPoint(x: origin.x + size.width / 2, y: origin.y + size.height / 2)
}

func rightEvent(_ type: CGEventType, at: CGPoint) {
    let event = CGEvent(
        mouseEventSource: nil, mouseType: type, mouseCursorPosition: at, mouseButton: .right)
    event?.flags = []
    event?.post(tap: .cghidEventTap)
}

if let row, let target = center(row) {
    for step in 1...8 {
        let t = Double(step) / 8
        rightEvent(
            .rightMouseDragged,
            at: CGPoint(x: point.x + (target.x - point.x) * t, y: point.y + (target.y - point.y) * t))
        usleep(12_000)
    }
    rightEvent(.rightMouseUp, at: target)
    FileHandle.standardError.write(Data("possess: released on the row at \(target)\n".utf8))
    exit(0)
}
if let escape = CGEvent(keyboardEventSource: nil, virtualKey: 53, keyDown: true) {
    escape.post(tap: .cghidEventTap)
    CGEvent(keyboardEventSource: nil, virtualKey: 53, keyDown: false)?.post(tap: .cghidEventTap)
}
// Release only once the menu has closed, so the release cannot pick a row.
_ = waitUntil(seconds: 2) { seen.contains(kAXMenuClosedNotification) || menuWindow() == nil }
mouse(.rightMouseUp)
fail(row == nil ? "the menu has no Possess row" : "the Possess row has no frame")
