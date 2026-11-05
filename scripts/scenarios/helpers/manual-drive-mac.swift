// The macOS half of manual-drive's menu gesture. A Control-click, the same
// gesture control-click-menu sends, then the Possess row. The Control flag is
// cleared before this process exits: a later Right arrow must not switch
// Spaces.
// possess <pid> <x> <y>
// window <owner name>  prints "x y w h" of that owner's top window at layer 0.

import AppKit
import ApplicationServices

func fail(_ message: String, _ code: Int32 = 1) -> Never {
    FileHandle.standardError.write(Data("\(message)\n".utf8))
    exit(code)
}

let args = CommandLine.arguments
guard args.count >= 2 else { fail("usage: manual-drive-mac possess|window ...", 2) }

if args[1] == "window" {
    guard args.count == 3 else { fail("usage: manual-drive-mac window <owner>", 2) }
    let info =
        CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: AnyObject]]
        ?? []
    let rect = info.first {
        ($0[kCGWindowOwnerName as String] as? String) == args[2]
            && (($0[kCGWindowLayer as String] as? Int) ?? 0) == 0
    }.flatMap { CGRect(dictionaryRepresentation: $0[kCGWindowBounds as String] as! CFDictionary) }
    guard let rect else { fail("no \(args[2]) window") }
    print("\(Int(rect.minX)) \(Int(rect.minY)) \(Int(rect.width)) \(Int(rect.height))")
    exit(0)
}

guard args[1] == "possess", args.count == 5, let pid = pid_t(args[2]), let x = Double(args[3]),
    let y = Double(args[4])
else { fail("usage: manual-drive-mac possess <pid> <x> <y>", 2) }
let point = CGPoint(x: x, y: y)

func mouse(_ type: CGEventType) {
    let event = CGEvent(
        mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left)
    event?.flags = .maskControl
    event?.post(tap: .cghidEventTap)
}

func value(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var out: CFTypeRef?
    return AXUIElementCopyAttributeValue(element, name as CFString, &out) == .success ? out : nil
}

func menuWindow() -> CGRect? {
    let info =
        CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: AnyObject]]
        ?? []
    return info.first {
        ($0[kCGWindowOwnerPID as String] as? pid_t) == pid
            && (($0[kCGWindowLayer as String] as? Int) ?? 0) >= 100
    }.flatMap { CGRect(dictionaryRepresentation: $0[kCGWindowBounds as String] as! CFDictionary) }
}

// A synthetic Control-click leaves the session flag set. One flagless move
// clears it, the same way control-click-menu does, before any arrow is posted.
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
    let deadline = Date().addingTimeInterval(1)
    while CGEventSource.flagsState(.combinedSessionState).contains(.maskControl) && Date() < deadline {
        usleep(20_000)
    }
    if CGEventSource.flagsState(.combinedSessionState).contains(.maskControl) {
        fail("the Control-click left the session's Control flag set")
    }
}

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
    for name in [kAXMenuOpenedNotification, kAXMenuClosedNotification, kAXCreatedNotification] {
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

mouse(.leftMouseDown)
var rect: CGRect?
let opened = waitUntil(seconds: 3) {
    rect = menuWindow()
    return rect != nil
}
guard opened, let rect else {
    mouse(.leftMouseUp)
    clearFlags()
    fail("no menu window within 3 s of the Control-click")
}
let row = possessRow(in: rect)
if let row {
    AXUIElementPerformAction(row, kAXPressAction as CFString)
} else if let escape = CGEvent(keyboardEventSource: nil, virtualKey: 53, keyDown: true) {
    escape.post(tap: .cghidEventTap)
    CGEvent(keyboardEventSource: nil, virtualKey: 53, keyDown: false)?.post(tap: .cghidEventTap)
}
_ = waitUntil(seconds: 2) { seen.contains(kAXMenuClosedNotification) || menuWindow() == nil }
mouse(.leftMouseUp)
clearFlags()
if row == nil { fail("the menu has no Possess row") }
