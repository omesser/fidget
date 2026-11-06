// A frame can land before the Character's art does. The sprite stays hidden
// until its art is there, then shows it, and only then tells the Shell it drew.
// Drives the real src/index.html and main.js.

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

// A second, different image, so each Character's decode can be held apart.
const OTHER_ART =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z/C/HgAGgwJ/lK3Q6wAAAABJRU5ErkJggg==";

const CHARACTERS = { characters: { "Buddy Bot": { smooth: false, art: { idle: [ART, ART] } } } };

const TWO_CHARACTERS = {
  characters: {
    "Buddy Bot": { smooth: false, art: { idle: [ART, ART] } },
    "Other Bot": { smooth: false, art: { idle: [OTHER_ART, OTHER_ART] } },
  },
};

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

const moved = { ...frame, sprites: [{ ...frame.sprites[0], x: 260, frame_index: 1 }] };

const twoSprites = {
  ...frame,
  sprites: [frame.sprites[0], { ...frame.sprites[0], id: "ivy", character: "Other Bot", x: 400 }],
};

// `character` answers through `loadArt`, or rejects when `artFails`. `invoked`
// lists every command, `drewIds` each `overlay_drew` id, and `drewWith` is the
// sprite as the first arrived. With `holdDecodes`, an image decodes only when
// `finishDecode(src)` says so.
function startup(drive, { artFails = false, holdDecodes = false, first = frame } = {}) {
  const stub = `
<script type="module">
  ${pageClock()}
  ${pageWait()}
  const handlers = {};
  const invoked = [];
  const drewIds = [];
  let loadArt;
  let drewWith;
  const decodes = new Map();
  const decoded = (src) => {
    if (!decodes.has(src)) {
      let finish;
      decodes.set(src, { done: new Promise((resolve) => (finish = resolve)), finish });
    }
    return decodes.get(src);
  };
  const finishDecode = (src) => decoded(src).finish();
  if (${holdDecodes}) {
    HTMLImageElement.prototype.decode = function () {
      return decoded(this.getAttribute("src")).done;
    };
  }
  window.__TAURI__ = {
    core: {
      invoke(name, args) {
        invoked.push(name);
        if (name === "overlay_drew") {
          drewIds.push(args?.id);
          drewWith ??= shown();
        }
        if (name === "character") {
          return ${artFails ? 'Promise.reject(new Error("no art"))' : "new Promise((resolve) => (loadArt = resolve))"};
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
  const shown = () => {
    const sprite = document.querySelector('.sprite[data-instance="bmo"]');
    return {
      visible: getComputedStyle(sprite).visibility === "visible",
      src: sprite.getAttribute("src"),
    };
  };
  const drew = () => invoked.filter((name) => name === "overlay_drew").length;
  async function drive() {
    console.log("__PROBE_READY__");
    await ready("frame");
    handlers.frame({ payload: ${JSON.stringify(first)} });
    await settle();
    ${drive}
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "index.html"), "utf8").replace(
    "<head>",
    `<head><base href="${pathToFileURL(SRC).href}">${stub}`,
  );
  return probePage(html);
}

const report = (fields) => `console.log("__PROBE_RESULT__" + JSON.stringify({ ${fields} }));`;

test("the sprite shows no art-less box before its art arrives", { skip: chromeSkip, timeout: 60000 }, async () => {
  const result = await startup(`
    const before = shown();
    loadArt(${JSON.stringify(CHARACTERS)});
    await until(() => shown().visible);
    ${report("before: before.visible, after: shown()")}`);
  assert.deepEqual(result, { before: false, after: { visible: true, src: ART } });
});

test("the overlay says it drew only after the art is on the sprite, and once", { skip: chromeSkip, timeout: 60000 }, async () => {
  const result = await startup(`
    const early = drew();
    loadArt(${JSON.stringify(CHARACTERS)});
    await until(() => drew() > 0);
    handlers.frame({ payload: ${JSON.stringify(moved)} });
    await settle();
    await settle();
    ${report("early, drewWith, drewIds")}`);
  assert.deepEqual(result, { early: 0, drewWith: { visible: true, src: ART }, drewIds: ["bmo"] });
});

test("each Instance is reported only once its own art has decoded", { skip: chromeSkip, timeout: 60000 }, async () => {
  const result = await startup(
    `
    loadArt(${JSON.stringify(TWO_CHARACTERS)});
    await until(() => document.querySelectorAll(".sprite[src]").length === 2);
    finishDecode(${JSON.stringify(ART)});
    await until(() => drewIds.length > 0);
    await settle();
    await settle();
    const firstOnly = [...drewIds];
    finishDecode(${JSON.stringify(OTHER_ART)});
    await until(() => drewIds.length > 1);
    ${report("firstOnly, drewIds")}`,
    { holdDecodes: true, first: twoSprites },
  );
  assert.deepEqual(result, { firstOnly: ["bmo"], drewIds: ["bmo", "ivy"] });
});

test("an overlay whose art never loads never says it drew", { skip: chromeSkip, timeout: 60000 }, async () => {
  const result = await startup(
    `
    await settle();
    await settle();
    ${report("drew: drew(), shown: shown()")}`,
    { artFails: true },
  );
  assert.deepEqual(result, { drew: 0, shown: { visible: false, src: null } });
});
