// A scenario leaf's cleanup may end only processes the run started: the app by
// pid, and the Harness by a command line holding this run's count= path. A
// kill by app name could reach the user's own.

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

const scenarios = path.join(import.meta.dirname, "..", "scripts", "scenarios");

// A pid picked out of a name lookup is still a name kill.
const NAME_LOOKUPS = /\bpgrep\b|Get-Process\s+-Name\b|GetProcessesByName\b/i;
const KILLS = /\b(?:kill|pkill|killall|Stop-Process|taskkill)\b|\.Kill\(/i;

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

function functionSource(script, name) {
  const text = fs.readFileSync(findLeaf(script), "utf8");
  const body = text.match(new RegExp(`^${name}\\(\\) \\{[\\s\\S]*?\\n\\}`, "m"));
  assert.ok(body, `no ${name}() in ${script}`);
  return body[0];
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

test("capture_textedit_pid looks up the pid once after open", () => {
  for (const name of ["chat-close-pill-type.sh", "pill-two-displays.sh"]) {
    const body = functionSource(name, "capture_textedit_pid");
    assert.doesNotMatch(body, /\bsleep\b/, name);
    assert.equal((body.match(/\bpgrep\b/g) || []).length, 1, name);
  }
});

test("case 7 quits through quit_textedit, which has no fixed sleep", () => {
  const leaf = "chat-close-pill-type.sh";
  const text = fs.readFileSync(findLeaf(leaf), "utf8");
  const case7 = text.slice(text.indexOf("# 7."), text.indexOf("# 8."));
  assert.match(case7, /^quit_textedit$/m);
  assert.doesNotMatch(case7, /textedit_pid=|\bsleep\b/);
  assert.doesNotMatch(functionSource(leaf, "quit_textedit"), /\bsleep\b/);
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

// kill(pid, 0) still succeeds while the child is a zombie, so wait until
// this process reaps it. A live stand-in does not exit within the wait.
async function reaped(child) {
  if (child.exitCode !== null || child.signalCode !== null) return true;
  let timer;
  const pending = new Promise((resolve) => {
    timer = setTimeout(() => resolve(false), 200);
  });
  const exited = once(child, "exit").then(() => {
    clearTimeout(timer);
    return true;
  });
  return Promise.race([exited, pending]);
}

// The leaf's own watchdog walks children with pgrep -P. Answer that from the
// real pgrep so the watchdog can end its sleep; otherwise every capped call
// waits out its whole 3 s inside the command substitution.
const REAL_PGREP = '[ "$1" = "-P" ] && exec /usr/bin/pgrep "$@"';

function runLeaf(leaf, sandbox) {
  return spawnSync(
    "bash",
    [findLeaf(leaf), "--go", "/bin/true", "/bin/true"],
    {
      encoding: "utf8",
      env: {
        ...process.env,
        // Unix tools only after the stubs. A Windows `head` earlier on PATH
        // keeps the captured pid from being the stand-in's, so the kill misses.
        PATH: `${sandbox.bin}:/usr/bin:/bin`,
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

  // The builds and the Accessibility preflight sit between the first refusal
  // and the run's own open. A TextEdit that appears in that gap is the user's.
  test(
    `${leaf} refuses a TextEdit that appears after the first check and before open`,
    { skip: process.platform === "win32" && "needs bash and a pgrep stub" },
    async () => {
      const box = sandbox();
      const calls = path.join(box.dir, "pgrep-calls");
      const standin = spawn("sleep", ["30"], { stdio: "ignore" });
      try {
        box.stub(
          "pgrep",
          [
            REAL_PGREP,
            '[ "$1" = "-x" ] && [ "$2" = "TextEdit" ] || exit 1',
            `n=$(cat ${JSON.stringify(calls)} 2>/dev/null || echo 0)`,
            "n=$((n + 1))",
            `echo "$n" > ${JSON.stringify(calls)}`,
            '[ "$n" -eq 1 ] && exit 1',
            `echo ${standin.pid}`,
            "exit 0",
          ].join("\n"),
        );
        stubSwiftc(box);
        for (const tool of ["osascript", "open", "pkill", "killall"]) {
          box.stub(tool, `echo ${tool} >> ${JSON.stringify(box.touched)}; exit 1`);
        }

        const run = runLeaf(leaf, box);
        const touched = fs.existsSync(box.touched)
          ? fs.readFileSync(box.touched, "utf8")
          : "";
        const ended = await reaped(standin);
        fs.rmSync(box.dir, { recursive: true, force: true });

        assert.equal(
          run.status,
          1,
          `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
        );
        assert.match(run.stderr, REFUSAL);
        assert.equal(touched, "", `the refused run still ran ${touched}`);
        assert.equal(ended, false, "the run ended a TextEdit it did not start");
      } finally {
        try {
          standin.kill();
        } catch {
          // The assertion above already failed if the run ended it.
        }
      }
    },
  );

  // The stand-in starts only when the stubbed open runs, then a probe fails.
  // Cleanup has to end that process, and only because the run started it.
  test(
    `${leaf} ends the TextEdit its own open started when a later probe fails`,
    { skip: process.platform === "win32" && "needs bash and a pgrep stub" },
    async () => {
      const box = sandbox();
      const opened = path.join(box.dir, "opened");
      const standin = spawn("sleep", ["30"], { stdio: "ignore" });
      try {
        box.stub(
          "pgrep",
          [
            REAL_PGREP,
            '[ "$1" = "-x" ] && [ "$2" = "TextEdit" ] || exit 1',
            `[ -f ${JSON.stringify(opened)} ] || exit 1`,
            `echo ${standin.pid}`,
            "exit 0",
          ].join("\n"),
        );
        stubSwiftc(box);
        box.stub(
          "open",
          `echo open >> ${JSON.stringify(box.touched)}; : > ${JSON.stringify(opened)}; exit 0`,
        );
        box.stub(
          "osascript",
          [
            `echo osascript >> ${JSON.stringify(box.touched)}`,
            'case "$*" in',
            '*"count every document"*) echo 0; exit 0 ;;',
            '*"whose path is"*scenario-one.txt*) exit 0 ;;',
            "esac",
            "exit 1",
          ].join("\n"),
        );
        for (const tool of ["pkill", "killall"]) {
          box.stub(tool, `echo ${tool} >> ${JSON.stringify(box.touched)}; exit 0`);
        }

        const run = runLeaf(leaf, box);
        const touched = fs.existsSync(box.touched)
          ? fs.readFileSync(box.touched, "utf8")
          : "";
        const ended = await reaped(standin);
        fs.rmSync(box.dir, { recursive: true, force: true });

        assert.equal(
          run.status,
          1,
          `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
        );
        assert.match(run.stderr, /no answer from TextEdit/);
        assert.match(touched, /^open\n/);
        assert.equal(ended, true, "cleanup left the TextEdit this run started");
      } finally {
        try {
          standin.kill();
        } catch {
          // Cleanup already ended it, which is what the test wants.
        }
      }
    },
  );
}

// The count after closing this run's paths is the whole decision. Zero may
// quit. Anything else, including a failed count, leaves TextEdit running.
const BLOCKED_LEFT_RUNNING =
  /TextEdit has another document open, so this run left it running\./;
const UNKNOWN_LEFT_RUNNING =
  /TextEdit's documents could not be listed, so this run left it running\./;

function runOwnedCleanup(leaf, box, standin, { fn, owned, osascript }) {
  const log = path.join(box.dir, "osascript.log");
  box.stub(
    "osascript",
    [`echo "$*" >> ${JSON.stringify(log)}`, osascript].join("\n"),
  );
  const script = path.join(box.dir, "cleanup.sh");
  const lines = [
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    `. ${JSON.stringify(path.join(scenarios, "helpers", "wait-helpers.mac.sh"))}`,
    'fail() { echo "FAIL: $*" >&2; exit 1; }',
    'capped() { local seconds=$1; shift; "$@"; }',
    `out=${JSON.stringify(box.dir)}`,
    `textedit_pid=${standin.pid}`,
    "textedit_opened=()",
  ];
  if (owned) lines.push(`textedit_opened+=(${JSON.stringify(owned)})`);
  lines.push(functionSource(leaf, fn), fn, "");
  fs.writeFileSync(script, lines.join("\n"), { mode: 0o755 });
  const run = spawnSync("bash", [script], {
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${box.bin}:/usr/bin:/bin`,
      TMPDIR: box.dir,
    },
  });
  const logged = fs.existsSync(log) ? fs.readFileSync(log, "utf8") : "";
  return { run, logged };
}

for (const leaf of ["chat-close-pill-type.sh", "pill-two-displays.sh"]) {
  test(
    `close_textedit in ${leaf} closes only the run's document and does not quit while another document is open`,
    { skip: process.platform === "win32" && "needs bash and an osascript stub" },
    async () => {
      const box = sandbox();
      const owned = path.join(box.dir, "scenario-one.txt");
      const standin = spawn("sleep", ["30"], { stdio: "ignore" });
      try {
        const { run, logged } = runOwnedCleanup(leaf, box, standin, {
          fn: "close_textedit",
          owned,
          osascript: [
            'case "$*" in',
            '*"count every document"*) echo 1; exit 0 ;;',
            "esac",
            "exit 0",
          ].join("\n"),
        });
        const ended = await reaped(standin);
        fs.rmSync(box.dir, { recursive: true, force: true });

        assert.equal(
          run.status,
          0,
          `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
        );
        assert.equal(ended, false, "cleanup quit TextEdit while another document was open");
        assert.match(run.stderr, BLOCKED_LEFT_RUNNING);
        const closes = logged.split("\n").filter((line) => line.includes("whose path is"));
        assert.equal(closes.length, 1);
        assert.match(closes[0], pathInClose(owned));
        assert.doesNotMatch(logged, /close every document/);
        assert.doesNotMatch(logged, /quit saving no/);
      } finally {
        try {
          standin.kill();
        } catch {
          // Still alive is what this test wants. A dead one already failed above.
        }
      }
    },
  );

  test(
    `close_textedit in ${leaf} quits TextEdit when every open document is one the run opened`,
    { skip: process.platform === "win32" && "needs bash and an osascript stub" },
    async () => {
      const box = sandbox();
      const owned = path.join(box.dir, "scenario-one.txt");
      const standin = spawn("sleep", ["30"], { stdio: "ignore" });
      try {
        const { run, logged } = runOwnedCleanup(leaf, box, standin, {
          fn: "close_textedit",
          owned,
          osascript: [
            'case "$*" in',
            '*"count every document"*) echo 0; exit 0 ;;',
            "esac",
            "exit 0",
          ].join("\n"),
        });
        const ended = await reaped(standin);
        fs.rmSync(box.dir, { recursive: true, force: true });

        assert.equal(
          run.status,
          0,
          `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
        );
        assert.equal(ended, true, "cleanup left TextEdit running with no document open");
        assert.match(logged, pathInClose(owned));
        assert.match(logged, /quit saving no/);
        assert.doesNotMatch(logged, /close every document/);
      } finally {
        try {
          standin.kill();
        } catch {
          // The function's kill already reaped it.
        }
      }
    },
  );

  test(
    `close_textedit in ${leaf} leaves TextEdit running when its documents cannot be listed`,
    { skip: process.platform === "win32" && "needs bash and an osascript stub" },
    async () => {
      const box = sandbox();
      const owned = path.join(box.dir, "scenario-one.txt");
      const standin = spawn("sleep", ["30"], { stdio: "ignore" });
      try {
        const { run, logged } = runOwnedCleanup(leaf, box, standin, {
          fn: "close_textedit",
          owned,
          osascript: [
            'case "$*" in',
            '*"count every document"*) exit 1 ;;',
            "esac",
            "exit 0",
          ].join("\n"),
        });
        const ended = await reaped(standin);
        fs.rmSync(box.dir, { recursive: true, force: true });

        assert.equal(
          run.status,
          0,
          `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
        );
        assert.equal(ended, false, "cleanup quit TextEdit without a document count");
        assert.match(run.stderr, UNKNOWN_LEFT_RUNNING);
        assert.doesNotMatch(logged, /quit saving no/);
      } finally {
        try {
          standin.kill();
        } catch {
          // Still alive is what this test wants.
        }
      }
    },
  );
}

function pathInClose(owned) {
  const escaped = owned.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`whose path is.*${escaped}`);
}

test(
  "quit_textedit refuses before quit or kill when another document is open",
  { skip: process.platform === "win32" && "needs bash and an osascript stub" },
  async () => {
    const box = sandbox();
    const owned = path.join(box.dir, "scenario-one.txt");
    const standin = spawn("sleep", ["30"], { stdio: "ignore" });
    try {
      const { run, logged } = runOwnedCleanup(
        "chat-close-pill-type.sh",
        box,
        standin,
        {
          fn: "quit_textedit",
          owned,
          osascript: [
            'case "$*" in',
            '*"count every document"*) echo 1; exit 0 ;;',
            "esac",
            "exit 0",
          ].join("\n"),
        },
      );
      const ended = await reaped(standin);
      fs.rmSync(box.dir, { recursive: true, force: true });

      assert.equal(
        run.status,
        1,
        `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
      );
      assert.match(
        run.stderr,
        /TextEdit has another document open\. This run will not quit it\./,
      );
      assert.doesNotMatch(logged, /quit saving no/);
      assert.equal(ended, false, "quit_textedit quit TextEdit while another document was open");
    } finally {
      try {
        standin.kill();
      } catch {
        // Still alive is what this test wants.
      }
    }
  },
);

test(
  "quit_textedit quits when the only open document is the run's",
  {
    skip:
      process.platform !== "darwin" &&
      "wait_for_exit watches the pid with caffeinate on macOS",
  },
  () => {
    const box = sandbox();
    const owned = path.join(box.dir, "scenario-one.txt");
    const log = path.join(box.dir, "osascript.log");
    const standin = spawn(
      "/bin/sh",
      [
        "-c",
        "trap 'kill \"$sleeper\" 2> /dev/null; exit 0' TERM; sleep 60 & sleeper=$!; wait",
      ],
      { stdio: "ignore" },
    );
    try {
      box.stub(
        "osascript",
        [
          `echo "$*" >> ${JSON.stringify(log)}`,
          'case "$*" in',
          `*"quit saving no"*) ( sleep 1; kill -TERM ${standin.pid} ) > /dev/null 2>&1 & ;;`,
          '*"count every document"*) echo 0; exit 0 ;;',
          "esac",
          "exit 0",
        ].join("\n"),
      );
      const script = path.join(box.dir, "quit.sh");
      fs.writeFileSync(
        script,
        [
          "#!/usr/bin/env bash",
          "set -euo pipefail",
          `. ${JSON.stringify(path.join(scenarios, "helpers", "wait-helpers.mac.sh"))}`,
          'fail() { echo "FAIL: $*" >&2; exit 1; }',
          'capped() { local seconds=$1; shift; "$@"; }',
          `out=${JSON.stringify(box.dir)}`,
          `textedit_pid=${standin.pid}`,
          "textedit_opened=()",
          `textedit_opened+=(${JSON.stringify(owned)})`,
          functionSource("chat-close-pill-type.sh", "quit_textedit"),
          "quit_textedit",
          'echo "pid=${textedit_pid:-empty}"',
          "",
        ].join("\n"),
        { mode: 0o755 },
      );
      const run = spawnSync("bash", [script], {
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${box.bin}:/usr/bin:/bin`,
          TMPDIR: box.dir,
        },
      });
      const logged = fs.existsSync(log) ? fs.readFileSync(log, "utf8") : "";
      fs.rmSync(box.dir, { recursive: true, force: true });

      assert.equal(
        run.status,
        0,
        `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
      );
      assert.match(run.stdout, /^pid=empty$/m);
      assert.match(logged, /quit saving no/);
      assert.match(logged, pathInClose(owned));
      assert.doesNotMatch(logged, /close every document/);
    } finally {
      standin.kill();
    }
  },
);

test("each TextEdit open records the path it opened", () => {
  function appendsAfterOpens(name) {
    const lines = fs.readFileSync(findLeaf(name), "utf8").split("\n");
    const found = [];
    for (let i = 0; i < lines.length; i++) {
      if (/^\s*#/.test(lines[i])) continue;
      if (!/\bopen\s+(?:-g\s+)?-a\s+TextEdit\b/.test(lines[i])) continue;
      found.push(lines[i + 1]);
    }
    return found;
  }
  assert.deepEqual(appendsAfterOpens("chat-close-pill-type.sh"), [
    'textedit_opened+=("$out/$one")',
    'textedit_opened+=("$out/$two")',
  ]);
  assert.deepEqual(appendsAfterOpens("pill-two-displays.sh"), [
    'textedit_opened+=("$out/$one")',
  ]);
});

// Case 8 refuses a running TextEdit, so case 7's clear has to wait out a quit
// that is still exiting.
test(
  "case 7 waits for TextEdit to exit before clearing its pid",
  {
    skip:
      process.platform !== "darwin" &&
      "wait_for_exit watches the pid with caffeinate on macOS",
  },
  () => {
    const box = sandbox();
    const gone = path.join(box.dir, "textedit-gone");
    const standin = spawn(
      "/bin/sh",
      [
        "-c",
        `trap 'kill "$sleeper" 2> /dev/null; touch ${gone}; exit 0' TERM; sleep 60 & sleeper=$!; wait`,
      ],
      { stdio: "ignore" },
    );
    try {
      box.stub(
        "pgrep",
        [
          '[ "$1" = "-x" ] && [ "$2" = "TextEdit" ] || exit 1',
          `[ -f ${JSON.stringify(gone)} ] && exit 1`,
          `echo ${standin.pid}`,
          "exit 0",
        ].join("\n"),
      );
      // The quit returns at once; TextEdit exits a moment later. A count of
      // zero is what lets that quit run. Any other count leaves the pid set.
      box.stub(
        "osascript",
        [
          'case "$*" in',
          '*"count every document"*) echo 0; exit 0 ;;',
          '*"whose path is"*) exit 0 ;;',
          `*"quit saving no"*) ( sleep 1; kill -TERM ${standin.pid} ) > /dev/null 2>&1 & ;;`,
          "esac",
          "exit 0",
        ].join("\n"),
      );

      // Run the leaf's own functions, in the order case 7 and case 8 use them.
      const script = path.join(box.dir, "case7.sh");
      fs.writeFileSync(
        script,
        [
          "#!/usr/bin/env bash",
          "set -euo pipefail",
          `. ${JSON.stringify(path.join(scenarios, "helpers", "wait-helpers.mac.sh"))}`,
          'fail() { echo "FAIL: $*" >&2; exit 1; }',
          'capped() { local seconds=$1; shift; "$@"; }',
          `out=${JSON.stringify(box.dir)}`,
          `textedit_pid=${standin.pid}`,
          "textedit_opened=()",
          functionSource("chat-close-pill-type.sh", "quit_textedit"),
          functionSource("chat-close-pill-type.sh", "refuse_running_textedit"),
          "quit_textedit",
          "refuse_running_textedit",
          'echo "pid=${textedit_pid:-empty}"',
          "",
        ].join("\n"),
        { mode: 0o755 },
      );

      const run = spawnSync("bash", [script], {
        encoding: "utf8",
        env: {
          ...process.env,
          PATH: `${box.bin}:/usr/bin:/bin`,
          TMPDIR: box.dir,
        },
      });
      fs.rmSync(box.dir, { recursive: true, force: true });

      assert.equal(
        run.status,
        0,
        `stdout:\n${run.stdout}\nstderr:\n${run.stderr}`,
      );
      assert.match(run.stdout, /^pid=empty$/m);
    } finally {
      standin.kill();
    }
  },
);

// A successful swiftc, so a leaf can get past its builds to the open below.
function stubSwiftc(box) {
  box.stub(
    "swiftc",
    [
      "out=",
      'while [ "$#" -gt 0 ]; do',
      '  if [ "$1" = "-o" ]; then out=$2; shift 2; continue; fi',
      "  shift",
      "done",
      '[ -n "$out" ] || exit 1',
      'printf "%s\\n" "#!/bin/sh" "exit 0" > "$out"',
      'chmod +x "$out"',
    ].join("\n"),
  );
}
