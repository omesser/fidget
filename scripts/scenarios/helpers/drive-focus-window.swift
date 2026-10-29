// Throwaway #1920 probe: a plain app window at (100,100) 500x350 in global
// top-left points that prints each key and focus event it gets, one per line.
// Usage: drive-focus-window
import AppKit

func say(_ line: String) {
  print(line)
  fflush(stdout)
}

final class Typing: NSView {
  override var acceptsFirstResponder: Bool { true }
  override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
  override func keyDown(with event: NSEvent) {
    say("window: keydown \(event.charactersIgnoringModifiers ?? "")")
  }
  override func mouseDown(with event: NSEvent) { say("window: mousedown") }
}

final class Watcher: NSObject, NSWindowDelegate {
  func windowDidBecomeKey(_ notification: Notification) { say("window: key") }
  func windowDidResignKey(_ notification: Notification) { say("window: resign key") }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let screen = NSScreen.screens[0].frame
let rect = NSRect(x: 100, y: screen.height - 100 - 350, width: 500, height: 350)
let window = NSWindow(
  contentRect: rect, styleMask: [.titled], backing: .buffered, defer: false)
window.title = "drive-focus-window"
let view = Typing(frame: NSRect(origin: .zero, size: rect.size))
window.contentView = view
let watcher = Watcher()
window.delegate = watcher
window.makeKeyAndOrderFront(nil)
window.makeFirstResponder(view)
app.activate(ignoringOtherApps: true)
say("window: up")
app.run()
