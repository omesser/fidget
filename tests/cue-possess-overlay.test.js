// The possess cue on the overlay page itself, in headless Chrome: the shake
// turns the sprite about its drawn centre, and the ghost outlines the frame
// as drawn, mirror included.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

const frame = {
  sprites: [
    {
      id: "bmo",
      character: "Lefty",
      animation: "idle",
      frame_index: 0,
      x: 600,
      y: 400,
      width: 64,
      height: 64,
      mirror: -1,
      dialogue: null,
      thinking: false,
      asking: false,
      bubble: true,
      cue: "possess",
    },
  ],
  visible: true,
  fade_ms: 0,
  sound: false,
};

function run() {
  const stub = `
<script type="module">
  ${pageWait()}
  const handlers = {};
  // Art filled on its left half only, so a mirrored ghost is filled on its right.
  const art = document.createElement("canvas");
  art.width = 64;
  art.height = 64;
  art.getContext("2d").fillRect(0, 0, 32, 64);
  window.__TAURI__ = {
    core: {
      invoke(name) {
        if (name === "character") {
          return Promise.resolve({ characters: { Lefty: { art: { idle: [art.toDataURL()] } } } });
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
  async function drive() {
    console.log("__PROBE_READY__");
    await ready("frame");
    // Possession lands on a sprite already showing, so the art is drawn first.
    const shown = ${JSON.stringify(frame)};
    handlers.frame({ payload: { ...shown, sprites: [{ ...shown.sprites[0], cue: null }] } });
    const sprite = () => document.querySelector('.sprite[data-instance="bmo"]');
    await until(() => sprite()?.currentSrc && sprite().complete);
    handlers.frame({ payload: shown });
    await until(() => document.querySelector(".cue-oghost"));
    const drawn = sprite();
    const centre = (r) => [Math.round(r.left + r.width / 2), Math.round(r.top + r.height / 2)];
    const ghosts = [...document.querySelectorAll(".cue-oghost")];
    const lead = ghosts.find((g) => g.classList.contains("lead"));
    const alphaAt = (x, y) => lead.getContext("2d").getImageData(x, y, 1, 1).data[3];
    // Ghost px are (display + 6) * 2: the frame's left and right halves.
    const ghost = { left: alphaAt(28, 76), right: alphaAt(124, 76) };
    // The shake's first hard swing, 9% into it.
    for (const a of drawn.getAnimations()) {
      a.pause();
      a.currentTime = 260 + 0.09 * 320;
    }
    const swing = { rotate: getComputedStyle(drawn).rotate, centre: centre(drawn.getBoundingClientRect()) };
    const during = drawn.className;
    for (const a of drawn.getAnimations()) a.finish();
    // Settled frames, not until(): a class that stays must fail one test,
    // not hang every test that shares this probe.
    for (let i = 0; i < 5; i += 1) await settle();
    console.log("__PROBE_RESULT__" + JSON.stringify({
      after: drawn.className,
      classes: during,
      layers: ghosts.map((g) => g.className),
      ghost,
      swing,
    }));
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "index.html"), "utf8").replace("<head>", `<head>${stub}`);
  return probePage(html);
}

let report = null;
const probe = () => (report ??= run());

test("possess draws three outline ghosts over the sprite and shakes it", { skip: chromeSkip, timeout: 60000 }, async () => {
  const { classes, layers } = await probe();
  assert.match(classes, /\bcue-possess\b/);
  assert.deepEqual(layers, ["cue cue-oghost trail2", "cue cue-oghost trail", "cue cue-oghost lead"]);
});

test("the sprite drops the cue class once its animations end", { skip: chromeSkip, timeout: 60000 }, async () => {
  const { after } = await probe();
  assert.doesNotMatch(after, /\bcue-(possess|release)\b/);
});

test("the ghost outlines the frame as drawn, mirror included", { skip: chromeSkip, timeout: 60000 }, async () => {
  const { ghost } = await probe();
  assert.deepEqual(ghost, { left: 0, right: 46 }, "the art's filled half is on the right once mirrored");
});

// `rotate` applies after `transform`, so a pivot left at the element's own box
// would swing a sprite drawn at 600 px about the overlay's corner instead.
test("the shake turns the sprite about its drawn centre", { skip: chromeSkip, timeout: 60000 }, async () => {
  const { swing } = await probe();
  assert.equal(swing.rotate, "-5deg");
  const [x, y] = swing.centre;
  assert.ok(Math.abs(x - 632) <= 8 && Math.abs(y - 432) <= 8, `centre ${x},${y} near 632,432`);
});
