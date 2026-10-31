// What the overlay tells the Shell it draws outside the art. Windows keeps every
// rect in the window's region so none is clipped, and the frame loop takes a
// click only over the clickable ones. Drives the real src/index.html and main.js.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

const sprite = (id, x, extra) => ({
  id,
  character: "Buddy Bot",
  animation: "idle",
  frame_index: 0,
  x,
  y: 400,
  width: 64,
  height: 64,
  mirror: 1,
  bubble: true,
  ...extra,
});

// Long enough to truncate, which is what puts "Open chat" in the bubble.
const longLine = "word ".repeat(200).trim();

const frame = {
  sprites: [
    sprite("talker", 100, { dialogue: longLine }),
    sprite("thinker", 600, { thinking: true }),
    sprite("idler", 1000),
  ],
  visible: true,
  fade_ms: 0,
  sound: false,
  // This page has no fake clock, so a 1 ms dwell keeps the hover from costing wall time.
  hover_delay_ms: 1,
};

// Every bubble moved to another display's overlay.
const handedOff = {
  ...frame,
  sprites: frame.sprites.map(({ dialogue, thinking, ...rest }) => ({ ...rest, bubble: false })),
};

// One launch for every test here: a headless Chrome start is most of the cost.
let probe = null;

function drive() {
  probe ??= launch();
  return probe;
}

async function launch() {
  const stub = `
<script type="module">
  const handlers = {};
  // Event-driven: each wait resolves on the invoke or listen it is waiting
  // for, never on a timer. The bubbles show over a few frames after the pill,
  // so the probe waits for the report that holds every rect, not for the pill.
  const waiters = [];
  const until = (ready) =>
    new Promise((resolve) => {
      const check = () => (ready() ? (resolve(), true) : false);
      if (!check()) waiters.push(check);
    });
  const wake = () => {
    for (let i = waiters.length - 1; i >= 0; i -= 1) if (waiters[i]()) waiters.splice(i, 1);
  };
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
  const box = (el, tail = 0) => {
    const r = el.getBoundingClientRect();
    return [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height) + tail];
  };
  const rects = () => window.__invoked.filter((c) => c.name === "overlay_rects").map((c) => c.args.rects);
  const last = () => rects().at(-1) ?? null;
  const has = (instance, clickable) => (last() ?? []).some((r) => r.follows?.instance === instance && r.clickable === clickable);
  async function drive() {
    console.log("__PROBE_READY__");
    await until(() => handlers.frame);
    handlers.frame({ payload: ${JSON.stringify(frame)} });
    const quick = document.querySelector('.quick-message[data-instance="idler"]');
    document.querySelector('.sprite[data-instance="idler"]').dispatchEvent(new PointerEvent("pointerenter"));
    // Open chat, the line, the dots and the pill: four rects, all reported.
    await until(() => has("talker", true) && has("talker", false) && has("thinker", false) && has("idler", true));
    const bubble = (id) => document.querySelector('.bubble:not(.quick-message)[data-instance="' + id + '"]');
    const reported = last();
    const expected = {
      openChat: box(bubble("talker").querySelector(".bubble-more")),
      speech: box(bubble("talker"), 10),
      dots: box(bubble("thinker"), 10),
      pill: box(quick),
    };
    handlers.frame({ payload: ${JSON.stringify(handedOff)} });
    await until(() => last()?.length === 0);
    console.log("__PROBE_RESULT__" + JSON.stringify({
      reported,
      expected,
      handedOff: last(),
      sent: rects().map((r) => JSON.stringify(r)),
    }));
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "index.html"), "utf8").replace(
    "<head>",
    `<head>${stub}`,
  );
  return probePage(html, { args: ["--window-size=1400,900"] });
}

test("Open chat and the pill take clicks; the bubble and its dots are only drawn", { skip: chromeSkip, timeout: 60000 }, async () => {
  const { reported, expected } = await drive();
  assert.ok(reported, "the overlay reported its rects");
  // Each names the sprite it rides with and where that sprite was, so the
  // Shell can keep it in the region while a drag moves the sprite.
  const rows = [
    ["Open chat", expected.openChat, true, { instance: "talker", at: [100, 400] }],
    ["the quick pill", expected.pill, true, { instance: "idler", at: [1000, 400] }],
    ["a line of dialogue", expected.speech, false, { instance: "talker", at: [100, 400] }],
    ["the thinking dots", expected.dots, false, { instance: "thinker", at: [600, 400] }],
  ];
  for (const [name, rect, clickable, follows] of rows) {
    const want = JSON.stringify({ rect, clickable, follows });
    assert.ok(
      reported.some((r) => JSON.stringify(r) === want),
      `${name}: want ${want} in ${JSON.stringify(reported)}`,
    );
  }
  assert.equal(reported.length, rows.length, "nothing else is reported");
});

test("an overlay that hands its bubbles on clears its rects, and only a change is sent", { skip: chromeSkip, timeout: 60000 }, async () => {
  const { handedOff, sent } = await drive();
  assert.deepEqual(handedOff, [], "an empty list clears");
  const repeats = sent.filter((rects, i) => i > 0 && rects === sent[i - 1]);
  assert.deepEqual(repeats, [], "no report repeats the one before it");
});
