// Places the cursor and prints where it landed. `--event` posts a move;
// without it the warp stays silent, so hover does not follow.
// Usage: swift scripts/warp-cursor.swift x y [--event]

import AppKit

let args = CommandLine.arguments
let notify = args.count == 4 && args[3] == "--event"
guard (args.count == 3 || notify), let x = Double(args[1]), let y = Double(args[2]) else {
    FileHandle.standardError.write(Data("usage: warp-cursor.swift x y [--event]\n".utf8))
    exit(2)
}

let point = CGPoint(x: x, y: y)
CGWarpMouseCursorPosition(point)
if notify,
    let source = CGEventSource(stateID: .combinedSessionState),
    let moved = CGEvent(
        mouseEventSource: source,
        mouseType: .mouseMoved,
        mouseCursorPosition: point,
        mouseButton: .left)
{
    moved.post(tap: .cghidEventTap)
}

// Read the cursor back rather than trusting the warp: the window server clamps
// to the displays it has, so a point off every screen quietly lands elsewhere
// and the caller has to be able to tell.
let mainHeight = CGDisplayBounds(CGMainDisplayID()).height
let landed = NSEvent.mouseLocation
print(String(format: "%.0f %.0f", landed.x, mainHeight - landed.y))
