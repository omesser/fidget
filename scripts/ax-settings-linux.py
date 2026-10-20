#!/usr/bin/env python3
"""Drives and reads the Linux Settings window through AT-SPI.

Why this exists: every question the verification actually asks - is this row
frozen, do the sections come in this order, does this label say "AI brain" -
is a string or a boolean the AT-SPI tree already holds. Reading it turns a
ten-minute click-and-look session into a second of text, and the assertions
become greppable instead of visual.

Why not other UI automation: AT-SPI is the native Linux accessibility protocol
for GTK and WebKitGTK. Other tools either don't work with GTK3 or require
extensive setup.

Usage:
    ax-settings-linux.py wait [--timeout SECONDS]
    ax-settings-linux.py dump
    ax-settings-linux.py pick-source TITLE

Needs AT-SPI accessibility infrastructure (atspi2, pyatspi).
"""

import sys
import os
import time
import argparse


def has_ai_tab(acc, depth=0):
    """Check if the window has an AI page tab (identifies Settings notebook)."""
    if depth > 6:
        return False
    try:
        if acc.getRoleName() == "page tab" and (acc.name or "") == "AI":
            return True
        for i in range(acc.childCount):
            if has_ai_tab(acc.getChildAtIndex(i), depth + 1):
                return True
    except Exception:
        return False
    return False


def find_settings_window():
    """Find the Settings window via AT-SPI. Returns the window element or None."""
    try:
        import pyatspi
        desktop = pyatspi.Registry.getDesktop(0)
        for app_idx in range(desktop.childCount):
            app = desktop.getChildAtIndex(app_idx)
            if "fidget" not in (app.name or "").lower():
                continue
            for win_idx in range(app.childCount):
                win = app.getChildAtIndex(win_idx)
                # Overlay frames carry the same title; the AI tab marks Settings.
                if has_ai_tab(win):
                    return win
        return None
    except Exception as e:
        print(f"AT-SPI error: {e}", file=sys.stderr)
        return None


def cmd_wait(args):
    """Wait for Settings window to appear with AI tab."""
    waited = 0
    while waited < args.timeout:
        win = find_settings_window()
        if win is not None:
            return 0
        time.sleep(1)
        waited += 1
    return 1


def dump_accessible(acc, depth=0):
    """Recursively dump the accessible tree."""
    indent = "  " * depth
    role = acc.getRoleName() if hasattr(acc, "getRoleName") else "unknown"
    name = acc.name if hasattr(acc, "name") else ""

    try:
        import pyatspi
        states = []
        if hasattr(acc, "getState"):
            state_set = acc.getState()
            if hasattr(state_set, "contains"):
                if state_set.contains(pyatspi.STATE_EDITABLE):
                    states.append("editable")
                if state_set.contains(pyatspi.STATE_SENSITIVE):
                    states.append("sensitive")
                if not state_set.contains(pyatspi.STATE_SENSITIVE):
                    states.append("insensitive")

        state_str = f"[{','.join(states)}]" if states else ""
        print(f"{indent}{role}|{name}|{state_str}")

        if hasattr(acc, "childCount"):
            for i in range(acc.childCount):
                try:
                    child = acc.getChildAtIndex(i)
                    dump_accessible(child, depth + 1)
                except Exception:
                    pass
    except Exception:
        pass


def cmd_dump(args):
    """Dump the Settings window accessible tree."""
    settings = find_settings_window()
    if settings is None:
        print("Settings window not found", file=sys.stderr)
        return 1

    try:
        dump_accessible(settings)
        return 0
    except Exception as e:
        print(f"AT-SPI error: {e}", file=sys.stderr)
        return 1


def walk(acc, depth=0):
    """Generator to walk the accessible tree."""
    if depth > 20:
        return
    yield acc
    try:
        for i in range(acc.childCount):
            try:
                yield from walk(acc.getChildAtIndex(i), depth + 1)
            except Exception:
                pass
    except Exception:
        pass


def menu_titles(combo):
    """Get all menu item titles from a combo box."""
    titles = []
    for node in walk(combo, 0):
        try:
            if node.getRoleName() == "menu item":
                titles.append(node.name or "")
        except Exception:
            pass
    return titles


def find_source_combo(win):
    """Find the AI source combo box.

    The AI source combo's accessible name is the current selection (e.g. "Model
    API"), not the "AI source" label beside it. Identify it by its menu items.
    """
    for node in walk(win):
        try:
            if "combo" not in (node.getRoleName() or "").lower():
                continue
        except Exception:
            continue
        titles = menu_titles(node)
        if "Model API" in titles and any(t.startswith("Harness") for t in titles):
            return node
    return None


def find_menu_item(combo, want):
    """Find a menu item by title."""
    for node in walk(combo):
        try:
            if node.getRoleName() == "menu item" and (node.name or "") == want:
                return node
        except Exception:
            pass
    return None


def cmd_pick_source(args):
    """Pick an AI source from the combo box."""
    settings = find_settings_window()
    if settings is None:
        print("Settings window not found", file=sys.stderr)
        return 1

    combo = find_source_combo(settings)
    if combo is None:
        print("AI source combo not found", file=sys.stderr)
        return 1

    item = find_menu_item(combo, args.title)
    if item is None:
        print(f"menu item not found: {args.title!r}", file=sys.stderr)
        return 1

    acted = False
    try:
        act = item.queryAction()
        if act.nActions > 0:
            act.doAction(0)
            acted = True
    except Exception:
        pass

    if not acted:
        try:
            act = combo.queryAction()
            if act.nActions > 0:
                act.doAction(0)
                time.sleep(0.3)
                item = find_menu_item(combo, args.title)
                if item is not None:
                    item.queryAction().doAction(0)
                    acted = True
        except Exception as e:
            print(f"AT-SPI action error: {e}", file=sys.stderr)
            return 1

    if not acted:
        print("could not activate menu item", file=sys.stderr)
        return 1

    time.sleep(0.5)
    return 0


def main():
    parser = argparse.ArgumentParser(
        description='Drive and read the Linux Settings window through AT-SPI',
        formatter_class=argparse.RawDescriptionHelpFormatter
    )
    subparsers = parser.add_subparsers(dest='command', required=True)

    wait_parser = subparsers.add_parser('wait', help='Wait for Settings window')
    wait_parser.add_argument('--timeout', type=int, default=30,
                             help='Timeout in seconds (default: 30)')
    wait_parser.set_defaults(func=cmd_wait)

    dump_parser = subparsers.add_parser('dump', help='Dump Settings window tree')
    dump_parser.set_defaults(func=cmd_dump)

    pick_parser = subparsers.add_parser('pick-source', help='Pick AI source')
    pick_parser.add_argument('title', help='AI source title to pick')
    pick_parser.set_defaults(func=cmd_pick_source)

    args = parser.parse_args()
    sys.exit(args.func(args))


if __name__ == '__main__':
    main()
