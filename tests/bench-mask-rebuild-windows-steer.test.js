// scripts/bench-mask-rebuild-windows-steer.cs reads what fidget appended to
// process.log since the last display refresh and says where the pointer goes.
// The Windows calls (SetCursorPos, the refresh wait) are not exercised here;
// the reading and aiming are, on any OS pwsh runs on.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { test } from "node:test";

import { resolvePwsh } from "./pwsh-resolve.js";
import { tempDir } from "./temp-path.js";

const source = join(import.meta.dirname, "..", "scripts", "bench-mask-rebuild-windows-steer.cs");
const resolved = resolvePwsh();
const skip = resolved.kind === "ready" ? false : resolved.reason;

// Runs `steps`, a list of [text to append, now in Unix ms], against one
// follower and returns what it reported after each step.
function follow(t, { since = 1000, half = [64, 64], scale = 1, steady = "", steps }) {
  const log = join(tempDir(t, "bench-steer-"), "process.log");
  const driver = `
    $ErrorActionPreference = 'Stop'
    Add-Type -Path '${source}'
    $steps = ('${JSON.stringify({ steps }).replaceAll("'", "''")}' | ConvertFrom-Json).steps
    $f = New-Object FidgetBench.LogFollower '${log}', ${since}, ${half[0]}, ${half[1]}, ${scale}
    if ('${steady}') { $f.Steady = '${steady}' }
    $out = foreach ($s in $steps) {
      if ($s[0]) { [System.IO.File]::AppendAllText('${log}', [string]$s[0]) }
      $moved = $f.Step([long]$s[1])
      [pscustomobject]@{ moved = $moved; x = $f.AimX; y = $f.AimY; frames = $f.Frames;
        rebuilds = @($f.Rebuilds); lags = @($f.LagsMs); counted = $f.Counted.Count; onMs = $f.OnScreenMs }
    }
    $f.Dispose()
    ConvertTo-Json -InputObject @($out) -Depth 4 -Compress
  `;
  const run = spawnSync(
    resolved.command,
    [...(resolved.args ?? []), "-NoProfile", "-NonInteractive", "-Command", driver],
    { encoding: "utf8", timeout: 60000, env: { ...process.env, TERM: "dumb" } },
  );
  assert.equal(run.status, 0, run.stdout + run.stderr);
  return JSON.parse(run.stdout);
}

const frame = (stamp, atMs, x, y, animation = "walk") =>
  `${stamp} frame: ${atMs} Grounded pos(${x + 64},${y + 128}) sprite(${x},${y}) ${animation}#3 nim\n`;

test("each step aims at the newest frame line appended since the last one", { skip }, (t) => {
  const [first, second] = follow(t, {
    steps: [
      [frame(1001, 1001000, 500, 300), 1001010],
      [frame(1001, 1001016, 498, 300) + frame(1001, 1001033, 496, 300), 1001040],
    ],
  });
  assert.deepEqual([first.moved, first.x, first.y], [true, 564, 364]);
  assert.deepEqual([second.moved, second.x, second.y, second.frames], [true, 560, 364, 3]);
});

test("a step with nothing new leaves the aim where it was", { skip }, (t) => {
  const [, idle] = follow(t, { steps: [[frame(1001, 1001000, 10, 20), 1001005], ["", 1001020]] });
  assert.deepEqual([idle.moved, idle.x, idle.y, idle.frames], [false, 74, 84, 1]);
});

test("a line still being written waits for its newline", { skip }, (t) => {
  const line = frame(1001, 1001000, 200, 100);
  const [half, whole] = follow(t, {
    steps: [
      [line.slice(0, 30), 1001001],
      [line.slice(30), 1001002],
    ],
  });
  assert.equal(half.moved, false);
  assert.deepEqual([whole.moved, whole.x, whole.y], [true, 264, 164]);
});

test("lines stamped before the recording started are skipped", { skip }, (t) => {
  const [step] = follow(t, {
    since: 1001,
    steps: [
      [
        frame(1000, 1000900, 1, 1) +
          "1000 overlay 0: region rebuild 40 rects (art 40 + hotspots 0 + painted 0), 2.10 ms\n",
        1001000,
      ],
    ],
  });
  assert.deepEqual([step.moved, step.frames, step.rebuilds], [false, 0, []]);
});

test("region rebuild lines are kept and frame lag is measured", { skip }, (t) => {
  const rebuild = "1001 overlay 0: region rebuild 41 rects (art 41 + hotspots 0 + painted 0), 2.42 ms";
  const [step] = follow(t, { steps: [[frame(1001, 1001000, 0, 0) + rebuild + "\n", 1001012]] });
  assert.deepEqual(step.rebuilds, [rebuild]);
  assert.deepEqual(step.lags, [12]);
});

test("the aim is in physical pixels at the primary display's scale", { skip }, (t) => {
  const [step] = follow(t, { scale: 1.5, steps: [[frame(1001, 1001000, 100, 200), 1001001]] });
  assert.deepEqual([step.x, step.y], [246, 396]);
});

test("a log that does not exist yet is read once it appears", { skip }, (t) => {
  const [before, after] = follow(t, {
    steps: [
      ["", 1001000],
      [frame(1001, 1001000, 0, 0), 1001001],
    ],
  });
  assert.equal(before.moved, false);
  assert.equal(after.moved, true);
});

const rebuildLine = (stamp) => `${stamp} overlay 0: region rebuild 41 rects (art 41 + hotspots 0 + painted 0), 2.42 ms\n`;

test("with a steady animation, only rebuilds while it is on screen count, over the time it was", { skip }, (t) => {
  const [step] = follow(t, {
    steady: "react",
    steps: [
      [
        frame(1001, 1001000, 0, 0, "land") +
          rebuildLine(1001) +
          frame(1001, 1001100, 0, 0, "react") +
          rebuildLine(1001) +
          frame(1001, 1001400, 0, 0, "react") +
          rebuildLine(1001) +
          frame(1001, 1001500, 0, 0, "idle") +
          rebuildLine(1001) +
          frame(1002, 1002000, 0, 0, "react") +
          rebuildLine(1002) +
          frame(1002, 1002200, 0, 0, "react"),
        1002210,
      ],
    ],
  });
  assert.equal(step.rebuilds.length, 5);
  assert.equal(step.counted, 3);
  assert.equal(step.onMs, 400 + 200);
});

test("without a steady animation every rebuild counts and on-screen time is the frame span", { skip }, (t) => {
  const [step] = follow(t, {
    steps: [[frame(1001, 1001000, 0, 0, "idle") + rebuildLine(1001) + frame(1001, 1001900, 0, 0, "sing"), 1001910]],
  });
  assert.deepEqual([step.counted, step.onMs], [1, 900]);
});

// BenchManifest.Rewrite makes the bench copy's manifest, as
// scripts/measure-mask-rebuild-x11.sh does on X11.
function rewrite(text, steady, scale) {
  const driver = `
    $ErrorActionPreference = 'Stop'
    Add-Type -Path '${source}'
    $text = '${text.replaceAll("'", "''")}'
    [FidgetBench.BenchManifest]::Rewrite($text, '${steady}', ${scale})
  `;
  const run = spawnSync(
    resolved.command,
    [...(resolved.args ?? []), "-NoProfile", "-NonInteractive", "-Command", driver],
    { encoding: "utf8", timeout: 60000, env: { ...process.env, TERM: "dumb" } },
  );
  assert.equal(run.status, 0, run.stdout + run.stderr);
  return run.stdout.replace(/\r/g, "").trimEnd();
}

const manifest = [
  'name = "BMO"',
  "scale = 1",
  "",
  "[cursor]",
  'rush_reaction = "react"',
  "",
  "[animations.react]",
  'frames = ["frames/react-0.png"]',
  "fps = 10",
  'loop = "once"',
  "",
  "[animations.land]",
  'loop = "once"',
  "",
  "[behaviors.walk]",
  'play = ["walk"]',
].join("\n");

test("the bench manifest loops the steady animation as its only behavior, with no cursor reactions", { skip }, () => {
  const out = rewrite(manifest, "react", 0).split("\n");
  assert.ok(!out.some((l) => l.startsWith("[cursor]") || l.startsWith("[behaviors.walk]")), out.join("\n"));
  assert.ok(!out.includes('rush_reaction = "react"'));
  assert.ok(out.includes("scale = 1"));
  const react = out.indexOf("[animations.react]");
  const land = out.indexOf("[animations.land]");
  assert.ok(!out.slice(react, land).includes('loop = "once"'), "react loops");
  assert.equal(out[land + 1], 'loop = "once"', "other animations keep their loop");
  const bench = out.indexOf("[behaviors.bench]");
  assert.ok(bench > 0);
  const play = out[bench + 1];
  assert.equal(play, `play = [${Array(50).fill('"react"').join(", ")}]`);
});

test("a scale replaces the manifest's, above the first table", { skip }, () => {
  const out = rewrite(manifest, "walk", 4).split("\n");
  assert.equal(out[0], "scale = 4");
  assert.ok(!out.includes("scale = 1"));
});
