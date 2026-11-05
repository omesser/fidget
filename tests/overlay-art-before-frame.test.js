// A frame can land before the Character's art does. The sprite stays hidden
// until its art is there, then shows it. Drives the real src/index.html and
// main.js.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageClock } from "./page-clock.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

const ART =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

const frame = {
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
    },
  ],
  visible: true,
  fade_ms: 0,
  sound: false,
  hover_delay_ms: 400,
};

function startup() {
  const stub = `
<script type="module">
  ${pageClock()}
  ${pageWait()}
  const handlers = {};
  let loadArt;
  window.__TAURI__ = {
    core: {
      invoke(name) {
        if (name === "character") return new Promise((resolve) => (loadArt = resolve));
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
  const shown = () => {
    const sprite = document.querySelector('.sprite[data-instance="bmo"]');
    return {
      visible: getComputedStyle(sprite).visibility === "visible",
      src: sprite.getAttribute("src"),
    };
  };
  async function drive() {
    console.log("__PROBE_READY__");
    await ready("frame");
    handlers.frame({ payload: ${JSON.stringify(frame)} });
    await settle();
    const before = shown();
    loadArt({ characters: { "Buddy Bot": { smooth: false, art: { idle: [${JSON.stringify(ART)}] } } } });
    await until(() => shown().visible);
    console.log("__PROBE_RESULT__" + JSON.stringify({ before: before.visible, after: shown() }));
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "index.html"), "utf8").replace(
    "<head>",
    `<head><base href="${pathToFileURL(SRC).href}">${stub}`,
  );
  return probePage(html);
}

test("the sprite shows no art-less box before its art arrives", { skip: chromeSkip, timeout: 60000 }, async () => {
  const report = await startup();
  assert.deepEqual(report, { before: false, after: { visible: true, src: ART } });
});
