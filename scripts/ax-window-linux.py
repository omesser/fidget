#!/usr/bin/env python3
"""Dump or resize one Fidget window through AT-SPI.

Dump lines are `role|name`. Pass `frames` after the title to append
`|x,y,w,h` on every line, matching the shape chat-header-narrow asserts.
`size` resizes the window and prints its frame as `x,y,w,h`.

Usage:
  ax-window-linux.py dump PID TITLE [frames]
  ax-window-linux.py size PID TITLE WIDTH HEIGHT

Exit 2 when pyatspi is missing. Exit 1 when that window is not in the tree
or a resize fails.
"""

import sys

ROLE = {
    "push button": "button",
    "toggle button": "button",
    "frame": "frame",
    "label": "label",
    "static text": "label",
    "link": "link",
    "scroll pane": "scroll-area",
    "document web": "web-area",
    "html container": "web-area",
    "document": "web-area",
}


def short_role(acc):
    try:
        raw = acc.getRoleName() or "unknown"
    except Exception:
        raw = "unknown"
    return ROLE.get(raw, raw.replace(" ", "-"))


def node_name(acc):
    try:
        return (acc.name or "").replace("\n", "\\n")
    except Exception:
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


def find_window(desktop, pid, title):
    title = title.lower()
    for i in range(desktop.childCount):
        app = desktop.getChildAtIndex(i)
        try:
            if app.get_process_id() != pid:
                continue
        except Exception:
            continue
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
    try:
        component = window.queryComponent()
        box = component.getExtents(pyatspi.DESKTOP_COORDS)
        component.setExtents(
            int(box.x),
            int(box.y),
            width,
            height,
            pyatspi.DESKTOP_COORDS,
        )
    except Exception as err:
        print(f"could not resize {title}: {err}", file=sys.stderr)
        return 1
    after = extents(window)
    if after is None:
        print(f"could not read frame for {title}", file=sys.stderr)
        return 1
    print(f"{after[0]},{after[1]},{after[2]},{after[3]}")
    return 0


def main(argv):
    if len(argv) < 2:
        print(
            "usage: ax-window-linux.py dump|size PID TITLE ...",
            file=sys.stderr,
        )
        return 2
    if argv[1] == "dump":
        return cmd_dump(argv)
    if argv[1] == "size":
        return cmd_size(argv)
    print(
        "usage: ax-window-linux.py dump|size PID TITLE ...",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
