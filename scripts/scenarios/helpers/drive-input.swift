// Throwaway #1920 probe: synthetic input through the HID tap.
// Usage: drive-input click <x> <y> | key <code> | down <code> [repeat] | up <code>
import CoreGraphics
import Foundation

let args = CommandLine.arguments
func num(_ i: Int) -> Double { Double(args[i])! }
func key(_ code: Int, down: Bool, repeating: Bool = false) {
  let event = CGEvent(keyboardEventSource: nil, virtualKey: CGKeyCode(code), keyDown: down)
  if repeating { event?.setIntegerValueField(.keyboardEventAutorepeat, value: 1) }
  event?.post(tap: .cghidEventTap)
}
switch args[1] {
case "click":
  let at = CGPoint(x: num(2), y: num(3))
  for type in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
    CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: at, mouseButton: .left)?
      .post(tap: .cghidEventTap)
    usleep(30_000)
  }
case "key":
  key(Int(num(2)), down: true)
  usleep(30_000)
  key(Int(num(2)), down: false)
case "down":
  key(Int(num(2)), down: true, repeating: args.count > 3)
case "up":
  key(Int(num(2)), down: false)
default:
  exit(2)
}
