#!/usr/bin/env python3
"""Measure traced release Chat opens through startup and native tray actions.

On screen: launches Fidget as BMO, opens Chat, closes it, and opens it from
the tray for each process. The native tray action takes focus.
Input: real tray clicks and native close-button actions. No text is typed.
Duration: about three minutes for five runs per side, a ten-minute cap plus teardown.
Each run needs the owner's GUI approval; the agent's private VM is exempt.
"""

import argparse
import ctypes
import json
import os
import platform
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path


TRACE = re.compile(
    r"chat-timing: label=(\S+) event=(\S+) at_us=(\d+) "
    r"duration_us=(?:Some\((\d+)\)|None)"
)
EXPIRES = None


def budget(seconds):
    remaining = seconds if EXPIRES is None else min(seconds, EXPIRES - time.monotonic())
    if remaining <= 0:
        raise TimeoutError("benchmark exceeded its ten-minute measurement cap")
    return remaining


def events(log):
    text = log.read_text(encoding="utf-8", errors="replace") if log.exists() else ""
    return [
        {"label": m[1], "event": m[2], "at_us": int(m[3]),
         "duration_us": int(m[4]) if m[4] is not None else None}
        for m in TRACE.finditer(text)
    ]


def wait_for(predicate, proc, description, seconds=30):
    deadline = time.monotonic() + budget(seconds)
    while time.monotonic() < deadline:
        if predicate():
            return
        if proc.poll() is not None:
            raise RuntimeError(f"Fidget exited {proc.returncode} waiting for {description}")
        time.sleep(0.02)
    raise TimeoutError(f"timed out waiting for {description}")


def run(command, destination, env=None):
    with destination.open("w", encoding="utf-8") as output:
        subprocess.run(command, stdout=output, stderr=subprocess.STDOUT,
                       env=env, timeout=budget(35), check=True)


class NativeChat:
    def __init__(self, root, output):
        self.root, self.output = root, output
        if sys.platform == "darwin":
            self.ax = output / "ax"
            self.close_tool = output / "chat-close"
            self.window_id = output / "window-id"
            for source, binary in [
                ("scripts/ax-settings.swift", self.ax),
                ("scripts/bench-chat-close.swift", self.close_tool),
                ("scripts/scenarios/helpers/window-id.swift", self.window_id),
            ]:
                subprocess.run(["swiftc", "-O", str(root / source), "-o", str(binary)], check=True)
        else:
            from ctypes import wintypes
            self.user32 = ctypes.WinDLL("user32", use_last_error=True)
            self.callback = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
            self.user32.EnumWindows.argtypes = [self.callback, wintypes.LPARAM]
            self.user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
            self.user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
            self.user32.IsWindowVisible.argtypes = [wintypes.HWND]
            self.user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]

    def visible(self, pid):
        if sys.platform == "darwin":
            return subprocess.run([str(self.window_id), str(pid), "BMO"],
                                  stdout=subprocess.DEVNULL, timeout=budget(5), check=False).returncode == 0
        from ctypes import wintypes
        found = []

        @self.callback
        def visit(window, _):
            owner = wintypes.DWORD()
            self.user32.GetWindowThreadProcessId(window, ctypes.byref(owner))
            title = ctypes.create_unicode_buffer(256)
            self.user32.GetWindowTextW(window, title, len(title))
            if owner.value == pid and title.value == "BMO" and self.user32.IsWindowVisible(window):
                found.append(window)
            return True

        self.user32.EnumWindows(visit, 0)
        return found

    def close(self, proc, output):
        if sys.platform == "darwin":
            run([str(self.close_tool), str(proc.pid), "BMO"], output / "close.txt")
        else:
            windows = self.visible(proc.pid)
            if len(windows) != 1 or not self.user32.PostMessageW(windows[0], 0x0010, 0, 0):
                raise RuntimeError("could not post WM_CLOSE to the one visible BMO window")
        wait_for(lambda: not self.visible(proc.pid), proc, "Chat to close")

    def open(self, proc, output):
        if sys.platform == "darwin":
            command = [str(self.ax), "open", str(proc.pid), "Chat", "BMO"]
        else:
            command = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                       str(self.root / "scripts/ax-window-win.ps1"), "tray",
                       "-ProcessId", str(proc.pid), "-Title", "BMO", "-Arg3", "Chat"]
        run(command, output / "open.txt")

    def assert_document(self, proc, output, phase):
        dump = output / f"{phase}.ax.txt"
        if sys.platform == "darwin":
            command = [str(self.ax), "dump", str(proc.pid), "BMO"]
            expected = "AXWebArea|"
        else:
            command = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                       str(self.root / "scripts/ax-window-win.ps1"), "dump",
                       "-ProcessId", str(proc.pid), "-Title", "BMO"]
            expected = "web-area|"
        run(command, dump)
        text = dump.read_text(encoding="utf-8-sig", errors="replace")
        if expected not in text or "BMO" not in text or not self.visible(proc.pid):
            raise RuntimeError(f"{phase} did not produce a visible BMO document; see {dump}")


def durations(trace):
    required = {"open-request", "main-dispatch", "opened"}
    count = sum(e["event"] == "built" for e in trace)
    if count:
        required |= {"build-request", "builder-configured", "built", "setup", "page-loaded", "ready"}
    missing = required - {e["event"] for e in trace}
    if missing or count > 1:
        raise RuntimeError(f"incomplete or duplicated Chat opening trace: missing={sorted(missing)}, builds={count}")
    result = {"build_count": count}
    for event in trace:
        if event["duration_us"] is not None:
            result[event["event"] + "_us"] = event["duration_us"]
    requested = next((e["at_us"] for e in trace if e["event"] == "open-request"), None)
    ready = next((e["at_us"] for e in trace if e["event"] == "ready"), None)
    if requested is not None and ready is not None and ready >= requested:
        result["request_to_listening_us"] = ready - requested
    page_started = next((e["at_us"] for e in trace if e["event"] == "page-started"), None)
    page_loaded = next((e["at_us"] for e in trace if e["event"] == "page-loaded"), None)
    if page_started is not None and page_loaded is not None:
        result["navigation_us"] = page_loaded - page_started
    dispatched = next((e["at_us"] for e in trace if e["event"] == "main-dispatch"), None)
    opened = next((e["at_us"] for e in trace if e["event"] == "opened"), None)
    if dispatched is not None and opened is not None:
        result["main_callback_us"] = opened - dispatched
    return result


def sample(binary, root, output, native):
    output.mkdir(parents=True)
    data = output / "data"
    data.mkdir()
    (data / "settings.json").write_text('{"first_run_tour_shown":true}', encoding="utf-8")
    env = dict(os.environ, FIDGET_DATA_DIR=str(data), FIDGET_TRACE_BUBBLE="1",
               FIDGET_OPEN_CHAT="1", FIDGET_HARNESS="", FIDGET_DIRECTOR="off",
               FIDGET_DIRECTOR_API_KEY="x", FIDGET_CHARACTER="bmo",
               FIDGET_CHARACTERS=str(root / "characters"), FIDGET_CAPTURABLE="1")
    if sys.platform == "win32":
        env["WEBVIEW2_USER_DATA_FOLDER"] = str(native.output / "webview2")
    log = data / "process.log"
    with (output / "console.log").open("wb") as console:
        launched = time.monotonic_ns()
        proc = subprocess.Popen([str(binary)], cwd=root, env=env,
                                stdout=console, stderr=subprocess.STDOUT)
        try:
            wait_for(lambda: {e["event"] for e in events(log)} >= {"opened", "ready", "page-loaded"},
                     proc, "startup Chat to open, load, and listen")
            startup_observed_us = (time.monotonic_ns() - launched) // 1000
            native.assert_document(proc, output, "startup")
            startup = events(log)
            if sum(e["event"] == "built" for e in startup) != 1:
                raise RuntimeError("startup must create exactly one native Chat webview")
            native.close(proc, output)
            offset = len(events(log))
            native.open(proc, output)
            wait_for(lambda: any(e["event"] == "opened" for e in events(log)[offset:]),
                     proc, "tray Chat to open")
            wait_for(lambda: not any(e["event"] == "built" for e in events(log)[offset:])
                     or {e["event"] for e in events(log)[offset:]} >= {"ready", "page-loaded"},
                     proc, "new tray document to load and listen")
            native.assert_document(proc, output, "reopen")
            reopened = events(log)[offset:]
            startup_durations = durations(startup)
            startup_durations["process_to_ready_observed_us"] = startup_observed_us
            return {"startup": startup_durations, "reopen": durations(reopened),
                    "events": {"startup": startup, "reopen": reopened}}
        finally:
            if proc.poll() is None:
                if sys.platform == "win32":
                    subprocess.run(["taskkill", "/PID", str(proc.pid), "/T", "/F"],
                                   stdout=subprocess.DEVNULL, check=False)
                else:
                    proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=10)


def main():
    global EXPIRES
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--go", action="store_true", help="permit launches, native tray clicks, and native closes")
    args = parser.parse_args()
    if not args.go:
        print(__doc__)
        parser.error("requires --go after GUI approval; hosted private VMs are exempt")
    if sys.platform not in ("darwin", "win32") or args.runs < 5:
        parser.error("requires macOS or Windows and at least five runs per binary")
    root = Path(__file__).resolve().parent.parent
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    result = {"platform": platform.platform(), "cpu_count": os.cpu_count(),
              "load_average": os.getloadavg() if hasattr(os, "getloadavg") else None,
              "runs": [], "errors": [], "order": "baseline,candidate repeated",
              "method": "one startup open and native close/tray reopen per fresh process; one excluded warmup per side",
              "process_ready_observer_interval_us": 20000}
    arms = [("baseline", args.baseline.resolve()), ("candidate", args.candidate.resolve())]
    try:
        native = NativeChat(root, output)
        EXPIRES = time.monotonic() + 600
        if sys.platform == "win32":
            for arm, binary in arms:
                env = dict(os.environ, FIDGET_DATA_DIR=str(output / f"promote-{arm}"),
                           FIDGET_OPEN_CHAT="0", FIDGET_HARNESS="", FIDGET_DIRECTOR="off",
                           FIDGET_DIRECTOR_API_KEY="x", FIDGET_CHARACTERS=str(root / "characters"))
                run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                     str(root / "scripts/show-tray-icon-win.ps1"), str(binary)], output / f"promote-{arm}.txt", env)
        for index in range(args.runs + 1):
            for arm, binary in arms:
                folder = output / f"{index:02}-{arm}"
                row = {"index": index, "arm": arm, "binary": str(binary), "warmup": index == 0}
                try:
                    row.update(sample(binary, root, folder, native))
                except Exception as error:
                    row["error"] = repr(error)
                    result["errors"].append({"folder": str(folder), "error": repr(error)})
                result["runs"].append(row)
                (output / "results.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
                print(json.dumps(row), flush=True)
                if "error" in row:
                    return 1
        result["summary"] = {}
        for arm, _ in arms:
            result["summary"][arm] = {}
            for phase in ("startup", "reopen"):
                rows = [r[phase] for r in result["runs"] if r["arm"] == arm and not r["warmup"]]
                keys = sorted({key for row in rows for key in row})
                result["summary"][arm][phase] = {
                    key: {"n": len(values), "median": statistics.median(values),
                          "min": min(values), "max": max(values)}
                    for key in keys if (values := [r[key] for r in rows if key in r])
                }
        print(json.dumps(result["summary"], indent=2))
        return 0
    except Exception as error:
        result["errors"].append({"error": repr(error)})
        print(repr(error), file=sys.stderr)
        return 1
    finally:
        (output / "results.json").write_text(json.dumps(result, indent=2), encoding="utf-8")


if __name__ == "__main__":
    sys.exit(main())
