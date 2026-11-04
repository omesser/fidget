// Drives possession on macOS through the HID stream, as a person would.
// possess <pid> <x> <y>: right-click the sprite and press the menu's Possess row.
// key <virtual key> down|up: post one keyboard edge, with no modifier flags.
// Usage: possession-input possess <pid> <x> <y> | possession-input key <code> down|up

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

func possessRow(in rect: CGRect) -> AXUIElement? {
    let system = AXUIElementCreateSystemWide()
    for rowY in stride(from: rect.minY + 4, to: rect.maxY, by: 6) {
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

mouse(.rightMouseDown)
var rect: CGRect?
let opened = waitUntil(seconds: 3) {
    rect = menuWindow()
    return rect != nil
}
guard opened, let rect else {
    mouse(.rightMouseUp)
    fail("no menu window within 3 s of the right-click")
}
let row = possessRow(in: rect)
if let row {
    AXUIElementPerformAction(row, kAXPressAction as CFString)
} else if let escape = CGEvent(keyboardEventSource: nil, virtualKey: 53, keyDown: true) {
    escape.post(tap: .cghidEventTap)
    CGEvent(keyboardEventSource: nil, virtualKey: 53, keyDown: false)?.post(tap: .cghidEventTap)
}
// Release only once the menu has closed, so the release cannot pick a row.
_ = waitUntil(seconds: 2) { seen.contains(kAXMenuClosedNotification) || menuWindow() == nil }
mouse(.rightMouseUp)
if row == nil { fail("the menu has no Possess row") }
