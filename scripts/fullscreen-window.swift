// A window over every whole display, so `fullscreen_displays` in
// crates/core/src/visibility.rs fades the Character: with a free display left,
// the Character would move there instead. The rule reads rectangles, not pixels,
// so each window is nearly transparent and lets every click through.

// It re-asserts its place at the front: an accessory window is buried by any
// focus change, and the rule takes the first window overlapping a display.
// Prints each window's bounds as a JSON line. Usage: swift scripts/fullscreen-window.swift [quit-after-secs]

import AppKit

let quitAfter = Double(CommandLine.arguments.dropFirst().first ?? "") ?? 120.0
let reassertInterval = 0.05

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let windows = NSScreen.screens.map { screen -> NSWindow in
    let window = NSWindow(
        contentRect: screen.frame, styleMask: [.borderless], backing: .buffered, defer: false)
    window.title = "fidget fullscreen prop"
    window.backgroundColor = .black
    window.alphaValue = 0.02
    window.ignoresMouseEvents = true
    window.level = .normal
    window.setFrame(screen.frame, display: false)
    window.orderFrontRegardless()
    return window
}
Timer.scheduledTimer(withTimeInterval: reassertInterval, repeats: true) { _ in
    windows.forEach { $0.orderFrontRegardless() }
}

RunLoop.current.run(until: Date().addingTimeInterval(0.3))
for window in windows {
    var line: [String: Double] = ["at_ms": Date().timeIntervalSince1970 * 1000]
    let entry = (CGWindowListCopyWindowInfo(
        .optionIncludingWindow, CGWindowID(window.windowNumber)) as? [[String: Any]])?.first
    if let settled = entry?[kCGWindowBounds as String] as? [String: Any] {
        line["x"] = settled["X"] as? Double ?? 0
        line["y"] = settled["Y"] as? Double ?? 0
        line["w"] = settled["Width"] as? Double ?? 0
        line["h"] = settled["Height"] as? Double ?? 0
    }
    print(String(data: try! JSONSerialization.data(withJSONObject: line), encoding: .utf8)!)
}
fflush(stdout)

DispatchQueue.main.asyncAfter(deadline: .now() + quitAfter) { app.terminate(nil) }
app.run()
