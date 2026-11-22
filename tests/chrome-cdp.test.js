import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { launchWithPipe, createPageTarget, cleanupChrome } from "./chrome-cdp.js";
import { tempDir } from "./temp-path.js";

// A stand-in for Chrome speaks CDP over fds 3 and 4. It ignores every call
// until `readyAfterMs`, like a Chrome still starting, then answers only the
// methods in `answers`. Anything else gets no reply.
function fakeChrome(t, readyAfterMs, answers, { stderr = "", closes = true, ignoresTerm = false } = {}) {
  const bin = join(tempDir(t, "fake-chrome-"), "chrome");
  writeFileSync(bin, `#!/usr/bin/env node
const { Socket } = require("node:net");
const answers = ${JSON.stringify(answers)};
const out = new Socket({ fd: 4, readable: false });
const input = new Socket({ fd: 3, writable: false }).setEncoding("utf8");
process.stderr.write(${JSON.stringify(stderr)});
${ignoresTerm ? 'process.on("SIGTERM", () => {});' : ""}
let buffer = "";
const queue = [];
function reply(msg) {
  if (msg.method === "Browser.close" && ${closes}) process.exit(0);
  const { TMPDIR, TMP, TEMP } = process.env;
  const profile = process.argv.find((arg) => arg.startsWith("--user-data-dir="));
  const result = msg.method === "Fake.env" ? { TMPDIR, TMP, TEMP, profile } : answers[msg.method];
  if (result) out.write(JSON.stringify({ id: msg.id, result }) + "\\0");
}
input.on("data", (chunk) => {
  buffer += chunk;
  const parts = buffer.split("\\0");
  buffer = parts.pop();
  for (const part of parts) {
    if (part) queue.push(JSON.parse(part));
  }
});
setTimeout(() => setInterval(() => { while (queue.length) reply(queue.shift()); }, 5), ${readyAfterMs});
setInterval(() => {}, 1000);
`);
  chmodSync(bin, 0o755);
  return bin;
}

const PAGE = {
  "Browser.getVersion": { product: "Fake/1" },
  "Target.createTarget": { targetId: "T1" },
  "Target.attachToTarget": { sessionId: "S1" },
  "Runtime.enable": {},
  "Log.enable": {},
  "Page.enable": {},
};

test("a Chrome that has not answered its first call within 3s fails with its stderr", { timeout: 20000 }, async (t) => {
  const cdp = launchWithPipe(fakeChrome(t, 3100, PAGE, { stderr: "still loading fonts" }), []);
  try {
    const started = Date.now();
    await assert.rejects(createPageTarget(cdp), /Target\.createTarget: CDP call timeout after 3000ms.*still loading fonts/);
    assert.ok(Date.now() - started < 3500);
  } finally {
    await cleanupChrome(cdp);
  }
});

test("a Chrome that answers at once gets a page", { timeout: 20000 }, async (t) => {
  const cdp = launchWithPipe(fakeChrome(t, 0, PAGE), []);
  try {
    assert.equal((await createPageTarget(cdp)).sessionId, "S1");
  } finally {
    await cleanupChrome(cdp);
  }
});

test("a Chrome that exits while starting fails at once with its exit", { timeout: 20000 }, async (t) => {
  const bin = join(tempDir(t, "fake-chrome-"), "chrome");
  writeFileSync(bin, "#!/bin/sh\necho 'no display' >&2\nsleep 0.2\nexit 3\n");
  chmodSync(bin, 0o755);
  const started = Date.now();
  const cdp = launchWithPipe(bin, []);
  try {
    await assert.rejects(createPageTarget(cdp), /Chrome exited with code 3.*no display|Chrome pipe closed/);
    assert.ok(Date.now() - started < 2000);
  } finally {
    await cleanupChrome(cdp);
  }
});

test("Chrome's profile and every temp dir variable point inside its launch dir", { timeout: 20000 }, async (t) => {
  const cdp = launchWithPipe(fakeChrome(t, 0, PAGE), []);
  try {
    const tmp = join(cdp.dir, "tmp");
    assert.deepEqual(await cdp.send("Fake.env"), {
      TMPDIR: tmp,
      TMP: tmp,
      TEMP: tmp,
      profile: `--user-data-dir=${join(cdp.dir, "profile")}`,
    });
  } finally {
    await cleanupChrome(cdp);
  }
});

for (const [name, fake, signal] of [
  ["closes over CDP without a signal", { closes: true }, null],
  ["sends SIGTERM when Browser.close is ignored", { closes: false }, "SIGTERM"],
  ["force-kills only a Chrome that also ignores SIGTERM", { closes: false, ignoresTerm: true }, "SIGKILL"],
]) {
  test(`cleanup ${name}, and removes the launch dir`, { timeout: 20000 }, async (t) => {
    const cdp = launchWithPipe(fakeChrome(t, 0, PAGE, fake), []);
    await createPageTarget(cdp);
    await cleanupChrome(cdp);
    assert.equal(cdp.proc.signalCode, signal);
    assert.equal(existsSync(cdp.dir), false);
  });
}

// e2e testing a hard-killed Chrome's launch-dir cleanup is not deterministic:
// Chrome does not always write temp or cache files into the launch dir at
// startup (seen with macOS Google Chrome headless, #2031).
test("cleanup removes a launch dir with nested files after a hard kill", { timeout: 20000 }, async (t) => {
  const cdp = launchWithPipe(fakeChrome(t, 0, PAGE), []);
  const nested = join(cdp.dir, "tmp", "nested");
  mkdirSync(nested, { recursive: true });
  const planted = join(nested, "planted.txt");
  writeFileSync(planted, "junk");
  assert.equal(existsSync(planted), true);
  cdp.proc.kill("SIGKILL");
  await cleanupChrome(cdp);
  assert.equal(cdp.proc.signalCode, "SIGKILL");
  assert.equal(existsSync(cdp.dir), false);
});
