// A plain window that another app's keys can land in, for scenarios that
// check focus left Fidget. Logs to stdout, one line per event:
//   ready <x> <y> <w> <h>   the window's frame, top-left global points
//   focus | blur            it became or stopped being the key window
//   key: <chars>            a keyDown it received
// work-area <x> <y> prints the usable area of the display holding that point
// as "<x> <y> <w> <h>", top-left global points, and exits.
// frontmost prints the pid of the app in front.
// Usage: type-target window <x> <y> <w> <h> | work-area <x> <y> | frontmost

import AppKit

setvbuf(stdout, nil, _IOLBF, 0)
let args = Array(CommandLine.arguments.dropFirst())
let primaryHeight = NSScreen.screens.first?.frame.height ?? 0

func topLeft(_ rect: NSRect) -> String {
    "\(Int(rect.minX)) \(Int(primaryHeight - rect.maxY)) \(Int(rect.width)) \(Int(rect.height))"
}

if args.first == "frontmost" {
    print(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1)
    exit(0)
}

if args.first == "work-area", args.count == 3, let x = Double(args[1]), let y = Double(args[2]) {
    let point = NSPoint(x: x, y: primaryHeight - y)
    guard let screen = NSScreen.screens.first(where: { NSMouseInRect(point, $0.frame, false) })
    else { exit(1) }
    print(topLeft(screen.visibleFrame))
    exit(0)
}

guard args.first == "window", args.count == 5, let x = Double(args[1]), let y = Double(args[2]),
    let w = Double(args[3]), let h = Double(args[4])
else {
    FileHandle.standardError.write(Data("usage: type-target window x y w h | work-area x y\n".utf8))
    exit(2)
}

final class Field: NSTextView {
    override func keyDown(with event: NSEvent) {
        print("key: \(event.characters ?? "")")
        super.keyDown(with: event)
    }
}

final class Watcher: NSObject, NSWindowDelegate {
    func windowDidBecomeKey(_ notification: Notification) { print("focus") }
    func windowDidResignKey(_ notification: Notification) { print("blur") }
}

let app = NSApplication.shared
// Accessory, so it never makes itself active. Only the scenario's click does.
app.setActivationPolicy(.accessory)
let frame = NSRect(x: x, y: primaryHeight - y - h, width: w, height: h)
let window = NSWindow(
    contentRect: frame, styleMask: [.titled], backing: .buffered, defer: false)
window.title = "fidget-type-target"
let field = Field(frame: NSRect(origin: .zero, size: frame.size))
window.contentView = field
window.makeFirstResponder(field)
let watcher = Watcher()
window.delegate = watcher
window.orderFrontRegardless()
print("ready \(topLeft(window.frame))")
app.run()
