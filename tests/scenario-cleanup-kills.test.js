// A scenario leaf's cleanup may end only processes the run started: the app by
// pid, and the Harness by a command line holding this run's count= path. A
// kill by app name could reach the user's own.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

const scenarios = path.join(import.meta.dirname, "..", "scripts", "scenarios");

// Every way these leaves can end a process, and what makes it a name kill.
const NAME_KILLS = [
  {
    rule: "Stop-Process without -Id",
    at: (line) => /\bStop-Process\b/.test(line) && !/\s-Id\b/.test(line),
  },
  {
    rule: "taskkill without /PID",
    at: (line) => /\btaskkill(?:\.exe)?\b/i.test(line) && !/\/PID\b/.test(line),
  },
  { rule: "killall", at: (line) => /\bkillall\b/.test(line) },
  {
    rule: "pkill without -P or this run's count= path",
    at: (line) =>
      /\bpkill\b/.test(line) &&
      !/\s-P\b/.test(line) &&
      !(/\s-f\b/.test(line) && /\$marks\b/.test(line)),
  },
];

function violations(text) {
  const found = [];
  for (const [at, line] of text.split("\n").entries()) {
    if (/^\s*#/.test(line)) continue;
    for (const kill of NAME_KILLS) {
      if (kill.at(line)) found.push(`${at + 1}: ${kill.rule}: ${line.trim()}`);
    }
  }
  return found;
}

const leaves = fs
  .readdirSync(scenarios)
  .filter((file) => /\.(sh|ps1)$/.test(file))
  .sort();

test("every scenario leaf ends only processes the run started", () => {
  assert.ok(leaves.length >= 10, `found only ${leaves.length} scenario scripts`);
  const found = leaves.flatMap((leaf) =>
    violations(fs.readFileSync(path.join(scenarios, leaf), "utf8")).map(
      (violation) => `${leaf}: ${violation}`,
    ),
  );
  assert.deepEqual(found, []);
});

test("the check accepts a pid kill and this run's Harness marker", () => {
  const allowed = [
    "# pkill -x TextEdit is the call this check forbids",
    'kill "$pid"',
    'pkill -P "$timer_pid"',
    'pkill -f "count=$marks"',
    "taskkill.exe /PID 42 /T /F",
    "Stop-Process -Id 42 -Force",
  ].join("\n");
  assert.deepEqual(violations(allowed), []);
});

test("the check catches each way of killing by name", () => {
  for (const sample of [
    "pkill -x TextEdit",
    'pkill -f "script=slow"',
    "taskkill /IM notepad.exe /F",
    "taskkill.exe /F /IM notepad.exe",
    "Stop-Process -Name Notepad -Force",
    "killall TextEdit",
  ]) {
    assert.notDeepEqual(violations(sample), [], sample);
  }
});

const REFUSAL =
  /TextEdit is running\. Quit it first: this run opens and quits TextEdit itself\./;

for (const leaf of ["chat-close-pill-type.sh", "pill-two-displays.sh"]) {
  test(
    `${leaf} refuses a running TextEdit before it builds or touches anything`,
    { skip: process.platform === "win32" && "needs bash and a pgrep stub" },
    () => {
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "scenario-refusal-"));
      const bin = path.join(dir, "bin");
      const touched = path.join(dir, "touched");
      fs.mkdirSync(bin);
      const stub = (name, body) =>
        fs.writeFileSync(path.join(bin, name), `#!/bin/sh\n${body}\n`, {
          mode: 0o755,
        });
      stub("pgrep", "exit 0");
      for (const tool of ["swiftc", "osascript", "open", "pkill", "killall"]) {
        stub(tool, `echo ${tool} >> ${JSON.stringify(touched)}; exit 0`);
      }

      const run = spawnSync(
        "bash",
        [path.join(scenarios, leaf), "--go", "/bin/true", "/bin/true"],
        {
          encoding: "utf8",
          env: {
            ...process.env,
            PATH: `${bin}:${process.env.PATH ?? ""}`,
            TMPDIR: dir,
          },
        },
      );
      const touchedCalls = fs.existsSync(touched)
        ? fs.readFileSync(touched, "utf8")
        : "";
      fs.rmSync(dir, { recursive: true, force: true });

      assert.equal(
        run.status,
        1,
        `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
      );
      assert.match(run.stderr, REFUSAL);
      assert.equal(touchedCalls, "", `the refused run still ran ${touchedCalls}`);
    },
  );
}
