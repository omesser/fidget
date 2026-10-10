// The pill and the bubble, measured on the real overlay. Dots use the same
// box as a line, and the pill has to step clear of both. Drives src/index.html
// the way overlay-rects.test.js does.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

function chromeBin() {
  if (process.env.FIDGET_CHROME) {
    return process.env.FIDGET_CHROME;
  }
  for (const name of ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]) {
    const found = spawnSync("bash", ["-lc", `command -v ${name}`], { encoding: "utf8" });
    const path = found.stdout.trim();
    if (found.status === 0 && path) {
      return path;
    }
  }
  return null;
}

const chrome = chromeBin();

const opening = {
  name: "Buddy Bot",
  configured: true,
  enabled: true,
  login: null,
  harness: null,
};

function drive() {
  const stub = `
<script type="module">
  const handlers = {};
  // Virtual time runs the first animation frame and then leaves later ones
  // queued, which sticks the overlay's single armed frame. Drain the queue
  // after each wait so a placement still draws.
  const queued = [];
  const nativeFrame = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (fn) => {
    const task = (t) => {
      if (task.ran) return;
      task.ran = true;
      const index = queued.indexOf(task);
      if (index >= 0) queued.splice(index, 1);
      fn(t);
    };
    queued.push(task);
    return nativeFrame(task);
  };
  const drain = () => {
    const batch = queued.splice(0, queued.length);
    for (const task of batch) task(performance.now());
  };
  window.__TAURI__ = {
    core: {
      invoke(name) {
        if (name === "character") return Promise.resolve({ characters: {} });
        if (name === "overlay_hit_tests_hotspots") return Promise.resolve(true);
        if (name === "chat_opening") return Promise.resolve(${JSON.stringify(opening)});
        return Promise.resolve();
      },
    },
    event: {
      listen(name, handler) {
        handlers[name] = handler;
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "overlay-test" }) },
  };
  const tick = () => new Promise((r) => setTimeout(r, 50)).then(drain);
  const spriteOf = (x, y, extra) => ({
    id: "bmo",
    character: "Buddy Bot",
    animation: "idle",
    frame_index: 0,
    x, y, width: 64, height: 64, mirror: 1, bubble: true,
    ...(extra || {}),
  });
  const frame = (x, y, extra, shown) => ({
    sprites: [spriteOf(x, y, extra)],
    visible: shown,
    fade_ms: 0,
    sound: false,
  });
  const sprite = () => document.querySelector(".sprite");
  const bubble = () => document.querySelector(".bubble:not(.quick-message)");
  const pill = () => document.querySelector(".quick-message");
  const field = () => document.querySelector(".quick-message-field");
  function box(el) {
    const r = el.getBoundingClientRect();
    return {
      x: Math.round(r.x), y: Math.round(r.y),
      w: Math.round(r.width), h: Math.round(r.height),
      visible: el.classList.contains("visible"),
      mode: el.dataset.mode || null,
    };
  }
  function hit(a, b) {
    if (!a.visible || !b.visible || a.w <= 0 || b.w <= 0) return false;
    return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
  }
  async function until(pred, why) {
    for (let i = 0; i < 80; i += 1) {
      if (pred()) return;
      await tick();
    }
    throw new Error(why + " bubble=" + (bubble()?.className || "none") + "/" + (bubble()?.dataset.mode || "-"));
  }
  let stamp = 0;
  function emit(x, y, extra, shown = true) {
    handlers.frame({ payload: frame(x, y, { ...(extra || {}), stamp: ++stamp }, shown) });
  }
  async function blank() {
    emit(400, 300, null, false);
    await until(() => bubble() && !bubble().classList.contains("visible"), "blank");
  }
  async function speech(x, y) {
    emit(x, y, { dialogue: "hello there" });
    await until(
      () => bubble().classList.contains("visible") && bubble().dataset.mode === "speech",
      "speech",
    );
  }
  async function dots(x, y) {
    emit(x, y, { thinking: true });
    await until(
      () => bubble().classList.contains("visible") && bubble().dataset.mode === "thinking",
      "dots",
    );
  }
  async function idle(x, y) {
    emit(x, y, null);
    await until(() => sprite() && field() && !field().disabled, "idle");
  }
  async function openPill() {
    sprite().dispatchEvent(new PointerEvent("pointerleave"));
    sprite().dispatchEvent(new PointerEvent("pointerenter"));
    await until(() => pill().classList.contains("visible"), "pill");
  }
  function snap(name, expect) {
    const b = box(bubble());
    const p = box(pill());
    return { name, expect, bubble: b, pill: p, overlap: hit(b, p) };
  }
  async function at(seat, x, y) {
    const rows = [];
    await blank();
    await speech(x, y);
    await openPill();
    rows.push(snap(seat + " text then pill", "speech"));

    await blank();
    await dots(x, y);
    await openPill();
    rows.push(snap(seat + " dots then pill", "thinking"));

    await blank();
    await idle(x, y);
    await openPill();
    await speech(x, y);
    rows.push(snap(seat + " text while pill open", "speech"));

    await blank();
    await idle(x, y);
    await openPill();
    await dots(x, y);
    rows.push(snap(seat + " dots while pill open", "thinking"));

    await blank();
    await idle(x, y);
    await openPill();
    field().value = "hey";
    field().dispatchEvent(new Event("input", { bubbles: true }));
    document.querySelector(".quick-message-send").click();
    await until(
      () => bubble().classList.contains("visible") && bubble().dataset.mode === "thinking" && !pill().classList.contains("visible"),
      "send",
    );
    sprite().dispatchEvent(new PointerEvent("pointerleave"));
    sprite().dispatchEvent(new PointerEvent("pointerenter"));
    await until(
      () => pill().classList.contains("visible") && bubble().dataset.mode === "thinking",
      "rehover",
    );
    rows.push(snap(seat + " send then rehover over dots", "thinking"));
    return rows;
  }
  async function run() {
    while (!handlers.frame || window.innerWidth < 100) await tick();
    const cases = [];
    for (const [seat, y] of [["mid", 300], ["top", 0]]) {
      cases.push(...await at(seat, 400, y));
    }
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify({ cases });
    document.body.append(out);
  }
  window.addEventListener("load", () => run().catch((why) => {
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify({ error: String(why), cases: [] });
    document.body.append(out);
  }));
</script>`;

  const dir = mkdtempSync(join(tmpdir(), "qm-bubble-clear-"));
  const page = join(dir, "harness.html");
  const html = readFileSync(join(SRC, "index.html"), "utf8").replace(
    "<head>",
    `<head><base href="${pathToFileURL(SRC).href}">${stub}`,
  );
  writeFileSync(page, html);

  const run = spawnSync(
    chrome,
    [
      "--headless",
      "--disable-gpu",
      "--no-sandbox",
      "--disable-dev-shm-usage",
      `--user-data-dir=${join(dir, "profile")}`,
      "--allow-file-access-from-files",
      "--window-size=1000,700",
      "--virtual-time-budget=30000",
      "--dump-dom",
      pathToFileURL(page).href,
    ],
    { encoding: "utf8", maxBuffer: 1 << 24, timeout: 90000, killSignal: "SIGKILL" },
  );
  const match = run.stdout.match(/<pre id="probe"[^>]*>(.*?)<\/pre>/s);
  assert.ok(match, `the overlay did not report. ${run.stderr?.slice(-800) ?? ""} status=${run.status}`);
  return JSON.parse(
    match[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">"),
  );
}

const skip = chrome ? false : "headless Chromium is not installed";

test("the pill steps clear of a speech bubble and of the thinking dots", { skip, timeout: 120000 }, () => {
  const report = drive();
  assert.equal(report.error, undefined, report.error);
  assert.equal(report.cases.length, 10);
  for (const row of report.cases) {
    assert.equal(row.bubble.visible, true, row.name);
    assert.equal(row.pill.visible, true, row.name);
    assert.equal(row.bubble.mode, row.expect, `${row.name} ${JSON.stringify(row)}`);
    assert.equal(row.overlap, false, `${row.name} ${JSON.stringify(row)}`);
  }
});
