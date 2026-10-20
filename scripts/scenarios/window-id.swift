// Prints the CGWindowID of <pid>'s on-screen window titled <title>, for
// `screencapture -l`, or with `bounds` its frame as x,y,w,h for `-R`. Any layer:
// a focused Chat floats at layer 25. Reads the window server, so no grant.
import CoreGraphics
import Foundation

let args = CommandLine.arguments
guard args.count == 3 || (args.count == 4 && args[3] == "bounds"), let pid = Int(args[1]) else {
    FileHandle.standardError.write("usage: window-id <pid> <title> [bounds]\n".data(using: .utf8)!)
    exit(2)
}
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for window in windows
where (window[kCGWindowOwnerPID as String] as? Int) == pid
    && (window[kCGWindowName as String] as? String) == args[2]
{
    if args.count == 4 {
        let b = window[kCGWindowBounds as String] as! [String: Double]
        print([b["X"]!, b["Y"]!, b["Width"]!, b["Height"]!].map { String(Int($0)) }.joined(separator: ","))
    } else {
        print(window[kCGWindowNumber as String] as! Int)
    }
    exit(0)
}
exit(1)
