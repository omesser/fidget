// Overlay seam: a Shell frame in, pill opacity out (`.bubble.visible` is 1).
// One mode per test. A shell that never reports the mode is quick_message_aside.

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

// `over.sprite` replaces hovered/possessed. `visible` defaults to on screen.
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
        possessed: false,
        ...over.sprite,
      },
    ],
    visible: over.visible !== false,
    fade_ms: 0,
    sound: true,
    hover_delay_ms: HOVER_DELAY_MS,
    do_not_disturb: Boolean(over.do_not_disturb),
    aside: over.aside ?? null,
  };
}

const stub = `
<script type="module">
  ${pageClock()}
  ${pageWait()}
  const style = document.createElement("style");
  // The 180ms fade is not on this clock. The settled opacity is what a user sees.
  style.textContent = ".bubble{transition:none !important}";
  document.head.append(style);
  const handlers = {};
  window.__TAURI__ = {
    core: {
      invoke(name) {
        if (name === "character") return Promise.resolve({ characters: {} });
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
    // Before the next draw. A hidden Character's draw dismisses a pill the
    // timer just opened, which would hide the flash a resting pointer causes.
    const flashed = getComputedStyle(pill()).opacity;
    await settle();
    return { flashed, settled: getComputedStyle(pill()).opacity };
  };
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

async function picks(evalJs, mode) {
  const shown = (payload) => evalJs(`window.__paint(${JSON.stringify(payload)})`);
  assert.equal(Number(await shown(frame(mode.open))), 0, "the pill waits out the dwell");
  const opened = await evalJs("window.__dwell()");
  assert.equal(Number(opened.flashed), 1, "the dwell opens the pill");
  assert.equal(Number(opened.settled), 1, "the open pill stays up");
  assert.equal(Number(await shown(frame(mode.pick))), 0, `${mode.name} puts the pill away`);
  assert.equal(Number(await shown(frame(mode.hold))), 0, "the next hover report keeps the pill down");
  const again = await evalJs("window.__dwell()");
  assert.equal(Number(again.flashed), 0, "the dwell does not bring the pill back");
  assert.equal(Number(again.settled), 0, "the pill stays down after the next draw");
  if (!mode.reopen) return;
  // Chat's window is not on the frame. Leaving and hovering again must open the pill.
  assert.equal(Number(await shown(frame({ sprite: { hovered: false } }))), 0);
  assert.equal(Number(await shown(frame({ sprite: { hovered: true } }))), 0, "a fresh hover still waits out the dwell");
  const fresh = await evalJs("window.__dwell()");
  assert.equal(Number(fresh.flashed), 1, "the pill opens again after the pointer leaves");
  assert.equal(Number(fresh.settled), 1, "that later hover stays up");
}

const modes = [
  {
    name: "entering possession",
    open: {},
    pick: { sprite: { possessed: true } },
    hold: { sprite: { possessed: true } },
  },
  {
    name: "releasing possession",
    open: { sprite: { possessed: true } },
    pick: { sprite: { possessed: false } },
    hold: { sprite: { possessed: false } },
  },
  {
    name: "enabling Do Not Disturb",
    open: { do_not_disturb: false },
    pick: { do_not_disturb: true },
    hold: { do_not_disturb: true },
  },
  {
    name: "disabling Do Not Disturb",
    open: { do_not_disturb: true },
    pick: { do_not_disturb: false },
    hold: { do_not_disturb: false },
  },
  {
    name: "enabling go-away",
    open: { visible: true },
    pick: { visible: false },
    hold: { visible: false },
  },
  {
    name: "opening Settings",
    open: {},
    pick: { aside: "settings" },
    hold: {},
  },
  {
    name: "opening Chat",
    open: {},
    pick: { aside: "chat" },
    hold: {},
    reopen: true,
  },
  {
    name: "opening Memory",
    open: {},
    pick: { aside: "memory" },
    hold: {},
  },
  {
    name: "opening the Action Log",
    open: {},
    pick: { aside: "action-log" },
    hold: {},
  },
];

for (const mode of modes) {
  test(`${mode.name} closes the quick-message pill and a resting pointer does not reopen it`, { skip: chromeSkip, timeout: 20000 }, async () => {
    await withOverlay((evalJs) => picks(evalJs, mode));
  });
}
