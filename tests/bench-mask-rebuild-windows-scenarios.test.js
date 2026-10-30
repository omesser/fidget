// scripts/bench-mask-rebuild-windows-scenarios.ps1 runs every #428 scenario on
// the live Windows desktop. Here only its gate and its table are exercised:
// the table reads the result.json each scenario run leaves behind.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { resolvePwsh } from "./pwsh-resolve.js";
import { tempDir } from "./temp-path.js";

const script = join(import.meta.dirname, "..", "scripts", "bench-mask-rebuild-windows-scenarios.ps1");
const resolved = resolvePwsh();
const skip = resolved.kind === "ready" ? false : resolved.reason;

function run(args, env = {}) {
  const { FIDGET_BENCH_GREEN_LIGHT: _, ...rest } = process.env;
  return spawnSync(
    resolved.command,
    [...(resolved.args ?? []), "-NoProfile", "-NonInteractive", "-File", script, ...args],
    { encoding: "utf8", timeout: 60000, env: { ...rest, TERM: "dumb", ...env } },
  );
}

test("it will not take the pointer without the operator's green light", { skip }, (t) => {
  const out = tempDir(t, "mask-scenarios-");
  const result = run(["-Bin", join(out, "fidget.exe"), "-Out", out]);
  assert.equal(result.status, 2, result.stdout + result.stderr);
  assert.match(result.stderr, /FIDGET_BENCH_GREEN_LIGHT=1/);
});

test("the report has one row a scenario, with rate, ms per rebuild and mask ms per second", { skip }, (t) => {
  const out = tempDir(t, "mask-scenarios-");
  mkdirSync(join(out, "small"));
  writeFileSync(
    join(out, "small", "result.json"),
    JSON.stringify({
      scenario: "small",
      character: "Nim",
      timing: "react",
      rebuilds: 200,
      onSeconds: 16.5,
      rate: 12.12,
      msMin: 0.9,
      msMedian: 1.36,
      msMax: 4.1,
      rectsMedian: 40,
      frames: 1000,
    }),
  );
  const result = run(["-Report", out]);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const table = readFileSync(join(out, "table.txt"), "utf8");
  const small = table.split(/\r?\n/).find((line) => /^small\s/.test(line));
  assert.ok(small, table);
  for (const cell of ["Nim", "react", "200", "16.5", "12.12", "1.36", "16.48"]) {
    assert.ok(small.includes(cell), `${cell} in ${small}`);
  }
  assert.match(table, /^idle\s+\(no result\.json\)/m);
});

test("-Only keeps just the named rows, as one comma-separated argument", { skip }, (t) => {
  const out = tempDir(t, "mask-scenarios-");
  const result = run(["-Report", out, "-Only", "idle,small-1x"]);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const rows = readFileSync(join(out, "table.txt"), "utf8")
    .split(/\r?\n/)
    .map((line) => line.split(/\s+/)[0])
    .filter((name) => name && !/^(Scenario|-+)$/.test(name));
  assert.deepEqual(rows, ["idle", "small-1x"]);
});

test("-Only refuses a row it does not have", { skip }, (t) => {
  const out = tempDir(t, "mask-scenarios-");
  const result = run(["-Report", out, "-Only", "idle,tiny"]);
  assert.equal(result.status, 2, result.stdout + result.stderr);
  assert.match(result.stderr, /unknown -Only tiny/);
});
