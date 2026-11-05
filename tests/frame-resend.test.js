// A resend of an unchanged placement asks for no display frame, and the first
// placement a late listener hears is still drawn.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
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
      dialogue: null,
      thinking: false,
      asking: false,
      bubble: true,
      cue: null,
    },
  ],
  visible: true,
  fade_ms: 0,
  sound: false,
};

const nextFrame = {
  ...frame,
  sprites: [{ ...frame.sprites[0], frame_index: 1 }],
};

function run() {
  const stub = `
<script type="module">
  ${pageWait()}
  const handlers = {};
  // A placement asks the page for one display frame. The test drives those
  // frames, so the count between two deliveries is the listener's own.
  let asks = 0;
  let queued = [];
  window.requestAnimationFrame = (fn) => {
    asks += 1;
    queued.push(fn);
    return asks;
  };
  const paint = () => {
    const due = queued;
    queued = [];
    for (const fn of due) fn(performance.now());
  };
  window.__TAURI__ = {
    core: {
      invoke(name) {
        if (name === "character") return Promise.resolve({ characters: { "Buddy Bot": { art: { idle: [${JSON.stringify(ART)}, ${JSON.stringify(ART)}] } } } });
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
  // Runs every frame asked for until none is pending, so the count that
  // follows is the listener's own and not a frame something else still holds.
  const drain = async () => {
    await Promise.resolve();
    while (queued.length) {
      paint();
      await Promise.resolve();
    }
  };
  const deliver = async (payload) => {
    await drain();
    const before = asks;
    handlers.frame({ payload });
    const asked = asks - before;
    await drain();
    return asked;
  };
  async function drive() {
    console.log("__PROBE_READY__");
    await ready("frame");
    const first = await deliver(${JSON.stringify(frame)});
    const sprite = document.querySelector('.sprite[data-instance="bmo"]');
    const drawn = {
      visibility: sprite.style.visibility,
      transform: sprite.style.transform,
      animation: sprite.dataset.animation,
      frameIndex: sprite.dataset.frameIndex,
    };
    const resend = await deliver(${JSON.stringify(frame)});
    const changed = await deliver(${JSON.stringify(nextFrame)});
    console.log("__PROBE_RESULT__" + JSON.stringify({
      asks: { first, resend, changed },
      drawn,
      frameIndex: sprite.dataset.frameIndex,
    }));
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "index.html"), "utf8").replace(
    "<head>",
    `<head>${stub}`,
  );
  return probePage(html);
}

let report = null;
const probe = () => (report ??= run());

test("a late listener's first placement is drawn", { skip: chromeSkip, timeout: 60000 }, async () => {
  const { asks, drawn } = await probe();
  assert.equal(asks.first, 1);
  assert.deepEqual(drawn, {
    visibility: "visible",
    transform: "translate(200px, 200px)",
    animation: "idle",
    frameIndex: "0",
  });
});

test("an unchanged resend asks for no frame, and a change still does", { skip: chromeSkip, timeout: 60000 }, async () => {
  const { asks, frameIndex } = await probe();
  assert.equal(asks.resend, 0, "the resend requested a display frame");
  assert.equal(asks.changed, 1);
  assert.equal(frameIndex, "1");
});
