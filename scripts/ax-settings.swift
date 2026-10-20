// Drives and reads the macOS Settings window through the Accessibility API.

// Not osascript: System Events cannot report a control's AXEnabled, and it
// addresses controls by index, so a reordered section silently checks the
// wrong row. Coordinates appear nowhere; controls are pressed by name.

import AppKit
import ApplicationServices
import Foundation

let args = Array(CommandLine.arguments.dropFirst())

func die(_ message: String) -> Never {
    FileHandle.standardError.write("ax-settings: \(message)\n".data(using: .utf8)!)
    exit(2)
}

guard args.count >= 2, let pid = pid_t(args[1]) else {
    die(
        "usage: ax-settings <open|tab|pick|dump|frame|popup-frame|open-popup|type-select|move|size|key|type|value|focused|focus-window|menus|press-button> <pid> [args]"
    )
}

guard AXIsProcessTrusted() else {
    die("""
        no Accessibility grant. Add the terminal running this to
        System Settings > Privacy & Security > Accessibility, then run again.
        """)
}

let app = AXUIElementCreateApplication(pid)

func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else {
        return nil
    }
    return value
}

func string(_ element: AXUIElement, _ name: String) -> String? {
    attribute(element, name) as? String
}

func children(_ element: AXUIElement) -> [AXUIElement] {
    attribute(element, kAXChildrenAttribute) as? [AXUIElement] ?? []
}

/// The first element a predicate accepts, depth-first: the tab group sits
/// deeper than the window's own buttons, and the first match by either order is
/// the same element in every tree this drives.
func find(
    _ element: AXUIElement,
    depth: Int = 0,
    where accept: (AXUIElement) -> Bool
) -> AXUIElement? {
    if accept(element) { return element }
    // The tree under a Settings window is shallow; the bound only stops a
    // runaway on a cycle, which AX does produce through AXParent links.
    guard depth < 30 else { return nil }
    for child in children(element) {
        if let hit = find(child, depth: depth + 1, where: accept) { return hit }
    }
    return nil
}

/// Polls rather than sleeping a fixed span: the window server answers as soon
/// as the window exists, and a fixed sleep is either a stall or a flake.
func waitFor(
    _ seconds: Double,
    _ probe: () -> AXUIElement?
) -> AXUIElement? {
    let deadline = Date().addingTimeInterval(seconds)
    repeat {
        if let hit = probe() { return hit }
        usleep(100_000)
    } while Date() < deadline
    return nil
}

var lastError = "none attempted"

/// The actions an element answers to, for the failure message.
func actions(_ element: AXUIElement) -> [String] {
    var names: CFArray?
    guard AXUIElementCopyActionNames(element, &names) == .success else { return [] }
    return names as? [String] ?? []
}

/// The element's own on-screen rectangle, in the global coordinates a
/// synthesized click wants.
func frame(_ element: AXUIElement) -> CGRect? {
    guard let posValue = attribute(element, kAXPositionAttribute),
        let sizeValue = attribute(element, kAXSizeAttribute)
    else { return nil }
    var origin = CGPoint.zero
    var size = CGSize.zero
    guard AXValueGetValue(posValue as! AXValue, .cgPoint, &origin),
        AXValueGetValue(sizeValue as! AXValue, .cgSize, &size)
    else { return nil }
    return CGRect(origin: origin, size: size)
}

/// A real click at the element's centre, for elements AX describes but will
/// not act on: the status item answers AXPress with kAXErrorCannotComplete,
/// its menu being the window server's. AXPosition keeps the point honest.
func click(_ element: AXUIElement) -> Bool {
    guard let rect = frame(element) else { return false }
    let point = CGPoint(x: rect.midX, y: rect.midY)
    guard let down = CGEvent(mouseEventSource: nil, mouseType: .leftMouseDown,
                             mouseCursorPosition: point, mouseButton: .left),
        let up = CGEvent(mouseEventSource: nil, mouseType: .leftMouseUp,
                         mouseCursorPosition: point, mouseButton: .left)
    else { return false }
    down.post(tap: .cghidEventTap)
    usleep(60_000)
    up.post(tap: .cghidEventTap)
    return true
}

func menuWindowCount() -> Int { menuWindows().count }

/// macOS menus select by typed prefix, so this picks an item by name from a
/// menu the tree offers no element for. The tracking menu takes the keyboard.
func typeSelect(_ title: String) {
    for character in title {
        var utf16 = Array(String(character).utf16)
        for down in [true, false] {
            guard let event = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: down)
            else { continue }
            event.keyboardSetUnicodeString(stringLength: utf16.count, unicodeString: &utf16)
            event.post(tap: .cghidEventTap)
        }
        usleep(50_000)
    }
    // 36 is Return. The tracking menu consumes it, so the Settings window's own
    // Enter handler never sees it and the window stays open.
    for down in [true, false] {
        CGEvent(keyboardEventSource: nil, virtualKey: 36, keyDown: down)?.post(tap: .cghidEventTap)
        usleep(30_000)
    }
}

/// A status item answers to AXPress on some macOS versions and only to
/// AXShowMenu on others, and which one has never been documented.
func press(_ element: AXUIElement) -> Bool {
    for action in [kAXPressAction as String, "AXShowMenu", kAXPickAction as String] {
        guard actions(element).contains(action) else { continue }
        let err = AXUIElementPerformAction(element, action as CFString)
        if err == .success { return true }
        lastError = "\(action) -> \(err.rawValue)"
    }
    return click(element)
}

func window(titled title: String) -> AXUIElement? {
    let windows = attribute(app, kAXWindowsAttribute) as? [AXUIElement] ?? []
    return windows.first { string($0, kAXTitleAttribute) == title }
}

func settingsWindow() -> AXUIElement? { window(titled: "Settings") }

/// The popup whose preceding static text is `label`. Render order, not index:
/// a source change rebuilds the webview tree, so callers resolve this fresh.
func popup(labelled label: String) -> AXUIElement? {
    guard let window = settledWindow(titled: "Settings") else { return nil }
    var lastLabel = ""
    var hit: AXUIElement?
    func scan(_ element: AXUIElement, depth: Int) {
        guard depth < 30, hit == nil else { return }
        let role = string(element, kAXRoleAttribute) ?? ""
        if role == "AXStaticText" { lastLabel = string(element, kAXValueAttribute) ?? "" }
        if role == "AXPopUpButton", lastLabel == label {
            hit = element
            return
        }
        for child in children(element) { scan(child, depth: depth + 1) }
    }
    scan(window, depth: 0)
    return hit
}

/// On-screen menu-layer windows this pid owns. WebKit's `<select>` menu
/// publishes to no accessibility tree, not even by hit test, so such a window
/// is the only evidence that it opened.
func menuWindows() -> [[String: AnyObject]] {
    let info = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
    return ((info as? [[String: AnyObject]]) ?? []).filter {
        ($0[kCGWindowOwnerPID as String] as? pid_t) == pid
            && (($0[kCGWindowLayer as String] as? Int) ?? 0) >= 100
    }
}

func rectText(_ rect: CGRect) -> String {
    "\(Int(rect.origin.x)),\(Int(rect.origin.y)),\(Int(rect.width)),\(Int(rect.height))"
}

func printRect(_ rect: CGRect, extra: String = "") {
    print(rectText(rect) + (extra.isEmpty ? "" : ",\(extra)"))
}

/// Keys have to land in the Settings webview, not the terminal that posted them.
func activateSettings() {
    if let running = NSRunningApplication(processIdentifier: pid) {
        running.activate()
    }
    if let window = settingsWindow() {
        _ = AXUIElementPerformAction(window, kAXRaiseAction as CFString)
    }
    usleep(200_000)
}

func postKey(_ virtualKey: CGKeyCode, flags: CGEventFlags = []) {
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: nil, virtualKey: virtualKey, keyDown: down)
        else { continue }
        event.flags = flags
        event.post(tap: .cghidEventTap)
        usleep(30_000)
    }
    usleep(120_000)
}

func typeText(_ title: String) {
    for character in title {
        var utf16 = Array(String(character).utf16)
        for down in [true, false] {
            guard let event = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: down)
            else { continue }
            event.keyboardSetUnicodeString(stringLength: utf16.count, unicodeString: &utf16)
            event.post(tap: .cghidEventTap)
        }
        usleep(50_000)
    }
}

func dumpOne(_ element: AXUIElement) -> String {
    let role = string(element, kAXRoleAttribute) ?? "?"
    let subrole = string(element, kAXSubroleAttribute) ?? ""
    let title = string(element, kAXTitleAttribute) ?? ""
    let value = attribute(element, kAXValueAttribute).map { "\($0)" } ?? ""
    let enabled = (attribute(element, kAXEnabledAttribute) as? Bool).map(String.init) ?? ""
    let flat = { (s: String) in s.replacingOccurrences(of: "\n", with: "\\n") }
    return "\(role)\(subrole.isEmpty ? "" : ":" + subrole)|\(flat(title))|\(flat(value))|\(enabled)"
}

/// An unprimed WKWebView shows as a childless AXGroup. Asking for the tree is
/// what primes it (the subtree lands ~107 ms later), so sleeping never works.
/// The native Settings window has no such child, so the shape is unambiguous.
func webContentSettled(_ window: AXUIElement) -> Bool {
    if let area = find(window, where: { string($0, kAXRoleAttribute) == "AXWebArea" }) {
        return !children(area).isEmpty
    }
    return !children(window).contains {
        string($0, kAXRoleAttribute) == "AXGroup" && children($0).isEmpty
    }
}

/// Commands that address a control the webview owns wait for that priming.
/// `open`, `move`, `frame` and `focus-window` do not: the status item and the
/// window rectangle are AppKit's.
func settledWindow(titled title: String) -> AXUIElement? {
    guard let window = window(titled: title) else { return nil }
    _ = waitFor(5, { webContentSettled(window) ? window : nil })
    return window
}

switch args[0] {
case "open":
    // The status item, not the app's own menu bar: Settings has no keyboard
    // shortcut and no menu of its own, so the tray row is the only way in.
    let rowPrefix = args.count >= 3 ? args[2] : "Settings"
    let windowTitle = args.count >= 4 ? args[3] : "Settings"
    guard let extras = waitFor(20, { attribute(app, "AXExtrasMenuBar").map { $0 as! AXUIElement } })
    else {
        die("no status item after 20s - did the app finish launching?")
    }
    guard let item = children(extras).first else { die("the app has no status item") }
    // A real click, not AXPress: tray-icon 0.25 attaches the menu only in its
    // mouse-down handler, so AXPress on the item opens nothing.
    guard click(item) else { die("the status item has no frame to click") }
    guard
        let row = waitFor(
            5,
            {
                find(extras) { string($0, kAXTitleAttribute)?.hasPrefix(rowPrefix) == true }
            })
    else {
        die("the tray menu has no \(rowPrefix) row")
    }
    guard press(row) else { die("could not press \(rowPrefix)…") }
    guard waitFor(10, { window(titled: windowTitle) }) != nil else {
        die("\(windowTitle) did not open within 10s")
    }

case "tab":
    guard args.count >= 3 else { die("usage: ax-settings tab <pid> <title>") }
    guard let window = settledWindow(titled: "Settings") else { die("Settings is not open") }
    guard let tab = find(window, where: { string($0, kAXTitleAttribute) == args[2] }),
        press(tab)
    else {
        die("no tab titled \(args[2])")
    }

case "pick":
    // Changing a popup in place is the only way to reach the states between
    // two launches.
    guard args.count >= 4 else { die("usage: ax-settings pick <pid> <label> <option>") }
    guard let target = popup(labelled: args[2]) else { die("no popup labelled \(args[2])") }
    // Setting AXValue is refused by NSPopUpButton, so open it and press the
    // row: the same path a person takes, and the only one that fires the
    // action the renderer listens for.
    let menusBefore = menuWindowCount()
    guard press(target) else { die("could not open the \(args[2]) popup") }
    // AppKit publishes its NSMenu under the popup, so the option is an element
    // to press. WebKit's `<select>` menu publishes nothing, so a new menu-layer
    // window is the signal and the option is typed. One loop polls both.
    var option: AXUIElement?
    var drew = false
    let deadline = Date().addingTimeInterval(5)
    repeat {
        option = find(target) { string($0, kAXTitleAttribute) == args[3] }
        drew = menuWindowCount() > menusBefore
        if option != nil || drew { break }
        usleep(100_000)
    } while Date() < deadline
    if let option = option {
        guard press(option) else { die("could not pick \(args[3])") }
    } else if drew {
        typeSelect(args[3])
    } else {
        die("the \(args[2]) popup drew no menu; last: \(lastError)")
    }
    // The row commits on the app's own thread and the window redraws the line
    // under it from what was saved, so neither is true the instant the press
    // returns. Waiting for the new title is what makes `pick` mean "the row took it".
    guard
        waitFor(
            10,
            {
                guard let fresh = popup(labelled: args[2]),
                    string(fresh, kAXValueAttribute) == args[3],
                    find(fresh, where: { string($0, kAXRoleAttribute) == "AXMenu" }) == nil
                else { return nil }
                return fresh
            }) != nil
    else {
        die(
            "the \(args[2]) popup still reads "
                + "\(popup(labelled: args[2]).flatMap { string($0, kAXValueAttribute) } ?? "nothing") "
                + "after picking \(args[3])")
    }

case "popup-frame":
    guard args.count >= 3 else { die("usage: ax-settings popup-frame <pid> <label>") }
    guard let target = popup(labelled: args[2]), let rect = frame(target) else {
        die("no popup labelled \(args[2])")
    }
    printRect(rect)

case "open-popup":
    // Press the popup and leave the menu up. A still of that menu is needed
    // against the overlay; `pick` would dismiss it before the capture.
    guard args.count >= 3 else { die("usage: ax-settings open-popup <pid> <label>") }
    var opened = menuWindows().first
    if opened == nil {
        guard let target = popup(labelled: args[2]) else { die("no popup labelled \(args[2])") }
        let menusBefore = menuWindowCount()
        guard press(target) else { die("could not open the \(args[2]) popup") }
        let deadline = Date().addingTimeInterval(5)
        repeat {
            let menus = menuWindows()
            if menus.count > menusBefore {
                opened = menus.first
                break
            }
            usleep(100_000)
        } while Date() < deadline
    }
    guard let opened,
        let bounds = opened[kCGWindowBounds as String] as? [String: AnyObject]
    else {
        die("the \(args[2]) popup drew no menu; last: \(lastError)")
    }
    let x = (bounds["X"] as? NSNumber)?.doubleValue ?? 0
    let y = (bounds["Y"] as? NSNumber)?.doubleValue ?? 0
    let w = (bounds["Width"] as? NSNumber)?.doubleValue ?? 0
    let h = (bounds["Height"] as? NSNumber)?.doubleValue ?? 0
    let layer = (opened[kCGWindowLayer as String] as? Int) ?? 0
    printRect(CGRect(x: x, y: y, width: w, height: h), extra: "layer=\(layer)")

case "type-select":
    // Menu already open. WebKit publishes no AXMenu, so the option is typed.
    guard args.count >= 4 else { die("usage: ax-settings type-select <pid> <label> <option>") }
    guard menuWindowCount() > 0 else { die("no menu is open") }
    typeSelect(args[3])
    guard
        waitFor(
            10,
            {
                guard let fresh = popup(labelled: args[2]),
                    string(fresh, kAXValueAttribute) == args[3]
                else { return nil }
                return fresh
            }) != nil
    else {
        die(
            "the \(args[2]) popup still reads "
                + "\(popup(labelled: args[2]).flatMap { string($0, kAXValueAttribute) } ?? "nothing") "
                + "after picking \(args[3])")
    }

case "move":
    guard args.count >= 4, let x = Double(args[2]), let y = Double(args[3]) else {
        die("usage: ax-settings move <pid> <x> <y>")
    }
    guard let window = settingsWindow() else { die("Settings is not open") }
    var origin = CGPoint(x: x, y: y)
    guard let value = AXValueCreate(.cgPoint, &origin),
        AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, value) == .success
    else {
        die("could not move Settings")
    }

case "size":
    guard args.count >= 5, let w = Double(args[3]), let h = Double(args[4]) else {
        die("usage: ax-settings size <pid> <window title> <width> <height>")
    }
    guard let window = window(titled: args[2]) else { die("\(args[2]) is not open") }
    var size = CGSize(width: w, height: h)
    guard let value = AXValueCreate(.cgSize, &size),
        AXUIElementSetAttributeValue(window, kAXSizeAttribute as CFString, value) == .success,
        let rect = frame(window)
    else {
        die("could not resize \(args[2])")
    }
    printRect(rect)

case "minimize":
    guard args.count >= 4, let on = Bool(args[3]) else {
        die("usage: ax-settings minimize <pid> <window title> <true|false>")
    }
    guard let window = window(titled: args[2]) else { die("\(args[2]) is not open") }
    guard
        AXUIElementSetAttributeValue(window, kAXMinimizedAttribute as CFString, on as CFBoolean)
            == .success
    else {
        die("could not set \(args[2]) minimized to \(on)")
    }

case "frame":
    // For `screencapture -R`, so the still is the window and not the desktop.
    guard let window = settingsWindow(), let rect = frame(window) else {
        die("Settings is not open")
    }
    printRect(rect)

case "key":
    guard args.count >= 3 else {
        die("usage: ax-settings key <pid> <tab|space|return|escape|down|up|delete|end>")
    }
    let codes: [String: (CGKeyCode, CGEventFlags)] = [
        "tab": (48, []), "space": (49, []), "return": (36, []), "escape": (53, []),
        "down": (125, []), "up": (126, []), "delete": (51, []), "end": (119, []),
    ]
    guard let pair = codes[args[2]] else { die("unknown key \(args[2])") }
    postKey(pair.0, flags: pair.1)

case "type":
    guard args.count >= 3 else { die("usage: ax-settings type <pid> <text>") }
    typeText(args[2])

case "value":
    // The focused element's text in any app, read through Accessibility: no
    // Apple event, so it needs no Automation grant and can't wait on a busy app.
    var valueRef: CFTypeRef?
    let valueErr = AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute as CFString, &valueRef)
    guard valueErr == .success, let element = valueRef, CFGetTypeID(element) == AXUIElementGetTypeID()
    else {
        die("no focused element")
    }
    guard let text = string(element as! AXUIElement, kAXValueAttribute) else {
        die("the focused element has no text value")
    }
    print(text)

case "focused":
    // The control Tab last landed on: dump's columns minus placeholder, settable and focusable.
    guard let window = settledWindow(titled: "Settings") else { die("Settings is not open") }
    var focusedRef: CFTypeRef?
    let err = AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute as CFString, &focusedRef)
    guard err == .success, let focused = focusedRef, CFGetTypeID(focused) == AXUIElementGetTypeID()
    else {
        die("no focused element")
    }
    _ = window
    print(dumpOne(focused as! AXUIElement))

case "focus-window":
    // Click the title bar, not a control, so the webview is key without
    // changing a row.
    guard let window = settingsWindow(), let rect = frame(window) else {
        die("Settings is not open")
    }
    activateSettings()
    let point = CGPoint(x: rect.midX, y: rect.minY + 12)
    guard
        let down = CGEvent(
            mouseEventSource: nil, mouseType: .leftMouseDown, mouseCursorPosition: point,
            mouseButton: .left),
        let up = CGEvent(
            mouseEventSource: nil, mouseType: .leftMouseUp, mouseCursorPosition: point,
            mouseButton: .left)
    else { die("could not click the title bar") }
    down.post(tap: .cghidEventTap)
    usleep(60_000)
    up.post(tap: .cghidEventTap)
    usleep(150_000)

case "menus":
    print(menuWindowCount())

case "press-button":
    // Tree order, 1-based among buttons that share a title. The BYO section
    // draws two Copy buttons; the snippet is first and the Hermes token is
    // second when that row is showing.
    guard args.count >= 3 else {
        die("usage: ax-settings press-button <pid> <title> [index] [window title]")
    }
    let want = args[2]
    let index = args.count >= 4 ? (Int(args[3]) ?? 1) : 1
    guard index >= 1 else { die("button index must be >= 1") }
    let windowTitle = args.count >= 5 ? args[4] : "Settings"
    guard let window = settledWindow(titled: windowTitle) else { die("\(windowTitle) is not open") }
    var matches: [AXUIElement] = []
    func collect(_ element: AXUIElement, depth: Int) {
        guard depth < 30 else { return }
        if string(element, kAXRoleAttribute) == "AXButton",
            string(element, kAXTitleAttribute) == want
        {
            matches.append(element)
        }
        for child in children(element) { collect(child, depth: depth + 1) }
    }
    collect(window, depth: 0)
    guard index <= matches.count else {
        die("no AXButton titled \(want) at index \(index) (\(matches.count) found)")
    }
    let target = matches[index - 1]
    activateSettings()
    guard press(target) else { die("could not press \(want) #\(index); last: \(lastError)") }
    if let rect = frame(target) { printRect(rect) }

case "dump":
    // One line per element as role|title|value|placeholder|enabled|settable|
    // focusable, in tree order, which is render order, so section order is
    // assertable.
    let wanted = args.count >= 3 ? args[2] : "Settings"
    // `frames` appends each element's x,y,w,h, for a layout assertion.
    let withFrames = args.count >= 4 && args[3] == "frames"
    guard settledWindow(titled: wanted) != nil else { die("\(wanted) is not open") }
    func walk(_ element: AXUIElement, depth: Int) {
        guard depth < 30 else { return }
        let role = string(element, kAXRoleAttribute) ?? "?"
        // The subrole is what separates the API key row from the rows beside
        // it: a secure field reports AXSecureTextField and an empty value.
        let subrole = string(element, kAXSubroleAttribute) ?? ""
        let title = string(element, kAXTitleAttribute) ?? ""
        let value = attribute(element, kAXValueAttribute).map { "\($0)" } ?? ""
        // A frozen field is often empty and showing its placeholder, so the
        // placeholder is the only text that says which row this is.
        let placeholder = string(element, kAXPlaceholderValueAttribute) ?? ""
        let enabled = (attribute(element, kAXEnabledAttribute) as? Bool).map(String.init) ?? ""
        // Whether the value can be written is the only signal a frozen text
        // field gives: the renderer freezes with setEditable(false), which
        // leaves AXEnabled true and the field looking exactly like a live one.
        var settableFlag: DarwinBoolean = false
        let settable =
            AXUIElementIsAttributeSettable(element, kAXValueAttribute as CFString, &settableFlag)
            == .success ? String(settableFlag.boolValue) : ""
        // WebKit reports a <pre tabindex="0"> as a plain AXGroup, like its layout
        // wrappers; only a settable AXFocused says Tab can reach it.
        var focusableFlag: DarwinBoolean = false
        let focusable =
            AXUIElementIsAttributeSettable(element, kAXFocusedAttribute as CFString, &focusableFlag)
            == .success ? String(focusableFlag.boolValue) : ""
        // A trailing newline inside a value would break the one-line-per-row
        // contract the shell greps against.
        let flat = { (s: String) in s.replacingOccurrences(of: "\n", with: "\\n") }
        print(
            "\(role)\(subrole.isEmpty ? "" : ":" + subrole)|\(flat(title))|\(flat(value))"
                + "|\(flat(placeholder))|\(enabled)|\(settable)|\(focusable)"
                + (withFrames ? "|" + (frame(element).map(rectText) ?? "") : ""))
        for child in children(element) { walk(child, depth: depth + 1) }
    }
    // Every window with the title: each display's overlay and the quick-message
    // pill are all titled Fidget.
    for window in attribute(app, kAXWindowsAttribute) as? [AXUIElement] ?? []
    where string(window, kAXTitleAttribute) == wanted {
        walk(window, depth: 0)
    }

default:
    die("unknown command \(args[0])")
}
