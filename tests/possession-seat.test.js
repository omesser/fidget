// The bindings card is seated in the display's usable area, and it stays there
// while the sprite walks. Drives the real src/index.html and main.js.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

// A menu bar (or a top taskbar) occupies the top of the frame. The overlay
// covers that strip; the card has to start below it.
const MENU_BAR = 30;
const INSET = 16;

const workArea = { x: 0, y: MENU_BAR, width: 1400, height: 900 - MENU_BAR };

function possessed(x, extra) {
  return {
    id: "driver",
    character: "Buddy Bot",
    animation: "idle",
    frame_index: 0,
    x,
    y: 500,
    width: 64,
    height: 64,
    mirror: 1,
    bubble: true,
    possessed: true,
    paused: false,
    charge: 0.4,
    ...extra,
  };
}

const charging = {
  sprites: [possessed(120)],
  visible: true,
  fade_ms: 0,
  sound: false,
  hover_delay_ms: 1,
  work_area: workArea,
};

const walked = {
  ...charging,
  sprites: [possessed(640)],
};

const paused = {
  ...charging,
  sprites: [possessed(640, { paused: true, charge: null })],
};

let probe = null;

function drive() {
  probe ??= launch();
  return probe;
}

async function launch() {
  const stub = `
<script type="module">
  const handlers = {};
  const waiters = [];
  const until = (ready) =>
    new Promise((resolve) => {
      const check = () => (ready() ? (resolve(), true) : false);
      if (!check()) waiters.push(check);
    });
  const wake = () => {
    for (let i = waiters.length - 1; i >= 0; i -= 1) if (waiters[i]()) waiters.splice(i, 1);
  };
  // A walk that does not move the card sends no new rect report, so the
  // waiter has to notice the sprite's transform on a frame of its own.
  const pump = () => {
    wake();
    requestAnimationFrame(pump);
  };
  requestAnimationFrame(pump);
  window.__invoked = [];
  window.__TAURI__ = {
    core: {
      invoke(name, args) {
        window.__invoked.push({ name, args: structuredClone(args) });
        queueMicrotask(wake);
        if (name === "character") return Promise.resolve({ characters: {} });
        if (name === "overlay_hit_tests_hotspots") return Promise.resolve(true);
        return Promise.resolve();
      },
    },
    event: {
      listen(name, handler) {
        handlers[name] = handler;
        queueMicrotask(wake);
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "overlay-test" }) },
  };
  const box = (el) => {
    const r = el.getBoundingClientRect();
    return [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)];
  };
  const reports = () => window.__invoked.filter((c) => c.name === "overlay_rects").map((c) => c.args.rects);
  const last = () => reports().at(-1) ?? null;
  const cardEl = () => document.querySelector(".possession.visible");
  async function drive() {
    try {
      console.log("__PROBE_READY__");
      await until(() => handlers.frame);
      handlers.frame({ payload: ${JSON.stringify(charging)} });
      await until(() => cardEl() && last());
      const chargingCard = box(cardEl());
      const chargingReported = last();
      handlers.frame({ payload: ${JSON.stringify(walked)} });
      await until(() => (document.querySelector(".sprite")?.style.transform || "").includes("640"));
      const walkedCard = box(cardEl());
      const walkedReported = last();
      handlers.frame({ payload: ${JSON.stringify(paused)} });
      await until(() => cardEl()?.classList.contains("paused") && last());
      const pausedCard = box(cardEl());
      const pausedReported = last();
      console.log("__PROBE_RESULT__" + JSON.stringify({
        chargingCard,
        chargingReported,
        walkedCard,
        walkedReported,
        pausedCard,
        pausedReported,
      }));
    } catch (err) {
      console.log("__PROBE_RESULT__" + JSON.stringify({ error: String(err && err.stack || err) }));
    }
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "index.html"), "utf8").replace("<head>", `<head>${stub}`);
  return probePage(html, { args: ["--window-size=1400,900"], timeout: 8000 });
}

function reportedFor(rects, measured) {
  return rects.find(
    (row) =>
      row.rect[0] === measured[0] &&
      row.rect[1] === measured[1] &&
      row.rect[2] === measured[2] &&
      row.rect[3] === measured[3],
  );
}

test(
  "the bindings card sits below a top menu bar and takes a click only while paused",
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const result = await drive();
    assert.equal(result.chargingCard[0], INSET, "left inset of the usable area");
    assert.equal(result.chargingCard[1], MENU_BAR + INSET, "below the menu bar");
    assert.deepEqual(result.walkedCard, result.chargingCard, "a walk does not move the card");

    const charging = reportedFor(result.chargingReported, result.chargingCard);
    assert.ok(charging, `charging card reported in ${JSON.stringify(result.chargingReported)}`);
    assert.equal(charging.clickable, false, "an active card lets a sprite under it be grabbed");
    assert.equal(charging.follows, undefined, "the card does not ride the sprite");

    const walked = reportedFor(result.walkedReported, result.walkedCard);
    assert.ok(walked, "the card is still reported after the sprite moves");
    assert.equal(walked.follows, undefined);
    assert.deepEqual(walked.rect, charging.rect);

    assert.equal(result.pausedCard[1], MENU_BAR + INSET, "pause keeps the same seat");
    const pausedRow = reportedFor(result.pausedReported, result.pausedCard);
    assert.ok(pausedRow, `paused card reported in ${JSON.stringify(result.pausedReported)}`);
    assert.equal(pausedRow.clickable, true, "paused is the resume click");
    assert.equal(pausedRow.follows, undefined);
  },
);
