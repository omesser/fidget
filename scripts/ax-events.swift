// Print one line per Accessibility change notification from a process until
// killed, so scenario waits block on AXObserver events instead of polling.
// Usage: ax-events <pid>
import ApplicationServices
import Foundation

setvbuf(stdout, nil, _IOLBF, 0)
guard CommandLine.arguments.count == 2, let pid = pid_t(CommandLine.arguments[1]) else {
    FileHandle.standardError.write("usage: ax-events <pid>\n".data(using: .utf8)!)
    exit(2)
}

let notifications = [
    kAXCreatedNotification, kAXUIElementDestroyedNotification, kAXValueChangedNotification,
    kAXTitleChangedNotification, kAXWindowCreatedNotification, kAXFocusedUIElementChangedNotification,
    kAXFocusedWindowChangedNotification, kAXLayoutChangedNotification, kAXResizedNotification,
    kAXMovedNotification, kAXMenuOpenedNotification, kAXMenuClosedNotification,
    kAXSelectedChildrenChangedNotification, kAXWindowMiniaturizedNotification,
    kAXWindowDeminiaturizedNotification,
]

var observer: AXObserver?
let callback: AXObserverCallback = { _, _, notification, _ in
    print(notification as String)
}
guard AXObserverCreate(pid, callback, &observer) == .success, let observer else {
    FileHandle.standardError.write("could not observe pid \(pid)\n".data(using: .utf8)!)
    exit(1)
}
let app = AXUIElementCreateApplication(pid)
for name in notifications {
    AXObserverAddNotification(observer, app, name as CFString, nil)
}
CFRunLoopAddSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(observer), .defaultMode)
print("ready")
CFRunLoopRun()
