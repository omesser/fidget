// The pill and the bubble, measured on the real overlay. Dots use the same
// box as a line, and the pill has to step clear of both. Drives src/index.html
// the way overlay-rects.test.js does.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageClock } from "./page-clock.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

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
  ${pageClock()}
  ${pageWait()}
  const handlers = {};
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
        listeningNow(name);
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "overlay-test" }) },
  };
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
    hover_delay_ms: 1500,
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
  let stamp = 0;
  function emit(x, y, extra, shown = true) {
    handlers.frame({ payload: frame(x, y, { ...(extra || {}), stamp: ++stamp }, shown) });
  }
  async function blank() {
    emit(400, 300, null, false);
    await until(() => bubble() && !bubble().classList.contains("visible"));
  }
  async function speech(x, y) {
    emit(x, y, { dialogue: "hello there" });
    await until(
      () => bubble().classList.contains("visible") && bubble().dataset.mode === "speech",
    );
  }
  async function dots(x, y) {
    emit(x, y, { thinking: true });
    await until(
      () => bubble().classList.contains("visible") && bubble().dataset.mode === "thinking",
    );
  }
  async function idle(x, y) {
    emit(x, y, null);
    await until(() => sprite() && field() && !field().disabled);
  }
  // The gesture that opens the pill. The delay it waits out before showing is
  // one of the page timers the poll below steps over.
  function hover() {
    sprite().dispatchEvent(new PointerEvent("pointerleave"));
    sprite().dispatchEvent(new PointerEvent("pointerenter"));
  }
  async function openPill() {
    hover();
    await until(() => pill().classList.contains("visible"));
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
    );
    hover();
    await until(
      () => pill().classList.contains("visible") && bubble().dataset.mode === "thinking",
    );
    rows.push(snap(seat + " send then rehover over dots", "thinking"));
    return rows;
  }
  async function run() {
    console.log("__PROBE_READY__");
    await ready("frame");
    await until(() => window.innerWidth >= 100);
    const cases = [];
    for (const [seat, y] of [["mid", 300], ["top", 0]]) {
      cases.push(...await at(seat, 400, y));
    }
    const spellcheck = field().spellcheck;
    console.log("__PROBE_RESULT__" + JSON.stringify({ cases, spellcheck }));
  }
  window.addEventListener("load", () => run().catch((why) => {
    console.log("__PROBE_RESULT__" + JSON.stringify({ error: String(why), cases: [] }));
  }));
</script>`;

  const html = readFileSync(join(SRC, "index.html"), "utf8").replace(
    "<head>",
    `<head>${stub}`,
  );
  return probePage(html, { args: ["--window-size=1000,700"] });
}

let probe;
const report = () => (probe ??= drive());

test("the pill steps clear of a speech bubble and of the thinking dots", { skip: chromeSkip, timeout: 90000 }, async () => {
  const { error, cases } = await report();
  assert.equal(error, undefined, error);
  assert.equal(cases.length, 10);
  for (const row of cases) {
    assert.equal(row.bubble.visible, true, row.name);
    assert.equal(row.pill.visible, true, row.name);
    assert.equal(row.bubble.mode, row.expect, `${row.name} ${JSON.stringify(row)}`);
    assert.equal(row.overlap, false, `${row.name} ${JSON.stringify(row)}`);
  }
});

test("the pill's field opts out of spellcheck", { skip: chromeSkip, timeout: 90000 }, async () => {
  const { error, spellcheck } = await report();
  assert.equal(error, undefined, error);
  assert.equal(spellcheck, false);
});
