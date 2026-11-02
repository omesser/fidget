// Close only this benchmark process's Chat through its native close button.
import ApplicationServices
import Foundation

let args = CommandLine.arguments
guard args.count == 3, let pid = Int32(args[1]) else {
    fputs("usage: bench-chat-close <pid> <title>\n", stderr)
    exit(2)
}
let app = AXUIElementCreateApplication(pid)
var raw: CFTypeRef?
guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &raw) == .success,
    let windows = raw as? [AXUIElement]
else { exit(1) }
for window in windows {
    var title: CFTypeRef?
    _ = AXUIElementCopyAttributeValue(window, kAXTitleAttribute as CFString, &title)
    guard title as? String == args[2] else { continue }
    var close: CFTypeRef?
    guard AXUIElementCopyAttributeValue(window, kAXCloseButtonAttribute as CFString, &close) == .success,
        let close, CFGetTypeID(close) == AXUIElementGetTypeID()
    else { exit(1) }
    exit(AXUIElementPerformAction(close as! AXUIElement, kAXPressAction as CFString) == .success ? 0 : 1)
}
exit(1)
