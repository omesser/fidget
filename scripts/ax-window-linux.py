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
        return text.getText(0, text.characterCount).replace("\n", "\\n")
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
