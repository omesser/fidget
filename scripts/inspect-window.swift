// Prints the overlay's on-screen geometry as JSON. CGWindowListCopyWindowInfo
// reports bounds, owner and layer with no permission prompt, and no stock
// command-line tool reports a window's bounds or level.

// Not the crate's own WindowSource: that is deliberately blind to our own
// process, because a Character able to see the overlay would find a Perch under
// its own feet. This observer asks the window server, not the app.

import AppKit
import CoreGraphics
import Foundation

var displays: [[String: Any]] = []
var activeCount: UInt32 = 0
CGGetActiveDisplayList(0, nil, &activeCount)
var ids = [CGDirectDisplayID](repeating: 0, count: Int(activeCount))
CGGetActiveDisplayList(activeCount, &ids, &activeCount)
// The usable part of each display as well as its frame: the sprite rests on the
// near edge of the Dock and menu bar strips, and only NSScreen says where they
// are. NSScreen measures from the bottom and CoreGraphics from the top.
let screens = NSScreen.screens
for id in ids {
    let b = CGDisplayBounds(id)
    var usable: [String: Any] = ["x": b.origin.x, "y": b.origin.y, "w": b.width, "h": b.height]

    if let screen = screens.first(where: {
        ($0.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
            .uint32Value == id
    }) {
        let f = screen.frame
        let v = screen.visibleFrame
        let left = v.origin.x - f.origin.x
        let right = (f.origin.x + f.width) - (v.origin.x + v.width)
        let dock = v.origin.y - f.origin.y
        let menuBar = (f.origin.y + f.height) - (v.origin.y + v.height)
        usable = [
            "x": b.origin.x + left, "y": b.origin.y + menuBar,
            "w": b.width - left - right, "h": b.height - dock - menuBar,
        ]
    }

    displays.append([
        "id": Int(id), "x": b.origin.x, "y": b.origin.y,
        "w": b.width, "h": b.height,
        "usable": usable,
    ])
}

var windows: [[String: Any]] = []
// Everything the window server stacks above or below the ordinary application
// level: the menu bar, the Dock, the status items, Notification Centre. None is
// a Perch, and the frame loop is checked against these rectangles.
var elevated: [[String: Any]] = []
let opts = CGWindowListOption(arrayLiteral: .optionOnScreenOnly, .excludeDesktopElements)
if let list = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] {
    for w in list {
        let owner = w[kCGWindowOwnerName as String] as? String ?? ""
        let layer = w[kCGWindowLayer as String] as? Int ?? -999
        let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
        let entry: [String: Any] = [
            "owner": owner,
            "layer": layer,
            "alpha": w[kCGWindowAlpha as String] as? Double ?? -1,
            // 0 is NSWindowSharingNone: the window server will not hand this
            // window to any capture, which is how the Character stays out of a
            // screen share without anything having to detect one.
            "sharing": w[kCGWindowSharingState as String] as? Int ?? -1,
            "onscreen": w[kCGWindowIsOnscreen as String] as? Bool ?? false,
            "x": b["X"] as? Double ?? 0, "y": b["Y"] as? Double ?? 0,
            "w": b["Width"] as? Double ?? 0, "h": b["Height"] as? Double ?? 0,
        ]
        if owner.lowercased().contains("fidget") {
            windows.append(entry)
        } else if layer != 0 {
            elevated.append(entry)
        }
    }
}

let out = ["displays": displays, "windows": windows, "elevated": elevated]
let data = try JSONSerialization.data(withJSONObject: out, options: [.prettyPrinted])
print(String(data: data, encoding: .utf8)!)
