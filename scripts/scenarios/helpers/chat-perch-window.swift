// Read or move one window of the test process without activating it.
import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

let args = Array(CommandLine.arguments.dropFirst())
guard let mode = args.first else { exit(2) }

if mode == "frontmost" {
    print(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1)
    exit(0)
}

if mode == "ax-trusted" {
    print(AXIsProcessTrusted() ? "yes" : "no")
    exit(0)
}

if mode == "focused-window" {
    guard AXIsProcessTrusted() else {
        FileHandle.standardError.write(Data("Accessibility trust is absent\n".utf8))
        exit(2)
    }
    guard let frontmost = NSWorkspace.shared.frontmostApplication else {
        FileHandle.standardError.write(Data("frontmost application unavailable\n".utf8))
        exit(1)
    }
    let focusedPid = frontmost.processIdentifier
    let focusedApp = AXUIElementCreateApplication(focusedPid)
    var windowValue: CFTypeRef?
    let windowStatus = AXUIElementCopyAttributeValue(focusedApp, kAXFocusedWindowAttribute as CFString, &windowValue)
    guard windowStatus == .success,
          let focusedWindow = windowValue, CFGetTypeID(focusedWindow) == AXUIElementGetTypeID()
    else {
        FileHandle.standardError.write(Data("frontmost pid \(focusedPid) has no AX focused window (\(windowStatus.rawValue))\n".utf8))
        exit(1)
    }
    var titleValue: CFTypeRef?
    let titleStatus = AXUIElementCopyAttributeValue(focusedWindow as! AXUIElement, kAXTitleAttribute as CFString, &titleValue)
    guard titleStatus == .success,
          let title = titleValue as? String
    else {
        FileHandle.standardError.write(Data("frontmost pid \(focusedPid) focused window has no title (\(titleStatus.rawValue))\n".utf8))
        exit(1)
    }
    print("\(focusedPid)|\(title)")
    exit(0)
}

func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success else { return nil }
    return value
}

func string(_ element: AXUIElement, _ key: String) -> String {
    attribute(element, key) as? String ?? ""
}

func enclosingWindow(_ element: AXUIElement) -> AXUIElement? {
    var current = element
    for _ in 0..<20 {
        if string(current, kAXRoleAttribute) == kAXWindowRole as String { return current }
        if let value = attribute(current, kAXWindowAttribute), CFGetTypeID(value) == AXUIElementGetTypeID() {
            return (value as! AXUIElement)
        }
        guard let value = attribute(current, kAXParentAttribute), CFGetTypeID(value) == AXUIElementGetTypeID() else {
            return nil
        }
        current = value as! AXUIElement
    }
    return nil
}

func describe(_ element: AXUIElement) -> String? {
    guard let window = enclosingWindow(element) else { return nil }
    var owner: pid_t = 0
    guard AXUIElementGetPid(window, &owner) == .success else { return nil }
    return "\(owner)|\(string(window, kAXTitleAttribute))|\(string(element, kAXRoleAttribute))|\(string(element, kAXTitleAttribute))"
}

if mode == "element-at" {
    guard args.count == 3, let x = Float(args[1]), let y = Float(args[2]), AXIsProcessTrusted() else { exit(2) }
    var target: AXUIElement?
    guard AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(), x, y, &target) == .success,
          let element = target, let description = describe(element)
    else { exit(1) }
    print(description)
    exit(0)
}

if mode == "focused-element" {
    guard AXIsProcessTrusted(), let frontmost = NSWorkspace.shared.frontmostApplication else { exit(2) }
    let app = AXUIElementCreateApplication(frontmost.processIdentifier)
    guard let value = attribute(app, kAXFocusedUIElementAttribute), CFGetTypeID(value) == AXUIElementGetTypeID(),
          let description = describe(value as! AXUIElement)
    else { exit(1) }
    print(description)
    exit(0)
}

if mode == "screen" {
    let b = CGDisplayBounds(CGMainDisplayID())
    print("\(Int(b.minX)),\(Int(b.minY)),\(Int(b.width)),\(Int(b.height))")
    exit(0)
}

guard args.count >= 3, let pid = Int(args[1]) else { exit(2) }
let title = args[2]

if mode == "info" {
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
    for w in windows
    where (w[kCGWindowOwnerPID as String] as? Int) == pid
        && (w[kCGWindowName as String] as? String) == title
    {
        guard let b = w[kCGWindowBounds as String] as? [String: Any],
              let x = b["X"] as? Int, let y = b["Y"] as? Int,
              let width = b["Width"] as? Int, let height = b["Height"] as? Int,
              let layer = w[kCGWindowLayer as String] as? Int
        else { exit(1) }
        print("\(x),\(y),\(width),\(height),\(layer)")
        exit(0)
    }
    exit(1)
}

if mode == "move" {
    guard args.count == 5, let x = Double(args[3]), let y = Double(args[4]),
          AXIsProcessTrusted()
    else { exit(2) }
    let app = AXUIElementCreateApplication(pid_t(pid))
    var windows: CFTypeRef?
    guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &windows) == .success,
          let list = windows as? [AXUIElement]
    else { exit(1) }
    for window in list {
        var name: CFTypeRef?
        guard AXUIElementCopyAttributeValue(window, kAXTitleAttribute as CFString, &name) == .success,
              (name as? String) == title
        else { continue }
        var point = CGPoint(x: x, y: y)
        guard let value = AXValueCreate(.cgPoint, &point),
              AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, value) == .success
        else { exit(1) }
        exit(0)
    }
    exit(1)
}

exit(2)
