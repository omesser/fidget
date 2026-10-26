#!/usr/bin/env python3
"""Reads a FIDGET_CLICK_TRACE log and reports, per press, which source saw it.

Three sources can see a click on macOS (#1858):

- poll: the session button level read once per tick (`session_edge=down`).
- counter: the session's mouse-down event counter, which a press between two
  ticks still bumps (`down_delta=N`).
- witness: the overlay webview's pointerdown (`witness ... edge=down`).

A press is one counter or poll down, or one witness down nothing else matched.
The question for #1858 is whether the witness ever sees a press that both the
poll and the counter missed while the loop was ticking normally. Each such
press is printed as a FLAG line and the exit status is 2.

Usage: analyze-click-trace.py LOG [--normal-gap-ms N] [--late-ms N] [--json]
"""

import argparse
import json
import re
import sys

LINE = re.compile(r"click-trace: (start|tick|witness|scenario)\b(.*)")
FIELD = re.compile(r"(\w+)=(\S+)")


def parse(lines):
    """Returns the trace events in time order, each a dict of its fields."""
    events = []
    for number, line in enumerate(lines, 1):
        match = LINE.search(line)
        if not match:
            continue
        event = dict(FIELD.findall(match.group(2)))
        if "t" not in event:
            continue
        event["kind"] = match.group(1)
        event["t"] = float(event["t"])
        event["line"] = number
        if "gap_ms" in event:
            event["gap_ms"] = float(event["gap_ms"])
        events.append(event)
    # Witness lines come from the IPC thread, so their order in the file can
    # trail the tick they belong to by a line or two.
    events.sort(key=lambda event: (event["t"], event["line"]))
    return events


def scenario_at(scenarios, t):
    name = "-"
    for at, scenario in scenarios:
        if at > t:
            break
        name = scenario
    return name


def tick_after(ticks, t):
    """The gap of the first tick after `t`, as (gap_ms, how it is known).

    Only ticks that moved a source or came 50 ms or more late are logged. So
    when the first logged tick after `t` began its gap after `t`, an unlogged
    fast tick ran in between."""
    for tick in ticks:
        if tick["t"] <= t:
            continue
        if tick["t"] - tick["gap_ms"] <= t:
            return tick["gap_ms"], "logged"
        return None, "fast"
    return None, "log-ended"


def analyze(events, normal_gap_ms, late_ms):
    scenarios = [(e["t"], e.get("name", "-")) for e in events if e["kind"] == "scenario"]
    ticks = [e for e in events if e["kind"] == "tick" and "gap_ms" in e]
    presses = []
    for tick in ticks:
        if "button" not in tick:
            continue
        downs = int(tick["down_delta"])
        polled = tick["session_edge"] == "down"
        for index in range(max(downs, int(polled))):
            presses.append(
                {
                    "button": tick["button"],
                    "t": tick["t"],
                    "from_ms": tick["t"] - tick["gap_ms"],
                    "gap_ms": tick["gap_ms"],
                    "poll": polled and index == 0,
                    "counter": index < downs,
                    "witness": False,
                    "witness_t": None,
                    "control": tick.get("control") == "1",
                    "line": tick["line"],
                }
            )
    witnesses = [e for e in events if e["kind"] == "witness"]
    for down in (w for w in witnesses if w.get("edge") == "down"):
        # The press happened inside its tick's gap, and the witness hears it
        # after the press, up to `late_ms` after (#1795 measured ~220 ms).
        candidates = [
            p
            for p in presses
            if p["button"] == down["button"]
            and not p["witness"]
            and (p["counter"] or p["poll"])
            and p["from_ms"] - 5 < down["t"] <= p["t"] + late_ms
        ]
        if candidates:
            press = min(candidates, key=lambda p: abs(p["t"] - down["t"]))
            press["witness"] = True
            press["witness_t"] = down["t"]
            continue
        gap, known = tick_after(ticks, down["t"])
        presses.append(
            {
                "button": down["button"],
                "t": down["t"],
                "from_ms": down["t"],
                "gap_ms": gap,
                "gap_known": known,
                "poll": False,
                "counter": False,
                "witness": True,
                "witness_t": down["t"],
                "control": False,
                "line": down["line"],
            }
        )
    presses.sort(key=lambda p: p["t"])
    for press in presses:
        press["scenario"] = scenario_at(scenarios, press["t"])
        start = press["witness_t"] if press["witness_t"] is not None else press["t"]
        end = next((t["t"] for t in ticks if t["t"] > start), float("inf"))
        press["forgotten"] = any(
            w.get("edge") == "forget"
            and w["button"] == press["button"]
            and start <= w["t"] <= end
            for w in witnesses
        )
        missed = press["witness"] and not press["poll"] and not press["counter"]
        known = press.get("gap_known")
        press["ticks_normal"] = known == "fast" or (
            known == "logged" and press["gap_ms"] <= normal_gap_ms
        )
        press["flag"] = missed and press["ticks_normal"]
    return presses, scenarios


def summarize(presses, scenarios):
    names = [name for _, name in scenarios] or ["-"]
    if any(p["scenario"] == "-" for p in presses) and "-" not in names:
        names.insert(0, "-")
    rows = []
    for name in dict.fromkeys(names):
        for button in ("left", "right"):
            group = [p for p in presses if p["scenario"] == name and p["button"] == button]
            if not group:
                continue
            rows.append(
                {
                    "scenario": name,
                    "button": button,
                    "presses": len(group),
                    "poll": sum(p["poll"] for p in group),
                    "counter": sum(p["counter"] for p in group),
                    "witness": sum(p["witness"] for p in group),
                    "witness_only": sum(
                        p["witness"] and not p["poll"] and not p["counter"] for p in group
                    ),
                    "flagged": sum(p["flag"] for p in group),
                }
            )
    return rows


def yes(value):
    return "y" if value else "."


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("log")
    parser.add_argument(
        "--normal-gap-ms",
        type=float,
        default=1500.0,
        help="the longest tick gap still counted as normal (the idle cap is 1 s)",
    )
    parser.add_argument(
        "--late-ms",
        type=float,
        default=400.0,
        help="how long after a press its witness can still arrive",
    )
    parser.add_argument("--json", action="store_true", help="print JSON instead")
    args = parser.parse_args()
    with open(args.log, encoding="utf-8", errors="replace") as log:
        presses, scenarios = analyze(parse(log), args.normal_gap_ms, args.late_ms)
    rows = summarize(presses, scenarios)
    flagged = [p for p in presses if p["flag"]]
    if args.json:
        json.dump({"presses": presses, "summary": rows}, sys.stdout, indent=2)
        print()
        return 2 if flagged else 0
    print("press  scenario         button  poll counter witness  gap_ms  note")
    for number, press in enumerate(presses, 1):
        gap = press["gap_ms"]
        gap_text = f"{gap:7.1f}" if gap is not None else f"{press.get('gap_known', '-'):>7}"
        notes = []
        if press["control"]:
            notes.append("control")
        if press["forgotten"]:
            notes.append("witness forgotten")
        if press["witness"] and not press["poll"] and not press["counter"]:
            notes.append("witness only, ticks normal" if press["ticks_normal"] else "witness only, ticks stalled")
        row = (
            f"{number:5}  {press['scenario']:<16} {press['button']:<6}  "
            f"{yes(press['poll']):>4} {yes(press['counter']):>7} {yes(press['witness']):>7}  "
            f"{gap_text}  {', '.join(notes)}"
        )
        print(row.rstrip())
    print()
    print("scenario         button  presses  poll  counter  witness  witness-only  flagged")
    for row in rows:
        print(
            f"{row['scenario']:<16} {row['button']:<6}  {row['presses']:7}  {row['poll']:4}  "
            f"{row['counter']:7}  {row['witness']:7}  {row['witness_only']:12}  {row['flagged']:7}"
        )
    for press in flagged:
        print(
            f"FLAG: {press['button']} witness press at t={press['witness_t']:.1f} "
            f"(line {press['line']}, scenario {press['scenario']}) missed by poll and counter "
            "while ticks were normal"
        )
    if not flagged:
        print("No witness press was missed by both the poll and the counter while ticks were normal.")
    return 2 if flagged else 0


if __name__ == "__main__":
    sys.exit(main())
