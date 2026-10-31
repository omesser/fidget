#!/usr/bin/env python3
"""Capture the real Fidget GTK/WebKit UI under the GitHub-hosted X11 desktop.

No HTML mockups, screenshots of a local browser, or images generated from strings.
The app and its language setting are driven through real desktop input.
"""
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time

import pyatspi
from PIL import Image, ImageStat

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs/i18n/gallery"
TABS = ("Presence", "Character", "AI", "Chat", "Privacy", "Caffeinate", "Development")
LANGS = ("en", "fr", "es")
STRINGS = {"en": {}, **{
    lang: json.loads((ROOT / f"src/locales/{lang}.json").read_text(encoding="utf-8"))
    for lang in ("fr", "es")
}}
NAMES = {"en": "English", "fr": "Français", "es": "Español"}


def run(*args, check=True):
    return subprocess.run([str(a) for a in args], check=check, text=True, capture_output=True)


def eventually(check, why, seconds=20):
    deadline = time.monotonic() + seconds
    last_error = None
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except Exception as exc:
            last_error = exc
        time.sleep(0.25)
    raise RuntimeError(f"{why}; last error: {last_error}")


def xwindows(pid):
    found = run("xdotool", "search", "--pid", pid, check=False)
    if found.returncode:
        return []
    windows = []
    for wid in found.stdout.splitlines():
        title = run("xdotool", "getwindowname", wid, check=False)
        if not title.returncode:
            windows.append((wid, title.stdout.strip()))
    return windows


def find_window(pid, word):
    matches = [(wid, name) for wid, name in xwindows(pid) if word.lower() in name.lower()]
    if not matches:
        return None
    matches.sort(key=lambda entry: geometry(entry[0])[2] * geometry(entry[0])[3], reverse=True)
    return matches[0][0]


def geometry(wid):
    output = run("xdotool", "getwindowgeometry", "--shell", wid).stdout
    fields = dict(part.split("=", 1) for part in output.splitlines() if "=" in part)
    return tuple(int(fields[k]) for k in ("X", "Y", "WIDTH", "HEIGHT"))


def raise_window(wid):
    run("xdotool", "windowactivate", "--sync", wid)
    run("xdotool", "windowraise", wid)
    time.sleep(0.18)


def app_accessible(pid):
    desktop = pyatspi.Registry.getDesktop(0)
    for i in range(desktop.childCount):
        app = desktop.getChildAtIndex(i)
        try:
            if app.get_process_id() == pid:
                return app
        except Exception:
            pass
    return None


def walk(node, depth=0):
    if depth > 25:
        return
    yield node
    try:
        count = node.childCount
    except Exception:
        return
    for i in range(count):
        try:
            yield from walk(node.getChildAtIndex(i), depth + 1)
        except Exception:
            pass


def accessible_name(node):
    try:
        return node.name or ""
    except Exception:
        return ""


def accessible_role(node):
    try:
        return node.getRoleName().lower()
    except Exception:
        return ""


def center(node):
    ext = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    if ext.width < 3 or ext.height < 3:
        raise RuntimeError(f"bad accessibility bounds: {ext}")
    return ext.x + ext.width // 2, ext.y + ext.height // 2


def click_node(node):
    x, y = center(node)
    run("xdotool", "mousemove", "--sync", x, y, "click", "--clearmodifiers", "1")


def localized(lang, word):
    return STRINGS[lang].get(word, word)


def locate_tab(pid, lang, tab):
    app = app_accessible(pid)
    if app is None:
        return None
    expected = localized(lang, tab)
    for node in walk(app):
        if accessible_name(node) == expected and accessible_role(node) in (
            "page tab", "push button", "toggle button", "radio button", "button"
        ):
            return node
    return None


def locate_language_picker(pid):
    app = app_accessible(pid)
    if app is None:
        return None
    # Presence contains one select: the Language setting. Other sections'
    # selects are on other tabs and not in the rendered accessibility tree.
    for node in walk(app):
        if "combo box" in accessible_role(node):
            try:
                x, y = center(node)
                if x >= 0 and y >= 0:
                    return node
            except Exception:
                pass
    return None


def capture(name, wid=None, popup=False):
    """Capture the actual root framebuffer, including native popup windows."""
    screen_file = Path(tempfile.mkstemp(suffix=".png")[1])
    try:
        run("import", "-window", "root", screen_file)
        image = Image.open(screen_file).convert("RGB")
        if wid is not None:
            x, y, w, h = geometry(wid)
            padding = 10
            extra = 170 if popup else 0
            left = max(0, x - padding)
            top = max(0, y - padding)
            right = min(image.width, x + w + padding + extra)
            bottom = min(image.height, y + h + padding + extra)
            if right - left < 300 or bottom - top < 250:
                raise RuntimeError(f"tiny window crop: {name}: {w}x{h}")
            image = image.crop((left, top, right, bottom))
        stats = ImageStat.Stat(image)
        if max(stats.stddev) < 3:
            raise RuntimeError(f"blank/flat screenshot: {name}")
        path = OUT / name
        path.parent.mkdir(parents=True, exist_ok=True)
        image.save(path, format="PNG", optimize=True)
        print(f"CAPTURE {path.relative_to(ROOT)} ({image.width}x{image.height})", flush=True)
    finally:
        screen_file.unlink(missing_ok=True)


def settings_language(home):
    path = home / ".local/share/fidget/settings.json"
    try:
        return json.loads(path.read_text(encoding="utf-8")).get("language")
    except (OSError, json.JSONDecodeError):
        return None


def set_language(pid, wid, home, lang):
    raise_window(wid)
    tab = eventually(lambda: locate_tab(pid, NAMES_BY_SETTING[0], "Presence"),
                     "Presence tab absent before switching")
    click_node(tab)
    picker = eventually(lambda: locate_language_picker(pid), "language picker absent")
    click_node(picker)
    run("xdotool", "key", "--clearmodifiers", "Home")
    for _ in range(LANGS.index(lang) + 1):
        run("xdotool", "key", "--clearmodifiers", "Down")
    run("xdotool", "key", "--clearmodifiers", "Return")
    eventually(lambda: settings_language(home) == lang,
               f"language switch to {lang} did not save", 12)
    # Locale-change redraws Settings and the already-open Chat asynchronously.
    eventually(lambda: locate_tab(pid, lang, "Presence"),
               f"{lang}: tab labels did not translate", 12)
    # A locale change can replace the Settings webview while AX is rebuilding.
    # The first matching translated tab is not necessarily its final redraw.
    time.sleep(1.1)
    NAMES_BY_SETTING[0] = lang


def capture_menu(pid, wid, log, lang):
    # Keep Settings away from the sprite, then right-click its actual
    # trace-reported screen position (the app's own existing E2E method).
    run("xdotool", "windowminimize", wid)
    eventually(lambda: "sprite(" in log.read_text(errors="replace"),
               "no frame/sprite trace")
    frames = re.findall(r"sprite\((-?\d+),(-?\d+)\)", log.read_text(errors="replace"))
    sizes = re.findall(r"sprite (\d+)x(\d+);", log.read_text(errors="replace"))
    if not frames or not sizes:
        raise RuntimeError("the trace has no sprite geometry")
    x, y = map(int, frames[-1])
    w, h = map(int, sizes[0])
    x += w // 2
    y += h // 2
    run("xdotool", "mousemove", "--sync", x, y, "mousedown", "3")
    time.sleep(0.16)
    run("xdotool", "mouseup", "3")

    def menu_items():
        app = app_accessible(pid)
        if app is None:
            return []
        items = []
        for node in walk(app):
            if accessible_role(node) not in ("menu item", "check menu item", "radio menu item"):
                continue
            try:
                if node.getState().contains(pyatspi.STATE_SHOWING):
                    items.append(accessible_name(node))
            except Exception:
                pass
        return items

    items = eventually(menu_items, f"{lang}: context menu did not open", 6)
    expected = [localized(lang, "Settings…"), localized(lang, "Chat…")]
    if not all(any(item == want for item in items) for want in expected):
        raise RuntimeError(f"{lang}: menu not translated: {items!r}; wanted {expected!r}")
    capture(f"{lang}/10-context-menu.png")
    run("xdotool", "key", "Escape")
    raise_window(wid)



def capture_french_speech():
    """Use the real ACP fixture to hold BMO's French speech on screen."""
    import glob

    binaries = [Path(p) for p in glob.glob(str(ROOT / "target/debug/deps/fidget-*"))
                if re.search(r"/fidget-[0-9a-f]{16}$", p) and os.access(p, os.X_OK)]
    if not binaries:
        raise RuntimeError("no fidget test binary; run cargo test -p fidget --no-run")
    test_bin = max(binaries, key=lambda p: p.stat().st_mtime)

    home = Path(tempfile.mkdtemp(prefix="fidget-french-speech-",
                                dir=os.environ.get("RUNNER_TEMP")))
    conf = home / ".local/share/fidget/settings.json"
    conf.parent.mkdir(parents=True, exist_ok=True)
    conf.write_text(json.dumps({
        "first_run_tour_shown": True,
        "language": "fr",
        "director_enabled": True,
        "proactive_wakes": True
    }), encoding="utf-8")
    marks, hold = home / "marks.txt", home / "hold"
    marks.touch()
    os.mkfifo(hold)
    command = (f"{ROOT}/scripts/scenarios/helpers/fixture-harness.sh {test_bin} "
               f"script=scenario-gallery-french count={marks} hold={hold}")
    if any(" " in str(part) for part in (ROOT, test_bin, home)):
        raise RuntimeError("fixture harness paths contain spaces")
    env = {**os.environ, "HOME": str(home),
           "FIDGET_HARNESS": command,
           "FIDGET_DIRECTOR_WAKE_SECS": "1",
           "FIDGET_DIRECTOR_API_KEY": "x",
           "FIDGET_CAPTURABLE": "1",
           "FIDGET_TRACE_FRAMES": "1",
           "FIDGET_TRACE_BUBBLE": "1",
           "FIDGET_CHARACTER": "bmo",
           "FIDGET_CHARACTERS": str(ROOT / "characters")}
    log = home / "app.log"
    with log.open("w") as log_out:
        app = subprocess.Popen([str(ROOT / "target/debug/fidget")],
                               env=env, stdout=log_out, stderr=subprocess.STDOUT,
                               start_new_session=True)
        try:
            eventually(lambda: "french-spoken" in marks.read_text(errors="replace"),
                       "the French ACP fixture did not speak", 30)
            # The fixture marks its output before the UI receives it. The
            # frontend's showSpeech trace is the real webview witness.
            eventually(lambda: "showSpeech instance=" in log.read_text(errors="replace"),
                       "WebKit never displayed the streamed French bubble", 12)

            def bubble_text():
                dump = run("/usr/bin/python3", ROOT / "scripts/ax-window-linux.py",
                           "dump", app.pid, "Fidget", check=False)
                if dump.returncode == 0 and "Bonjour ! Je suis BMO." in dump.stdout:
                    return dump.stdout
                return None

            # Some WebKitGTK builds omit overlay text from AT-SPI despite
            # drawing it (the fixture logs + animation still confirm speech).
            # Keep the AX dump as diagnostic evidence, never invent AX text.
            text_dump = bubble_text()
            if text_dump:
                (OUT / "fr/11-bmo-speaks-french.ax.txt").write_text(text_dump, encoding="utf-8")
            else:
                print("INFO: overlay speech is not exposed through this runner's AT-SPI tree", flush=True)
            trace = eventually(
                lambda: log.read_text(errors="replace")
                if "sprite(" in log.read_text(errors="replace") else None,
                "no sprite position in French bubble run", 10)
            if " talk#" not in trace:
                raise RuntimeError("BMO never entered talk animation for the French line")
            positions = re.findall(r"sprite\((-?\d+),(-?\d+)\)", trace)
            sizes = re.findall(r"sprite (\d+)x(\d+);", trace)
            if not positions or not sizes:
                raise RuntimeError("the French dialogue run lacks sprite geometry")
            sx, sy = map(int, positions[-1])
            sw, sh = map(int, sizes[0])
            screen_file = Path(tempfile.mkstemp(suffix=".png")[1])
            try:
                def visible_bubble():
                    run("import", "-window", "root", screen_file)
                    whole = Image.open(screen_file).convert("RGB")
                    # A bubble is at most 260px wide and sits above BMO's head.
                    cx = sx + sw // 2
                    left, top = max(0, cx - 210), max(0, sy - 245)
                    right = min(whole.width, cx + 210)
                    bottom = min(whole.height, sy + sh + 55)
                    crop = whole.crop((left, top, right, bottom))
                    if crop.width < 220 or crop.height < 200:
                        raise RuntimeError("French speech crop is outside the desktop")
                    # Checking image variance alone accepted a black screenshot
                    # containing only BMO's sprite. Require actual light-coloured
                    # pixels above the sprite, where the speech bubble is drawn.
                    upper_height = max(1, min(crop.height, sy - top - 8))
                    upper = crop.crop((0, 0, crop.width, upper_height))
                    light = sum(max(pixel) > 150 for pixel in upper.getdata())
                    return crop if light >= 200 else None

                image = eventually(visible_bubble,
                    "French speech bubble is not visible in framebuffer above BMO", 10)
                dest = OUT / "fr/11-bmo-speaks-french.png"
                dest.parent.mkdir(parents=True, exist_ok=True)
                image.save(dest, optimize=True)
                print(f"CAPTURE {dest.relative_to(ROOT)} (verified visible French bubble)",
                      flush=True)
            finally:
                screen_file.unlink(missing_ok=True)
        except Exception:
            debug = ROOT / "docs/i18n/gallery-debug"
            debug.mkdir(parents=True, exist_ok=True)
            run("import", "-window", "root", debug / "french-full-screen.png", check=False)
            (debug / "french-app.log").write_text(
                log.read_text(errors="replace"), encoding="utf-8")
            print("--- French speech run log ---", file=sys.stderr)
            print("\n".join(log.read_text(errors="replace").splitlines()[-100:]),
                  file=sys.stderr)
            raise
        finally:
            # Release the deliberately held ACP turn and terminate its process tree.
            try:
                fd = os.open(hold, os.O_RDWR | os.O_NONBLOCK)
                os.close(fd)
            except OSError:
                pass
            try:
                os.killpg(app.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                app.wait(timeout=6)
            except subprocess.TimeoutExpired:
                os.killpg(app.pid, signal.SIGKILL)


def build_readme():
    lines = [
        "# Fidget language gallery",
        "",
        "Real screenshots captured from the Fidget binary running on a GitHub-hosted",
        "Ubuntu Linux runner, in an isolated X11/Xvfb desktop with GTK/WebKit.",
        "These images are **not mockups**. The workflow opens Settings and Chat,",
        "switches the Language picker while Chat stays open, visits every tab,",
        "and opens the context menu. No user profile or credentials are used.",
        "",
        "**Reproduce:** [i18n-gallery GitHub Actions workflow](../../../.github/workflows/i18n-gallery.yml).",
        "",
        "| Surface | English | Français | Español |",
        "| --- | --- | --- | --- |",
    ]
    views = [(f"{i:02d}-{tab.lower()}.png", tab) for i, tab in enumerate(TABS, 1)]
    views += [("08-language-picker.png", "Language picker (expanded)"),
              ("09-chat.png", "Chat (live language change)"),
              ("10-context-menu.png", "Context menu")]
    for name, label in views:
        cells = [f"![{label} — {NAMES[lang]}]({lang}/{name})"
                 for lang in LANGS]
        lines.append("| **" + label + "** | " + " | ".join(cells) + " |")
    lines += ["", "## BMO speaking French", "",
              "![BMO speaking French in a real Fidget speech bubble](fr/11-bmo-speaks-french.png)",
              "",
              "BMO says: **Bonjour ! Je suis BMO. Enchanté de te rencontrer !**",
              "",
              "The line comes from a deterministic ACP test fixture, not a live AI model.",
              "The screenshot is taken from the actual running desktop app;",
              "the harness fixture's spoken line and the live talk animation are checked.",
              "",
              "The English screenshots use the English source strings; the French",
              "and Spanish screenshots use the branch's translation catalogs.",
              "The language is chosen through the actual in-app picker, not",
              "through a test-only DOM override or command-line language pin.", ""]
    (OUT / "README.md").write_text("\n".join(lines), encoding="utf-8")


def main():
    global NAMES_BY_SETTING
    NAMES_BY_SETTING = ["en"]
    home = Path(tempfile.mkdtemp(prefix="fidget-i18n-", dir=os.environ.get("RUNNER_TEMP")))
    config = home / ".local/share/fidget/settings.json"
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(json.dumps({
        "first_run_tour_shown": True,
        "language": "en",
        "director_enabled": False,
        "proactive_wakes": False
    }), encoding="utf-8")

    env = {**os.environ, "HOME": str(home),
           "FIDGET_OPEN_SETTINGS": "1", "FIDGET_OPEN_CHAT": "1",
           "FIDGET_TRACE_FRAMES": "1", "FIDGET_CAPTURABLE": "1",
           "FIDGET_DIRECTOR_WAKE_SECS": "3600",
           "FIDGET_CHARACTER": "bmo",
           "FIDGET_CHARACTERS": str(ROOT / "characters")}
    log = home / "app.log"
    with log.open("w") as log_out:
        app = subprocess.Popen([str(ROOT / "target/debug/fidget")],
                               env=env, stdout=log_out, stderr=subprocess.STDOUT,
                               start_new_session=True)
        try:
            settings_wid = eventually(lambda: find_window(app.pid, "Settings"),
                                     "Settings did not open", 35)
            chat_wid = eventually(lambda: find_window(app.pid, "BMO"),
                                 "Chat did not open", 35)
            eventually(lambda: locate_tab(app.pid, "en", "Presence"),
                       "English settings tabs not in AX tree", 15)
            # Fixed screen position makes the screenshots comparable and keeps
            # the menu/Chat unobscured when focus changes.
            run("xdotool", "windowmove", settings_wid, "365", "65")
            for lang in LANGS:
                if lang != "en":
                    set_language(app.pid, settings_wid, home, lang)
                for i, tab in enumerate(TABS, 1):
                    raise_window(settings_wid)
                    element = eventually(lambda: locate_tab(app.pid, lang, tab),
                                         f"{lang}: missing {tab} tab", 30)
                    click_node(element)
                    time.sleep(0.2)
                    capture(f"{lang}/{i:02d}-{tab.lower()}.png", settings_wid)
                raise_window(settings_wid)
                click_node(eventually(lambda: locate_tab(app.pid, lang, "Presence"),
                                      "cannot return to Presence"))
                picker = eventually(lambda: locate_language_picker(app.pid),
                                    f"{lang}: language picker absent")
                click_node(picker)
                time.sleep(0.2)
                capture(f"{lang}/08-language-picker.png", settings_wid, popup=True)
                run("xdotool", "key", "Escape")
                raise_window(chat_wid)
                time.sleep(0.3)
                capture(f"{lang}/09-chat.png", chat_wid)
                capture_menu(app.pid, settings_wid, log, lang)
            expected = 3 * (len(TABS) + 3)
            actual = len(list(OUT.glob("*/*.png")))
            if actual != expected:
                raise RuntimeError(f"expected {expected} PNGs, got {actual}")
            print(f"CAPTURED: {actual} interface images", flush=True)
        except Exception:
            print("\n--- Fidget process log (last 100 lines) ---", file=sys.stderr)
            print("\n".join(log.read_text(errors="replace").splitlines()[-100:]), file=sys.stderr)
            raise
        finally:
            try:
                os.killpg(app.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                app.wait(timeout=6)
            except subprocess.TimeoutExpired:
                os.killpg(app.pid, signal.SIGKILL)

    capture_french_speech()
    if len(list(OUT.glob("*/*.png"))) != 31:
        raise RuntimeError("expected 31 real screenshots (30 UI + French speech)")
    build_readme()
    print("SUCCESS: 31 live screenshots and gallery README", flush=True)


if __name__ == "__main__":
    main()
