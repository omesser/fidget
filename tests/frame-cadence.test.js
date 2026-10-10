import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { DROP_MS, analyze, compare, report, summarizeTimestamps, unwrapProcessLog } from "../scripts/frame-cadence.mjs";

// Five Engine ticks of a walk, one 40 ms late, and the display frames an overlay
// drew across them: a quiet loop restarts on the late arrival, then misses a
// vsync and holds the sprite at the latest placement. The sixth tick resends a
// standing sprite 250 ms on, and a still sprite has no lag to measure. The
// loop's own counter reports 23 ticks over the last 400 ms.
const log = [
  "cadence: overlay-0 900.000 1 880.000 890.000",
  "frame: 1000 Grounded pos(1,2) sprite(3,4) walk#0 BMO",
  "overlay: overlay-0 covers 1512x982 at (0,0)",
  "frame: 1016 Grounded pos(2,2) sprite(3,4) walk#1 BMO",
  "cadence: overlay-0 1017.000 1 1000.000 1016.500",
  "cadence: overlay-1 1025.000 1 - 1016.500",
  "frame: 1033 Grounded pos(3,2) sprite(3,4) walk#2 BMO",
  "cadence: overlay-0 1033.667 1 1016.500 1033.200",
  "frame: 1050 Grounded pos(4,2) sprite(3,4) walk#3 BMO",
  "cadence: overlay-0 1050.333 0 1033.200 1050.000",
  "frame: 1090 Grounded pos(5,2) sprite(3,4) walk#4 BMO",
  "cadence: overlay-0 1100.333 1 1050.000 1090.500",
  "cadence: overlay-0 1117.000 1 1050.000 1090.500",
  "cadence: overlay-0 1150.333 0 1050.000 1090.500",
  "frame: 1340 Grounded pos(5,2) sprite(3,4) walk#4 BMO",
  "cadence: overlay-0 1341.000 0 1090.500 1340.500",
  "cadence-ticks: 1000 60",
  "cadence-ticks: 1200 11",
  "cadence-ticks: 1400 12",
  "frame: 2000 Grounded pos(1,2) sprite(3,4) walk#5 BMO",
].join("\n");

// Gaps of 16 ms, then exactly DROP_MS, then one millisecond over it. A gap
// equal to the threshold is still a frame. Only the one past it is a drop.
test("timestamps become a histogram, a drop count, and an average fps", () => {
  const times = [0, 16, 32, 32 + DROP_MS, 32 + DROP_MS + (DROP_MS + 1)];
  assert.deepEqual(summarizeTimestamps(times), {
    fps: 54.8,
    drops: 1,
    histogram: [
      { bin: "0-10", count: 0 },
      { bin: "10-14", count: 0 },
      { bin: "14-18", count: 2 },
      { bin: "18-20", count: 0 },
      { bin: "20-25", count: 2 },
      { bin: "25-34", count: 0 },
      { bin: "34-50", count: 0 },
      { bin: "50+", count: 0 },
    ],
  });
});

test("a release process log timestamp still leaves the frame line", () => {
  const body = "frame: 1000 Perched pos(1,2) sprite(3,4) idle#0 BMO";
  const wrapped = `1700000000 ${body}`;
  assert.equal(unwrapProcessLog(wrapped), body);
  assert.equal(unwrapProcessLog(`note ${body}`), `note ${body}`);
  assert.equal(analyze(wrapped, {}).ticks, 1);
  const dir = mkdtempSync(join(tmpdir(), "frame-cadence-"));
  writeFileSync(join(dir, "process.log"), "1700000000 cadence: overlay-0 1.000 0 - 1.000\n");
  const run = spawnSync("node", ["scripts/frame-cadence.mjs", "unwrap", join(dir, "process.log")]);
  assert.equal(run.status, 0, run.stderr.toString());
  assert.equal(run.stdout.toString(), "cadence: overlay-0 1.000 0 - 1.000\n");
});

test("one timestamp has no frame rate and no drops", () => {
  const one = summarizeTimestamps([1000]);
  assert.equal(one.fps, null);
  assert.equal(one.drops, 0);
  assert.equal(one.histogram.reduce((n, bin) => n + bin.count, 0), 0);
});

test("a window of the log reduces to cadence, drops, and interpolation lag", () => {
  assert.deepEqual(analyze(log, { from: 1000, to: 1400 }), {
    frames: 8,
    fps: 48,
    drops: 1,
    restarts: 2,
    ticks: 6,
    tickHz: 14.7,
    moving: { ticks: 4, gapMs: 22.5, hz: 44.4 },
    still: { ticks: 1, gapMs: 250, hz: 4 },
    countedHz: 57.5,
    lag: { p50Ms: 16.8, p95Ms: 59.8, p50Samples: 1, p95Samples: 1.48, held: 1 },
    histogram: [
      { bin: "0-10", frames: 0, ticks: 0 },
      { bin: "10-14", frames: 0, ticks: 0 },
      { bin: "14-18", frames: 3, ticks: 3 },
      { bin: "18-20", frames: 0, ticks: 0 },
      { bin: "20-25", frames: 0, ticks: 0 },
      { bin: "25-34", frames: 1, ticks: 0 },
      { bin: "34-50", frames: 0, ticks: 1 },
      { bin: "50+", frames: 0, ticks: 1 },
    ],
  });
});

// Shaped like a walking run: BMO walks on the 16 ms deadline, stands between
// walks at the wake-based 20 ms pace, and walks again.
test("Engine ticks split into moving and still, which the blended rate hides", () => {
  const walking = [
    "frame: 0 Grounded pos(1,2) sprite(3,4) walk#0 BMO",
    "frame: 16 Grounded pos(2,2) sprite(3,4) walk#1 BMO",
    "frame: 32 Grounded pos(3,2) sprite(3,4) walk#2 BMO",
    "frame: 48 Grounded pos(4,2) sprite(3,4) walk#3 BMO",
    "frame: 68 Grounded pos(4,2) sprite(3,4) idle#0 BMO",
    "frame: 88 Grounded pos(4,2) sprite(3,4) idle#0 BMO",
    "frame: 108 Grounded pos(4,2) sprite(3,4) idle#1 BMO",
    "frame: 128 Grounded pos(4,2) sprite(3,4) idle#1 BMO",
    "frame: 144 Grounded pos(5,2) sprite(3,4) walk#0 BMO",
  ].join("\n");
  const result = analyze(walking, {});
  assert.equal(result.tickHz, 55.6);
  assert.deepEqual(result.moving, { ticks: 4, gapMs: 16, hz: 62.5 });
  assert.deepEqual(result.still, { ticks: 4, gapMs: 20, hz: 50 });
  assert.match(report(result), /\| Engine ticks, moving \| 4 \(16 ms, 62\.5 Hz\) \|/);
});

test("a window with no display frames says so rather than dividing by zero", () => {
  const idle = analyze("frame: 1000 Perched pos(1,2) sprite(3,4) idle#0 BMO", {});
  assert.equal(idle.frames, 0);
  assert.equal(idle.fps, null);
  assert.equal(idle.lag, null);
  assert.equal(idle.countedHz, null);
  assert.deepEqual(idle.moving, { ticks: 0, gapMs: null, hz: null });
  assert.match(report(idle), /\| Display frames \| 0 \|/);
});

test("the report is a Markdown table a baseline doc can paste", () => {
  const text = report(analyze(log, { from: 1000, to: 1400 }));
  assert.match(text, /\| Mean fps \| 48 \|/);
  assert.match(text, new RegExp(`\\| Dropped \\(>${DROP_MS} ms\\) \\| 1 \\|`));
  assert.match(text, /\| Interpolation lag p50, moving \| 16\.8 ms \(1 samples\) \|/);
  assert.match(text, /\| 25-34 \| 1 \| 0 \|/);
  assert.match(text, /\| Engine ticks\/s, loop counter \| 57\.5 \|/);
  assert.match(text, /\| Engine ticks, moving \| 4 \(22\.5 ms, 44\.4 Hz\) \|/);
  assert.match(text, /\| Engine ticks, still \| 1 \(250 ms, 4 Hz\) \|/);
});

const standing = (gapMs) =>
  Array.from({ length: 5 }, (_, i) => `frame: ${i * gapMs} Grounded pos(1,2) sprite(3,4) idle#0 BMO`).join("\n");

test("an A/B comparison reports each side's mean and rounds, and B minus A", () => {
  const a = [analyze(standing(20), {}), analyze(standing(25), {})];
  const b = [analyze(log, { from: 1000, to: 1400 })];
  assert.equal(
    compare(a, b),
    [
      "| Metric | A | B | B - A |",
      "|---|---|---|---|",
      "| Mean fps | N/A (N/A, N/A) | 48 (48) | N/A |",
      `| Dropped (>${DROP_MS} ms) | 0 (0, 0) | 1 (1) | 1 |`,
      "| Engine ticks/s, loop counter | N/A (N/A, N/A) | 57.5 (57.5) | N/A |",
      "| Engine ticks, still (Hz) | 45 (50, 40) | 4 (4) | -41 |",
      "| Engine ticks, still (ms) | 22.5 (20, 25) | 250 (250) | 227.5 |",
      "| Engine ticks, moving (Hz) | N/A (N/A, N/A) | 44.4 (44.4) | N/A |",
      "| Engine ticks, moving (ms) | N/A (N/A, N/A) | 22.5 (22.5) | N/A |",
      "| Interpolation lag p95, moving (ms) | N/A (N/A, N/A) | 59.8 (59.8) | N/A |",
      "| Moving frames held at the latest placement | N/A (N/A, N/A) | 1 (1) | N/A |",
    ].join("\n"),
  );
});

test("the reducer writes JSON that its compare command reads back", () => {
  const dir = mkdtempSync(join(tmpdir(), "frame-cadence-"));
  const reduce = (name, text) => {
    writeFileSync(join(dir, `${name}.log`), text);
    const run = spawnSync("node", ["scripts/frame-cadence.mjs", join(dir, `${name}.log`), "--json", join(dir, `${name}.json`)]);
    assert.equal(run.status, 0, run.stderr.toString());
    return join(dir, `${name}.json`);
  };
  const a = [reduce("a1", standing(20)), reduce("a2", standing(25))];
  const b = [reduce("b1", standing(16))];
  const run = spawnSync("node", ["scripts/frame-cadence.mjs", "compare", "--a", ...a, "--b", ...b]);
  assert.equal(run.status, 0, run.stderr.toString());
  assert.match(run.stdout.toString(), /\| Engine ticks, still \(Hz\) \| 45 \(50, 40\) \| 62\.5 \(62\.5\) \| 17\.5 \|/);
});

test("the bench refuses a third binary, and rounds without a second or of zero, before it launches anything", () => {
  const bench = (...args) => spawnSync("bash", ["scripts/bench-frame-cadence-macos.sh", ...args], { env: { ...process.env, FIDGET_BENCH_GREEN_LIGHT: "" } });
  for (const args of [
    ["idle", "--bin", "a", "--bin", "b", "--bin", "c"],
    ["idle", "--bin", "a", "--rounds", "2"],
    ["idle", "--bin", "a", "--bin", "b", "--rounds", "0"],
  ]) {
    const run = bench(...args);
    assert.equal(run.status, 2);
    assert.match(run.stderr.toString(), /^Usage: /m);
  }
  const ab = bench("idle", "--bin", "a", "--bin", "b", "--rounds", "2");
  assert.equal(ab.status, 2);
  assert.match(ab.stderr.toString(), /FIDGET_BENCH_GREEN_LIGHT=1 once the operator has agreed/);
});
