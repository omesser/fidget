// A sprite context menu suspends the quick-message dwell. Drives the real
// src/index.html and main.js: a frame's `menu` flag in, pill opacity out.

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { chromePath, chromeSkip, cleanupChrome, createPageTarget, launchWithPipe } from "./chrome-cdp.js";
import { pageClock } from "./page-clock.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
const HOVER_DELAY_MS = 400;

function frame(over = {}) {
  return {
    sprites: [
      {
        id: "bmo",
        character: "Buddy Bot",
        animation: "idle",
        frame_index: 0,
        x: 200,
        y: 200,
        width: 64,
        height: 64,
        mirror: 1,
        bubble: true,
        hovered: true,
        ...over.sprite,
      },
    ],
    visible: true,
    fade_ms: 0,
    sound: true,
    hover_delay_ms: HOVER_DELAY_MS,
    menu: over.menu === true,
  };
}

const stub = `
<script type="module">
  ${pageClock()}
  ${pageWait()}
  const style = document.createElement("style");
  style.textContent = ".bubble{transition:none !important}";
  document.head.append(style);
  const handlers = {};
  window.__TAURI__ = {
    core: {
      invoke(name) {
        if (name === "character") return Promise.resolve({ characters: {} });
        // A ready opening, so a typed draft is text the pill can keep.
        if (name === "chat_opening") {
          return Promise.resolve({ configured: true, enabled: true, name: "Buddy" });
        }
        return Promise.resolve();
      },
    },
    event: {
      listen(name, handler) {
        handlers[name] = handler;
        listeningNow(name);
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "overlay-test" }) },
  };
  const pill = () => document.querySelector('.quick-message[data-instance="bmo"]');
  const field = () => pill().querySelector(".quick-message-field");
  window.__ready = () => ready("frame");
  window.__paint = async (payload) => {
    handlers.frame({ payload });
    await settle();
    const node = pill();
    if (!node) throw new Error("the quick-message pill is missing");
    return getComputedStyle(node).opacity;
  };
  window.__dwell = async () => {
    window.__advance(${HOVER_DELAY_MS});
    const flashed = getComputedStyle(pill()).opacity;
    await settle();
    return { flashed, settled: getComputedStyle(pill()).opacity };
  };
  window.__type = async (text) => {
    const input = field();
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    return input.value;
  };
  window.__draft = () => field().value;
</script>`;

async function withOverlay(run) {
  const cdp = launchWithPipe(chromePath, [
    "--headless=new",
    "--disable-gpu",
    "--no-sandbox",
    "--disable-dev-shm-usage",
    "--allow-file-access-from-files",
  ]);
  try {
    const page = join(cdp.dir, "overlay.html");
    const html = readFileSync(join(SRC, "index.html"), "utf8").replace(
      "<head>",
      `<head><base href="${pathToFileURL(SRC).href}">${stub}`,
    );
    writeFileSync(page, html);
    const { sessionId, navigate } = await createPageTarget(cdp);
    const loaded = new Promise((resolve) => cdp.on(sessionId, "Page.loadEventFired", resolve));
    await navigate(pathToFileURL(page).href);
    await loaded;
    const evalJs = async (expression) => {
      const msg = await cdp.send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true }, sessionId);
      assert.equal(msg.exceptionDetails, undefined, JSON.stringify(msg.exceptionDetails));
      return msg.result.value;
    };
    await evalJs("window.__ready()");
    await run(evalJs);
  } finally {
    await cleanupChrome(cdp);
  }
}

test("an open sprite menu keeps the quick-message pill down, and closing it does not open the pill", { skip: chromeSkip, timeout: 20000 }, async () => {
  await withOverlay(async (evalJs) => {
    const shown = (payload) => evalJs(`window.__paint(${JSON.stringify(payload)})`);
    assert.equal(Number(await shown(frame())), 0, "the pill waits out the dwell");
    assert.equal(Number(await shown(frame({ menu: true }))), 0, "the menu is open");
    const during = await evalJs("window.__dwell()");
    assert.equal(Number(during.flashed), 0, "the dwell does not open the pill while the menu is up");
    assert.equal(Number(during.settled), 0, "the pill stays down while the menu is up");
    assert.equal(Number(await shown(frame())), 0, "the menu has closed and the pointer is still here");
    const after = await evalJs("window.__dwell()");
    assert.equal(Number(after.flashed), 0, "closing the menu does not open the pill");
    assert.equal(Number(after.settled), 0, "a resting pointer keeps the pill down");
    assert.equal(Number(await shown(frame({ sprite: { hovered: false } }))), 0);
    assert.equal(Number(await shown(frame())), 0, "a fresh hover still waits out the dwell");
    const fresh = await evalJs("window.__dwell()");
    assert.equal(Number(fresh.flashed), 1, "the pill opens after the pointer leaves and hovers again");
    assert.equal(Number(fresh.settled), 1, "that later hover stays up");
  });
});

test("an open quick-message draft stays while the sprite menu opens and closes", { skip: chromeSkip, timeout: 20000 }, async () => {
  await withOverlay(async (evalJs) => {
    const shown = (payload) => evalJs(`window.__paint(${JSON.stringify(payload)})`);
    assert.equal(Number(await shown(frame())), 0, "the pill waits out the dwell");
    const opened = await evalJs("window.__dwell()");
    assert.equal(Number(opened.settled), 1, "the dwell opens the pill");
    assert.equal(await evalJs('window.__type("hey")'), "hey");
    assert.equal(Number(await shown(frame({ menu: true }))), 1, "the open menu leaves the draft up");
    assert.equal(await evalJs("window.__draft()"), "hey", "the draft survives the open menu");
    assert.equal(Number(await shown(frame())), 1, "closing the menu leaves the draft up");
    assert.equal(await evalJs("window.__draft()"), "hey", "the draft survives the menu closing");
  });
});

test("the pointer can leave the character for the menu without the pill opening when the menu closes", { skip: chromeSkip, timeout: 20000 }, async () => {
  await withOverlay(async (evalJs) => {
    const shown = (payload) => evalJs(`window.__paint(${JSON.stringify(payload)})`);
    assert.equal(Number(await shown(frame())), 0, "the pill waits out the dwell");
    assert.equal(Number(await shown(frame({ menu: true }))), 0, "the menu is open");
    assert.equal(Number(await shown(frame({ menu: true, sprite: { hovered: false } }))), 0, "the pointer is on the menu");
    assert.equal(Number(await shown(frame({ menu: true }))), 0, "the pointer is back on the character");
    const during = await evalJs("window.__dwell()");
    assert.equal(Number(during.flashed), 0, "the menu still holds the dwell down");
    assert.equal(Number(during.settled), 0);
    assert.equal(Number(await shown(frame())), 0, "the menu has closed on the character");
    const after = await evalJs("window.__dwell()");
    assert.equal(Number(after.flashed), 0, "closing the menu does not open the pill");
    assert.equal(Number(after.settled), 0);
    assert.equal(Number(await shown(frame({ sprite: { hovered: false } }))), 0);
    assert.equal(Number(await shown(frame())), 0, "a fresh hover still waits out the dwell");
    const fresh = await evalJs("window.__dwell()");
    assert.equal(Number(fresh.flashed), 1, "the pill opens after the pointer leaves and hovers again");
    assert.equal(Number(fresh.settled), 1);
  });
});

test("an empty pill is put away when the sprite menu opens, and closing the menu leaves it down", { skip: chromeSkip, timeout: 20000 }, async () => {
  await withOverlay(async (evalJs) => {
    const shown = (payload) => evalJs(`window.__paint(${JSON.stringify(payload)})`);
    assert.equal(Number(await shown(frame())), 0, "the pill waits out the dwell");
    const opened = await evalJs("window.__dwell()");
    assert.equal(Number(opened.settled), 1, "the dwell opens the pill");
    assert.equal(Number(await shown(frame({ menu: true }))), 0, "the open menu puts an empty pill away");
    assert.equal(Number(await shown(frame())), 0, "closing the menu leaves it down");
    const after = await evalJs("window.__dwell()");
    assert.equal(Number(after.flashed), 0, "a resting pointer does not bring it back");
    assert.equal(Number(after.settled), 0);
  });
});
