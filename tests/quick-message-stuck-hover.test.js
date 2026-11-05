// A stuck :hover must not keep a Summon's hold: the Shell's hit test clears it.
// Drives the real src/index.html and main.js with a real pointer.

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
const ART =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

const frame = (hovered) => ({
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
      hovered,
    },
  ],
  visible: true,
  fade_ms: 0,
  sound: false,
  hover_delay_ms: HOVER_DELAY_MS,
});

const stub = `
<script type="module">
  ${pageClock()}
  ${pageWait()}
  const handlers = {};
  window.__TAURI__ = {
    core: {
      invoke(name) {
        if (name === "character") return Promise.resolve({ characters: { "Buddy Bot": { art: { idle: [${JSON.stringify(ART)}] } } } });
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
  const sprite = () => document.querySelector('.sprite[data-instance="bmo"]');
  const pill = () => document.querySelector('.quick-message[data-instance="bmo"]');
  // One Shell frame, then the hover delay on the page's clock.
  window.__frame = async (payload) => {
    handlers.frame({ payload });
    await settle();
    window.__advance(${HOVER_DELAY_MS});
    await settle();
    return { pill: pill().classList.contains("visible"), hover: sprite().matches(":hover") };
  };
  window.__start = async (payload) => {
    // Listened for once the art has loaded, so the sprite is drawn to hover.
    await ready("chat-opening");
    handlers.frame({ payload });
    await settle();
    const box = sprite().getBoundingClientRect();
    return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
  };
  window.__summon = async () => {
    sprite().dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await settle();
    return pill().classList.contains("visible");
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
    const point = (x, y) => cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y }, sessionId);
    await run({ evalJs, point });
  } finally {
    await cleanupChrome(cdp);
  }
}

test("a stuck :hover the Shell reports gone clears the Summon hold", { skip: chromeSkip, timeout: 5000 }, async () => {
  await withOverlay(async ({ evalJs, point }) => {
    const tick = (hovered) => evalJs(`window.__frame(${JSON.stringify(frame(hovered))})`);
    const at = await evalJs(`window.__start(${JSON.stringify(frame(true))})`);
    await point(at.x, at.y);
    assert.deepEqual(await tick(true), { pill: true, hover: true });

    assert.equal(await evalJs("window.__summon()"), false, "the Summon dismisses the pill");
    assert.deepEqual(await tick(true), { pill: false, hover: true }, "the hold keeps the pill down while the pointer rests");

    // The pointer never leaves the page, so :hover sticks; only the Shell says it left.
    assert.deepEqual(await tick(false), { pill: false, hover: true });
    assert.deepEqual(await tick(true), { pill: true, hover: true }, "a fresh hover opens the pill again");
  });
});
