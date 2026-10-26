// The #1858 analyzer must tell a press the overlay alone heard from one the
// session poll or its event counter also saw, and only flag the first kind
// when the frame loop was ticking normally.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { tempDir } from "./temp-path.js";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(ROOT, "scripts", "analyze-click-trace.py");

const tick = (t, gap, button, fields) =>
  `click-trace: tick t=${t} gap_ms=${gap} button=${button} ${fields} control=0 out_primary=0 out_secondary=0`;

const LOG = [
  "overlay: 1 display(s)",
  "click-trace: start t=1000.0 left_downs=10 left_ups=10 right_downs=3 right_ups=3",
  "click-trace: scenario name=art-left t=1100.0",
  // Seen by all three: the witness lands a few ms before the tick.
  "click-trace: witness t=1205.0 button=left edge=down",
  tick(1210.0, 16.0, "left", "session=1 session_edge=down down_delta=1 up_delta=0"),
  "click-trace: witness t=1300.0 button=left edge=up",
  tick(1306.0, 16.0, "left", "session=0 session_edge=up down_delta=0 up_delta=1"),
  // A tap between two idle ticks: only the counter and the witness saw it.
  // The schedule lines around it are context the analyzer skips.
  "click-trace: idle t=1400.0",
  "click-trace: witness t=1500.0 button=left edge=down",
  "click-trace: witness t=1540.0 button=left edge=up",
  tick(1600.0, 100.0, "left", "session=0 session_edge=- down_delta=1 up_delta=1"),
  "click-trace: active t=1610.0",
  // The witness alone, with fast unlogged ticks after it: the #1858 flag.
  "click-trace: witness t=2000.0 button=left edge=down",
  "click-trace: witness t=2060.0 button=left edge=up",
  "click-trace: tick t=2400.0 gap_ms=60.0",
  "click-trace: scenario name=off-art t=3000.0",
  // Off the art: the poll and the counter, no witness, as expected.
  tick(3100.0, 16.0, "left", "session=1 session_edge=down down_delta=1 up_delta=0"),
  tick(3180.0, 16.0, "left", "session=0 session_edge=up down_delta=0 up_delta=1"),
  "click-trace: scenario name=stall t=4000.0",
  // The witness alone, but the loop stalled for 3 s across it: not flagged.
  "click-trace: tick t=4100.0 gap_ms=40.0",
  "click-trace: witness t=4200.0 button=right edge=down",
  "click-trace: witness t=4250.0 button=right edge=up",
  "click-trace: tick t=7100.0 gap_ms=3000.0",
].join("\n");

function analyze(t, args = []) {
  const dir = tempDir(t, "click-trace-");
  const log = join(dir, "fidget.log");
  writeFileSync(log, LOG);
  return spawnSync("python3", [SCRIPT, log, ...args], { cwd: ROOT, encoding: "utf8" });
}

test("each press reports which source saw it", (t) => {
  const run = analyze(t, ["--json"]);
  const { presses } = JSON.parse(run.stdout);
  const seen = presses.map((p) => [p.scenario, p.button, p.poll, p.counter, p.witness, p.flag]);
  assert.deepEqual(seen, [
    ["art-left", "left", true, true, true, false],
    ["art-left", "left", false, true, true, false],
    ["art-left", "left", false, false, true, true],
    ["off-art", "left", true, true, false, false],
    ["stall", "right", false, false, true, false],
  ]);
  assert.equal(run.status, 2, "a flagged press fails the run");
});

test("the text report names the flagged press and the summary counts it", (t) => {
  const run = analyze(t);
  assert.match(run.stdout, /FLAG: left witness press at t=2000\.0 .*scenario art-left/);
  assert.match(run.stdout, /witness only, ticks stalled/);
  assert.match(run.stdout, /art-left\s+left\s+3\s+1\s+2\s+3\s+1\s+1/);
});

test("a long enough normal gap turns the stalled press into a flag", (t) => {
  const run = analyze(t, ["--json", "--normal-gap-ms", "5000"]);
  const flagged = JSON.parse(run.stdout).presses.filter((p) => p.flag);
  assert.deepEqual(flagged.map((p) => p.witness_t), [2000, 4200]);
});
