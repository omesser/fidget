#!/usr/bin/env python3
"""Dump or resize one Fidget window through AT-SPI.

Dump lines are `role|name`. Pass `frames` after the title to append
`|x,y,w,h` on every line, matching the shape chat-header-narrow asserts.
`size` resizes the window and prints its frame as `x,y,w,h`.
`press` performs the first action of the first button named NAME.
`tray` clicks the tray menu row whose label starts with ROW, through the
StatusNotifierItem's dbusmenu, then waits for a window titled TITLE.
`menu` right-clicks the screen point X,Y through xdotool, prints the names
of the menu items PID then shows, one per line, and presses Escape.

Usage:
  ax-window-linux.py dump PID TITLE [frames]
  ax-window-linux.py size PID TITLE WIDTH HEIGHT
  ax-window-linux.py press PID TITLE NAME
  ax-window-linux.py tray PID TITLE ROW
  ax-window-linux.py menu PID X Y

Exit 2 when pyatspi is missing, or for `tray` when no StatusNotifierWatcher
runs. Exit 1 when that window is not in the tree, no menu shows, or an
action fails.
"""

import sys

ROLE = {
    "push button": "button",
    "toggle button": "button",
    "frame": "frame",
    "label": "label",
    # WebKitGTK names <time> "static"; macOS AX said "static text".
    "static text": "label",
    "static": "label",
    "link": "link",
    "scroll pane": "scroll-area",
    "document web": "web-area",
    "html container": "web-area",
    "document": "web-area",
}


def x11_client_frame(pid, title):
    """Largest X11 client rect for pid whose name contains title.

    AT-SPI getExtents on the frame includes the WM decoration. chat-header-narrow
    wants the same content width macOS reports, which is the X11 client size.
    """
    import subprocess

    best = None
    try:
        wids = subprocess.check_output(
            ["xdotool", "search", "--pid", str(pid)], text=True
        ).splitlines()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    for wid in wids:
        wid = wid.strip()
        if not wid:
            continue
        try:
            name = subprocess.check_output(
                ["xdotool", "getwindowname", wid], text=True
            ).strip()
        except subprocess.CalledProcessError:
            continue
        if title.lower() not in name.lower():
            continue
        try:
            geom = subprocess.check_output(
                ["xdotool", "getwindowgeometry", "--shell", wid], text=True
            )
        except subprocess.CalledProcessError:
            continue
        vals = dict(part.split("=", 1) for part in geom.splitlines() if "=" in part)
        try:
            x, y = int(vals["X"]), int(vals["Y"])
            w, h = int(vals["WIDTH"]), int(vals["HEIGHT"])
        except (KeyError, ValueError):
            continue
        if w < 100 or h < 100:
            continue
        if best is None or w * h > best[2] * best[3]:
            best = (wid, x, y, w, h)
    return best



def short_role(acc):
    try:
        raw = acc.getRoleName() or "unknown"
    except Exception:
        raw = "unknown"
    return ROLE.get(raw, raw.replace(" ", "-"))


def node_name(acc):
    try:
        name = (acc.name or "").replace("\n", "\\n")
    except Exception:
        name = ""
    if name:
        return name
    # WebKitGTK often leaves <time> AccessibleName empty; the Text
    # interface still holds what chat-header-narrow asserts on.
    try:
        text = acc.queryText()
        name = text.getText(0, text.characterCount).replace("\n", "\\n")
    except Exception:
        name = ""
    if name:
        return name
    # An empty <textarea> has no name or text; its placeholder is what the
    # macOS dump shows for the composer.
    try:
        for attr in acc.getAttributes():
            if attr.startswith("placeholder-text:"):
                return attr.split(":", 1)[1]
    except Exception:
        pass
    return ""


def extents(acc):
    try:
        import pyatspi

        component = acc.queryComponent()
    except Exception:
        return None
    try:
        e = component.getExtents(pyatspi.DESKTOP_COORDS)
        return int(e.x), int(e.y), int(e.width), int(e.height)
    except Exception:
        return None


def dump(acc, with_frames, depth=0):
    if depth > 30:
        return
    role = short_role(acc)
    name = node_name(acc)
    line = f"{role}|{name}"
    if with_frames:
        box = extents(acc)
        line += "|" + (f"{box[0]},{box[1]},{box[2]},{box[3]}" if box else "")
    print(line)
    try:
        count = acc.childCount
    except Exception:
        return
    for i in range(count):
        try:
            dump(acc.getChildAtIndex(i), with_frames, depth + 1)
        except Exception:
            pass


def find_app(desktop, pid):
    for i in range(desktop.childCount):
        app = desktop.getChildAtIndex(i)
        try:
            if app.get_process_id() == pid:
                return app
        except Exception:
            continue
    return None


def find_window(desktop, pid, title):
    title = title.lower()
    app = find_app(desktop, pid)
    if app is None:
        return None
    for j in range(app.childCount):
        win = app.getChildAtIndex(j)
        try:
            name = win.name or ""
        except Exception:
            name = ""
        if title in name.lower():
            return win
    return None


def cmd_dump(argv):
    if len(argv) not in (4, 5) or (len(argv) == 5 and argv[4] != "frames"):
        print(
            "usage: ax-window-linux.py dump PID TITLE [frames]",
            file=sys.stderr,
        )
        return 2
    pid = int(argv[2])
    title = argv[3]
    with_frames = len(argv) == 5
    try:
        import pyatspi
    except ImportError:
        print("pyatspi is not installed", file=sys.stderr)
        return 2
    desktop = pyatspi.Registry.getDesktop(0)
    window = find_window(desktop, pid, title)
    if window is None:
        print(f"no window titled {title} for pid {pid}", file=sys.stderr)
        return 1
    client = x11_client_frame(pid, title) if with_frames else None
    if with_frames and client is not None:
        # Emit the window line with the X11 client frame, then its AT-SPI children.
        role = short_role(window)
        name = node_name(window)
        _, x, y, w, h = client
        print(f"{role}|{name}|{x},{y},{w},{h}")
        try:
            count = window.childCount
        except Exception:
            return 0
        for i in range(count):
            try:
                dump(window.getChildAtIndex(i), with_frames, 1)
            except Exception:
                pass
        return 0
    dump(window, with_frames)
    return 0


def cmd_size(argv):
    if len(argv) != 6:
        print(
            "usage: ax-window-linux.py size PID TITLE WIDTH HEIGHT",
            file=sys.stderr,
        )
        return 2
    pid = int(argv[2])
    title = argv[3]
    width = int(argv[4])
    height = int(argv[5])
    try:
        import pyatspi
    except ImportError:
        print("pyatspi is not installed", file=sys.stderr)
        return 2
    desktop = pyatspi.Registry.getDesktop(0)
    window = find_window(desktop, pid, title)
    if window is None:
        print(f"no window titled {title} for pid {pid}", file=sys.stderr)
        return 1
    client = x11_client_frame(pid, title)
    if client is None:
        print(f"no X11 window titled {title} for pid {pid}", file=sys.stderr)
        return 1
    wid, x, y, _, _ = client
    try:
        import subprocess
        import time

        subprocess.check_call(
            ["xdotool", "windowsize", wid, str(width), str(height)]
        )
        for _ in range(20):
            time.sleep(0.05)
            after = x11_client_frame(pid, title)
            if after and after[3] == width and after[4] == height:
                break
    except Exception as err:
        print(f"could not resize {title}: {err}", file=sys.stderr)
        return 1
    after = x11_client_frame(pid, title)
    if after is None:
        print(f"could not read frame for {title}", file=sys.stderr)
        return 1
    print(f"{after[1]},{after[2]},{after[3]},{after[4]}")
    return 0


def find_buttons(acc, name, found, depth=0):
    if depth > 30:
        return
    if short_role(acc) == "button" and node_name(acc) == name:
        found.append(acc)
    try:
        count = acc.childCount
    except Exception:
        return
    for i in range(count):
        try:
            find_buttons(acc.getChildAtIndex(i), name, found, depth + 1)
        except Exception:
            pass


def cmd_press(argv):
    if len(argv) != 5:
        print("usage: ax-window-linux.py press PID TITLE NAME", file=sys.stderr)
        return 2
    pid, title, name = int(argv[2]), argv[3], argv[4]
    try:
        import pyatspi
    except ImportError:
        print("pyatspi is not installed", file=sys.stderr)
        return 2
    window = find_window(pyatspi.Registry.getDesktop(0), pid, title)
    if window is None:
        print(f"no window titled {title} for pid {pid}", file=sys.stderr)
        return 1
    found = []
    find_buttons(window, name, found)
    if not found:
        print(f"no button {name}", file=sys.stderr)
        return 1
    try:
        if not found[0].queryAction().doAction(0):
            raise RuntimeError("doAction returned false")
    except Exception as err:
        print(f"could not press {name}: {err}", file=sys.stderr)
        return 1
    return 0


MENU_ITEM_ROLES = {"menu item", "check menu item", "radio menu item"}


def showing_menu_items(acc, found, depth=0):
    """Names of menu items on screen. Only showing subtrees are walked: the
    tray's GtkMenu lives in the same process with the same rows, hidden."""
    import pyatspi

    if depth > 30:
        return
    try:
        if not acc.getState().contains(pyatspi.STATE_SHOWING):
            return
        role = acc.getRoleName()
    except Exception:
        return
    if role in MENU_ITEM_ROLES:
        name = node_name(acc)
        if name:
            found.append(name)
        return
    try:
        count = acc.childCount
    except Exception:
        return
    for i in range(count):
        try:
            showing_menu_items(acc.getChildAtIndex(i), found, depth + 1)
        except Exception:
            pass


def cmd_menu(argv):
    if len(argv) != 5:
        print("usage: ax-window-linux.py menu PID X Y", file=sys.stderr)
        return 2
    pid, x, y = int(argv[2]), argv[3], argv[4]
    try:
        import pyatspi
    except ImportError:
        print("pyatspi is not installed", file=sys.stderr)
        return 2
    import subprocess
    import time

    subprocess.check_call(["xdotool", "mousemove", "--sync", x, y, "click", "3"])
    items = []
    for _ in range(20):
        time.sleep(0.25)
        app = find_app(pyatspi.Registry.getDesktop(0), pid)
        if app is None:
            continue
        items = []
        for j in range(app.childCount):
            showing_menu_items(app.getChildAtIndex(j), items)
        if items:
            break
    subprocess.call(["xdotool", "key", "Escape"])
    if not items:
        print(f"pid {pid} showed no menu within 5s", file=sys.stderr)
        return 1
    print("\n".join(items))
    return 0


def dbusmenu_find(layout, row):
    item_id, props, children = layout
    if props.get("label", "").replace("_", "").startswith(row):
        return item_id
    for child in children:
        found = dbusmenu_find(child, row)
        if found is not None:
            return found
    return None


def cmd_tray(argv):
    """libappindicator exports the tray menu over dbusmenu, so a row is
    clicked there, the same menu the panel draws, with no pointer."""
    if len(argv) != 5:
        print("usage: ax-window-linux.py tray PID TITLE ROW", file=sys.stderr)
        return 2
    pid, title, row = int(argv[2]), argv[3], argv[4]
    import time

    from gi.repository import Gio, GLib

    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def call(name, path, iface, method, args, reply):
        return bus.call_sync(
            name, path, iface, method, args, GLib.VariantType(reply),
            Gio.DBusCallFlags.NONE, 5000, None,
        ).unpack()

    def prop(name, path, iface, key):
        return call(
            name, path, "org.freedesktop.DBus.Properties", "Get",
            GLib.Variant("(ss)", (iface, key)), "(v)",
        )[0]

    item = None
    for _ in range(80):
        try:
            entries = prop(
                "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher",
                "org.kde.StatusNotifierWatcher", "RegisteredStatusNotifierItems",
            )
        except GLib.Error as err:
            print(f"no StatusNotifierWatcher, so no tray: {err.message}", file=sys.stderr)
            return 2
        for entry in entries:
            service, _, path = entry.partition("/")
            path = "/" + path if path else "/StatusNotifierItem"
            try:
                owner = call(
                    "org.freedesktop.DBus", "/org/freedesktop/DBus",
                    "org.freedesktop.DBus", "GetConnectionUnixProcessID",
                    GLib.Variant("(s)", (service,)), "(u)",
                )[0]
            except GLib.Error:
                continue
            if owner == pid:
                item = (service, path)
        if item:
            break
        time.sleep(0.25)
    if item is None:
        print(f"pid {pid} registered no tray item in 20s", file=sys.stderr)
        return 1
    menu = prop(item[0], item[1], "org.kde.StatusNotifierItem", "Menu")
    _, layout = call(
        item[0], menu, "com.canonical.dbusmenu", "GetLayout",
        GLib.Variant("(iias)", (0, -1, ["label"])), "(u(ia{sv}av))",
    )
    row_id = dbusmenu_find(layout, row)
    if row_id is None:
        print(f"the tray menu has no {row} row", file=sys.stderr)
        return 1
    call(
        item[0], menu, "com.canonical.dbusmenu", "Event",
        GLib.Variant("(isvu)", (row_id, "clicked", GLib.Variant("i", 0), 0)), "()",
    )
    import pyatspi

    for _ in range(40):
        if find_window(pyatspi.Registry.getDesktop(0), pid, title) is not None:
            return 0
        time.sleep(0.25)
    print(f"{title} did not open within 10s", file=sys.stderr)
    return 1


COMMANDS = {
    "dump": cmd_dump,
    "size": cmd_size,
    "press": cmd_press,
    "tray": cmd_tray,
    "menu": cmd_menu,
}


def main(argv):
    if len(argv) < 2:
        print(
            "usage: ax-window-linux.py dump|size|press|tray|menu PID TITLE ...",
            file=sys.stderr,
        )
        return 2
    if argv[1] in COMMANDS:
        return COMMANDS[argv[1]](argv)
    print(
        "usage: ax-window-linux.py dump|size|press|tray|menu PID TITLE ...",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
