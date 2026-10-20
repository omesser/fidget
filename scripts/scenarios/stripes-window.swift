// Fills the main display with hard-edged diagonal stripes at the normal window
// level, so a capture of a see-through window shows a known backdrop rather
// than the desktop. Usage: stripes-window
import AppKit

final class Stripes: NSView {
  override func draw(_ dirtyRect: NSRect) {
    let colors: [NSColor] = [.systemRed, .systemYellow, .systemGreen, .systemBlue]
    let (w, h) = (bounds.width, bounds.height)
    for (n, x) in stride(from: -h, to: w, by: 30).enumerated() {
      colors[n % colors.count].setFill()
      let band = NSBezierPath()
      band.move(to: NSPoint(x: x, y: 0))
      band.line(to: NSPoint(x: x + 30, y: 0))
      band.line(to: NSPoint(x: x + 30 + h, y: h))
      band.line(to: NSPoint(x: x + h, y: h))
      band.fill()
    }
  }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let frame = NSScreen.main!.frame
let window = NSWindow(contentRect: frame, styleMask: .borderless, backing: .buffered, defer: false)
window.contentView = Stripes(frame: frame)
window.ignoresMouseEvents = true
window.orderFront(nil)
app.run()
