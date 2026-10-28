// The X11 mask rebuild measurement must end when it is interrupted, with its
// app gone and no results printed. A fake Fidget stands in, so nothing shows:
// it writes its lines to the process log, as the app does, and DISPLAY is
// unset so the pointer stays put.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { tempDir } from "./temp-path.js";

const script = path.join(import.meta.dirname, "..", "scripts", "measure-mask-rebuild-x11.sh");
const ready = "overlay: 1 display";
const trace = "mask_rebuild: 126x128 @1x scale, 6300 opaque pixels, 0.5 ms";
const walking = "frame: 1 Grounded pos(0,0) sprite(0,0) walk#0 id";
const standing = "frame: 1 Grounded pos(0,0) sprite(0,0) idle#0 id";

function fakeFidget(dir, lines) {
  const bin = path.join(dir, "fidget");
  const prints = lines.map((line) => `echo "1 ${line}" >> "$FIDGET_DATA_DIR/process.log"\n`).join("");
  fs.writeFileSync(bin, `#!/usr/bin/env bash\necho $$ > "${dir}/pid.tmp" && mv "${dir}/pid.tmp" "${dir}/app.pid"\n${prints}exec sleep 30\n`, {
    mode: 0o755,
  });
  return bin;
}

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

// Each case gets 3 s; a hang or a cleanup that misses the app fails fast.
const budgetMs = 3000;

// Runs the script against a fake that prints `lines`, sending `signal` once
// `cue` shows and the fake has its pid down. TMPDIR keeps files in `dir`.
async function measure(t, { lines, args, signal, cue }) {
  const dir = tempDir(t, "mask-measure-");
  const pidFile = path.join(dir, "app.pid");
  const child = spawn("bash", [script, "--bin", fakeFidget(dir, lines), ...args], {
    env: { ...process.env, TMPDIR: dir, DISPLAY: "" },
  });
  let out = "";
  let afterSignal = "";
  let sent = false;
  let started = false;
  let timer;
  const watcher = fs.watch(dir, () => {
    started ||= fs.existsSync(pidFile);
    fire();
  });
  function fire() {
    if (signal && !sent && started && out.includes(cue)) {
      sent = true;
      child.kill(signal);
    }
  }
  const exited = new Promise((resolve) => child.on("exit", (code, sig) => resolve({ code, sig })));
  const budget = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`no exit within ${budgetMs} ms:\n${out}`)), budgetMs);
  });
  child.stdout.on("data", (chunk) => {
    if (sent) afterSignal += chunk;
    out += chunk;
    started ||= fs.existsSync(pidFile);
    fire();
  });
  let pid;
  try {
    const result = await Promise.race([exited, budget]);
    pid = Number(fs.readFileSync(pidFile, "utf8"));
    return { ...result, out, afterSignal, appAlive: alive(pid) };
  } finally {
    clearTimeout(timer);
    watcher.close();
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
    if (pid === undefined && fs.existsSync(pidFile)) pid = Number(fs.readFileSync(pidFile, "utf8"));
    if (pid && alive(pid)) process.kill(pid, "SIGKILL");
  }
}

function assertInterrupted(r) {
  assert.strictEqual(r.code, 130, r.out);
  assert.strictEqual(r.appAlive, false, "the app outlived the script");
  assert.strictEqual(r.afterSignal, "", "output followed the signal");
}

for (const signal of ["SIGINT", "SIGTERM"]) {
  test(`${signal} ends the measurement with status 130 and stops the app`, async (t) => {
    const cue = "Waiting for 100 mask updates";
    assertInterrupted(await measure(t, { lines: [ready, standing, trace], args: ["idle", "100", "2"], signal, cue }));
  });

  test(`${signal} during startup ends the run with status 130 and stops the app`, async (t) => {
    const cue = "Waiting for overlay ready...";
    assertInterrupted(await measure(t, { lines: [], args: ["idle", "100", "2"], signal, cue }));
  });
}

test("updates with none position-only report every one as a full rebuild", async (t) => {
  const r = await measure(t, { lines: [ready, walking, trace], args: ["walk", "1", "2"] });
  assert.strictEqual(r.code, 0, r.out);
  assert.match(r.out, /Full rebuilds: 1 /);
  assert.match(r.out, /Position-only: 0 /);
  assert.match(r.out, /Full rebuild ratio: 1\.00 /);
  assert.match(r.out, /Full rebuilds: 100% of updates/);
});

test("opaque pixel count histogram groups by pixel count", async (t) => {
  const mixedLines = [
    ready,
    walking,
    "mask_rebuild: 126x128 @1x scale, 6300 opaque pixels, 0.5 ms",
    "mask_rebuild: 37x33 @3x scale, 500 opaque pixels, 0.3 ms",
    "mask_rebuild: 126x128 @1x scale, 6300 opaque pixels, 0.6 ms (position-only)",
    "mask_rebuild: 37x33 @3x scale, 500 opaque pixels, 0.4 ms",
  ];
  const r = await measure(t, { lines: mixedLines, args: ["walk", "4", "2"] });
  assert.strictEqual(r.code, 0, r.out);
  assert.match(r.out, /Opaque pixel count:/);
  assert.match(r.out, /  6300 pixels: 2 update\(s\)/);
  assert.match(r.out, /  500 pixels: 2 update\(s\)/);
});

test("updates that land while another animation plays are left out", async (t) => {
  const lines = [
    ready,
    walking,
    "mask_rebuild: 126x128 @1x scale, 6300 opaque pixels, 0.5 ms",
    standing,
    "mask_rebuild: 126x128 @1x scale, 999 opaque pixels, 9.0 ms",
    walking,
    "mask_rebuild: 126x128 @1x scale, 6300 opaque pixels, 0.7 ms",
  ];
  const r = await measure(t, { lines, args: ["walk", "2", "2"] });
  assert.strictEqual(r.code, 0, r.out);
  assert.match(r.out, /^summary: scenario=walk updates=2 /m);
  assert.doesNotMatch(r.out, /999 pixels/);
});
