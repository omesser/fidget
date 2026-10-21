// A scenario leaf's cleanup may end only processes the run started: the app by
// pid, and the Harness by a command line holding this run's count= path. A
// kill by app name could reach the user's own.

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

const scenarios = path.join(import.meta.dirname, "..", "scripts", "scenarios");

// A pid picked out of a name lookup is still a name kill.
const NAME_LOOKUPS = /\bpgrep\b|Get-Process\s+-Name\b|GetProcessesByName\b/i;
const KILLS = /\b(?:kill|pkill|killall|Stop-Process|taskkill)\b|\.Kill\(/i;

// Every way these leaves can end a process, and what makes it a name kill.
const NAME_KILLS = [
  {
    rule: "Stop-Process without -Id",
    matches: (line) => /\bStop-Process\b/i.test(line) && !/\s-Id\b/i.test(line),
  },
  {
    rule: "taskkill without /PID",
    matches: (line) =>
      /\btaskkill(?:\.exe)?\b/i.test(line) && !/\/PID\b/i.test(line),
  },
  { rule: "killall", matches: (line) => /\bkillall\b/.test(line) },
  {
    rule: "pkill without -P or this run's count= path",
    matches: (line) =>
      /\bpkill\b/.test(line) &&
      !/\s-P\b/.test(line) &&
      !(/\s-f\b/.test(line) && /\$marks\b/.test(line)),
  },
  {
    rule: "a kill fed by a name lookup",
    matches: (line) => KILLS.test(line) && NAME_LOOKUPS.test(line),
  },
  {
    rule: ".Kill() on a process the run did not start",
    matches: (line) =>
      /\.Kill\(/.test(line) && !/\$(?:script:proc|Process)\.Kill\(/.test(line),
  },
];

function violations(text) {
  const found = [];
  for (const [at, line] of text.split("\n").entries()) {
    if (/^\s*#/.test(line)) continue;
    for (const kill of NAME_KILLS) {
      if (kill.matches(line)) {
        found.push(`${at + 1}: ${kill.rule}: ${line.trim()}`);
      }
    }
  }
  return found;
}

// Recursive, so a reorganisation into ci/, manual-only/ and helpers/ stays
// covered and no leaf drops out by moving.
function scenarioFiles(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return scenarioFiles(full);
    return /\.(sh|ps1)$/.test(entry.name) ? [full] : [];
  });
}

const leaves = scenarioFiles(scenarios).sort();

function findLeaf(name) {
  const leaf = leaves.find((file) => path.basename(file) === name);
  assert.ok(leaf, `no ${name} under scripts/scenarios`);
  return leaf;
}

test("every scenario leaf ends only processes the run started", () => {
  assert.ok(leaves.length >= 10, `found only ${leaves.length} scenario scripts`);
  assert.ok(
    leaves.some((file) => file.endsWith(".win.ps1")),
    "the scan no longer reaches a Windows leaf",
  );
  const found = leaves.flatMap((leaf) =>
    violations(fs.readFileSync(leaf, "utf8")).map(
      (violation) => `${path.relative(scenarios, leaf)}: ${violation}`,
    ),
  );
  assert.deepEqual(found, []);
});

test("chat-close-pill-type.win.ps1 ends processes only through Stop-FidgetTree", () => {
  const leaf = findLeaf("chat-close-pill-type.win.ps1");
  const calls = fs
    .readFileSync(leaf, "utf8")
    .split("\n")
    .filter(
      (line) =>
        !/^\s*#/.test(line) &&
        /Stop-FidgetTree|\bkill\b|pkill|killall|Stop-Process|taskkill|\.Kill\(/i.test(
          line,
        ),
    )
    .map((line) => line.trim());
  assert.deepEqual(calls, ["Stop-FidgetTree $script:proc $harnessTag"]);
});

test("the check accepts a pid kill and this run's Harness marker", () => {
  const allowed = [
    "# pkill -x TextEdit is the call this check forbids",
    'kill "$pid"',
    'pkill -P "$timer_pid"',
    'pkill -f "count=$marks"',
    "taskkill.exe /PID 42 /T /F",
    "Stop-Process -Id 42 -Force",
    "$script:proc.Kill()",
    "Get-Process -Name fidget | ForEach-Object { $_.Id }",
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
    "stop-process -name Notepad -Force",
    "killall TextEdit",
    "kill $(pgrep -x TextEdit)",
    "pgrep -x TextEdit | xargs kill",
    "Stop-Process -Id (Get-Process -Name notepad).Id",
    'GetProcessesByName("notepad") | % { $_.Kill() }',
    "$_.Kill()",
  ]) {
    assert.notDeepEqual(violations(sample), [], sample);
  }
});

const REFUSAL =
  /TextEdit is running\. Quit it first: this run opens and quits TextEdit itself\./;

function sandbox() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "scenario-refusal-"));
  const bin = path.join(dir, "bin");
  fs.mkdirSync(bin);
  const stub = (name, body) =>
    fs.writeFileSync(path.join(bin, name), `#!/bin/sh\n${body}\n`, {
      mode: 0o755,
    });
  return { dir, bin, stub, touched: path.join(dir, "touched") };
}

function runLeaf(leaf, sandbox) {
  return spawnSync(
    "bash",
    [findLeaf(leaf), "--go", "/bin/true", "/bin/true"],
    {
      encoding: "utf8",
      env: {
        ...process.env,
        PATH: `${sandbox.bin}:${process.env.PATH ?? ""}`,
        TMPDIR: sandbox.dir,
      },
    },
  );
}

for (const leaf of ["chat-close-pill-type.sh", "pill-two-displays.sh"]) {
  test(
    `${leaf} refuses a running TextEdit before it builds or touches anything`,
    { skip: process.platform === "win32" && "needs bash and a pgrep stub" },
    () => {
      const box = sandbox();
      box.stub(
        "pgrep",
        '[ "$1" = "-x" ] && [ "$2" = "TextEdit" ] && exit 0; exit 1',
      );
      for (const tool of ["swiftc", "osascript", "open", "pkill", "killall"]) {
        box.stub(tool, `echo ${tool} >> ${JSON.stringify(box.touched)}; exit 0`);
      }

      const run = runLeaf(leaf, box);
      const touched = fs.existsSync(box.touched)
        ? fs.readFileSync(box.touched, "utf8")
        : "";
      fs.rmSync(box.dir, { recursive: true, force: true });

      assert.equal(
        run.status,
        1,
        `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
      );
      assert.match(run.stderr, REFUSAL);
      assert.equal(touched, "", `the refused run still ran ${touched}`);
    },
  );

  test(
    `${leaf} leaves a TextEdit that appears after the refusal alone when the build fails`,
    { skip: process.platform === "win32" && "needs bash and a pgrep stub" },
    () => {
      const box = sandbox();
      const calls = path.join(box.dir, "pgrep-calls");
      const standin = spawn("sleep", ["30"], { stdio: "ignore" });
      try {
        // The refusal sees no TextEdit; from then on a stand-in answers to it.
        box.stub(
          "pgrep",
          [
            `n=$(cat ${JSON.stringify(calls)} 2>/dev/null || echo 0)`,
            "n=$((n + 1))",
            `echo "$n" > ${JSON.stringify(calls)}`,
            '[ "$n" -eq 1 ] && exit 1',
            `[ "$1" = "-x" ] && [ "$2" = "TextEdit" ] && { echo ${standin.pid}; exit 0; }`,
            "exit 1",
          ].join("\n"),
        );
        box.stub("swiftc", `echo swiftc >> ${JSON.stringify(box.touched)}; exit 1`);
        for (const tool of ["osascript", "open", "pkill", "killall"]) {
          box.stub(tool, `echo ${tool} >> ${JSON.stringify(box.touched)}; exit 0`);
        }

        const run = runLeaf(leaf, box);
        const touched = fs.existsSync(box.touched)
          ? fs.readFileSync(box.touched, "utf8")
          : "";
        const lookups = fs.existsSync(calls)
          ? fs.readFileSync(calls, "utf8").trim()
          : "0";
        let alive = true;
        try {
          process.kill(standin.pid, 0);
        } catch {
          alive = false;
        }
        fs.rmSync(box.dir, { recursive: true, force: true });

        assert.equal(
          run.status,
          1,
          `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
        );
        assert.match(run.stderr, /could not build ax/);
        assert.equal(touched.trim(), "swiftc", `the run also ran ${touched}`);
        assert.equal(lookups, "1", "cleanup looked TextEdit up by name");
        assert.ok(alive, "cleanup ended a process the run did not start");
      } finally {
        standin.kill();
      }
    },
  );
}
