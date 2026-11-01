#!/usr/bin/env python3
"""Wait for a Fidget window's AT-SPI tree to match a pattern.

Checks once, then again on each AT-SPI change event from PID (children,
names, text, states, new windows), and exits 0 on the first match or 3 when
SECONDS pass; 3, not 1, so a crash (Python exits 1) never reads as a
timeout. Nothing polls: the GLib main loop sleeps until an event.

Usage:
  ax-wait-linux.py PID TITLE SECONDS PATTERN [OUT]

With OUT, the dump that matched is written there.

Exit 2 when pyatspi is missing.
"""

import contextlib
import importlib.util
import io
import os
import re
import sys

EVENTS = (
    "object:children-changed",
    "object:property-change:accessible-name",
    "object:text-changed",
    "object:state-changed",
    "object:bounds-changed",
    "window:create",
    "window:activate",
)


def load_dumper():
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ax-window-linux.py")
    spec = importlib.util.spec_from_file_location("ax_window_linux", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main(argv):
    if len(argv) not in (5, 6):
        print(__doc__, file=sys.stderr)
        return 2
    pid, title, seconds, pattern = int(argv[1]), argv[2], float(argv[3]), re.compile(argv[4])
    try:
        import pyatspi
    except ImportError:
        print("pyatspi is not installed", file=sys.stderr)
        return 2
    ax = load_dumper()

    def matches():
        desktop = pyatspi.Registry.getDesktop(0)
        window = ax.find_window(desktop, pid, title)
        if window is None:
            return False
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            ax.dump(window, False)
        if not any(pattern.search(line) for line in out.getvalue().splitlines()):
            return False
        if len(argv) == 6:
            with open(argv[5], "w") as f:
                f.write(out.getvalue())
        return True

    return 0 if ax.wait_until(pid, matches, seconds, EVENTS) else 3


if __name__ == "__main__":
    sys.exit(main(sys.argv))
