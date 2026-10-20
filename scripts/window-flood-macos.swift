// LOAD GENERATOR: covers the main display with `count` titled windows. Needs
// FIDGET_BENCH_GREEN_LIGHT=1; capped at 300 and quits after quit-after-secs.
// Usage: FIDGET_BENCH_GREEN_LIGHT=1 swift scripts/window-flood-macos.swift count [quit-after-secs]

import AppKit

guard ProcessInfo.processInfo.environment["FIDGET_BENCH_GREEN_LIGHT"] == "1" else {
    FileHandle.standardError.write(
        Data(
            "window-flood-macos.swift covers the screen with windows. Set FIDGET_BENCH_GREEN_LIGHT=1 once the person at the machine has agreed.\n"
                .utf8))
    exit(2)
}

let maxWindows = 300
let maxQuitAfterSecs = 3600.0
let usage =
    "usage: window-flood-macos.swift count(1-\(maxWindows)) [quit-after-secs(1-\(Int(maxQuitAfterSecs)))]\n"
let args = CommandLine.arguments.dropFirst()
guard let count = Int(args.first ?? ""), (1...maxWindows).contains(count) else {
    FileHandle.standardError.write(Data(usage.utf8))
    exit(2)
}
let quitAfter: Double
if let raw = args.dropFirst().first {
    guard let value = Double(raw), value.isFinite, (1...maxQuitAfterSecs).contains(value) else {
        FileHandle.standardError.write(Data(usage.utf8))
        exit(2)
    }
    quitAfter = value
} else {
    quitAfter = 180.0
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let bounds = CGDisplayBounds(CGMainDisplayID())
let size = NSSize(width: 160, height: 100)
// Past one screenful, windows stack on each other; a stacked window still
// appears in the on-screen list.
let columns = max(1, Int((bounds.width - size.width) / 24))
let rows = max(1, Int((bounds.height - size.height) / 24))
var windows: [NSWindow] = []
for i in 0..<count {
    let x = 24.0 * Double(i % columns)
    let y = 24.0 * Double((i / columns) % rows)
    let window = NSWindow(
        contentRect: NSRect(x: x, y: y, width: size.width, height: size.height),
        styleMask: [.titled], backing: .buffered, defer: false)
    window.title = "fidget flood \(i)"
    window.alphaValue = 0.3
    window.ignoresMouseEvents = true
    window.level = .normal
    window.orderFrontRegardless()
    windows.append(window)
}

let ours = Set(windows.map { $0.windowNumber })
// Near 300 windows the window server lags creation, so poll. At most 2s, well
// inside the bench's 10s wait for the JSON line.
let settleStep = 0.05
let settleSteps = 40
var onScreen: [[String: Any]] = []
var listed = 0
var listedDeadlineHit = true
for _ in 0..<settleSteps {
    RunLoop.current.run(until: Date().addingTimeInterval(settleStep))
    onScreen = (CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]]) ?? []
    listed = onScreen.filter { ours.contains($0[kCGWindowNumber as String] as? Int ?? -1) }.count
    if listed >= count {
        listedDeadlineHit = false
        break
    }
}
let line: [String: Any] = [
    "at_ms": Date().timeIntervalSince1970 * 1000, "opened": count,
    "listed": listed, "on_screen": onScreen.count, "listed_deadline_hit": listedDeadlineHit,
]
guard let data = try? JSONSerialization.data(withJSONObject: line),
    let text = String(data: data, encoding: .utf8)
else {
    FileHandle.standardError.write(Data("window-flood-macos.swift: failed to encode the result JSON\n".utf8))
    exit(1)
}
print(text)
fflush(stdout)

DispatchQueue.main.asyncAfter(deadline: .now() + quitAfter) { app.terminate(nil) }
app.run()
