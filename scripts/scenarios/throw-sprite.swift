// Throws the sprite: a real press at a top-left-origin point, a fast drag of dx
// points, then the release, so the app sees a Grab and a Throw.
// Usage: swift scripts/scenarios/throw-sprite.swift x y dx

import AppKit

let args = CommandLine.arguments
guard args.count == 4, let x = Double(args[1]), let y = Double(args[2]), let dx = Double(args[3])
else {
    FileHandle.standardError.write(Data("usage: throw-sprite.swift x y dx\n".utf8))
    exit(2)
}
guard let source = CGEventSource(stateID: .combinedSessionState) else {
    FileHandle.standardError.write(Data("could not create CGEventSource\n".utf8))
    exit(1)
}

func post(_ type: CGEventType, _ point: CGPoint) {
    CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: point, mouseButton: .left)?
        .post(tap: .cghidEventTap)
}

let start = CGPoint(x: x, y: y)
CGWarpMouseCursorPosition(start)
post(.mouseMoved, start)
Thread.sleep(forTimeInterval: 0.12)
post(.leftMouseDown, start)
// A drag past DRAG_THRESHOLD makes this a Grab; the hold lets the press land first.
Thread.sleep(forTimeInterval: 0.15)
var point = start
for step in 1...8 {
    point = CGPoint(x: x + dx * Double(step) / 8, y: y - 10 * Double(step) / 8)
    post(.leftMouseDragged, point)
    Thread.sleep(forTimeInterval: 0.012)
}
post(.leftMouseUp, point)
print("threw from (\(Int(x)),\(Int(y))) by \(Int(dx))")
