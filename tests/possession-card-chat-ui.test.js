// The Possession card wears the Chat UI and Light/Dark the user picked, the
// way Chat and Settings do: the real overlay page, sent frames that carry the
// choice, in headless Chrome. Expected values are the token blocks', written out.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

// Panel, ink, accent, accent ink, line and control radius per Chat UI and
// Light/Dark. Glass is its unblurred tint: the overlay never blurs the desktop.
const CHAT_UIS = {
  "minimal/dark": ["rgb(20, 23, 30)", "rgb(232, 235, 242)", "rgb(92, 201, 181)", "rgb(11, 35, 31)", "rgba(255, 255, 255, 0.13)", "10px"],
  "minimal/light": ["rgb(248, 249, 251)", "rgb(26, 29, 36)", "rgb(22, 155, 133)", "rgb(255, 255, 255)", "rgba(0, 0, 0, 0.15)", "10px"],
  "terminal/dark": ["rgb(10, 13, 18)", "rgb(198, 208, 220)", "rgb(127, 224, 176)", "rgb(11, 35, 31)", "rgb(42, 51, 64)", "3px"],
  "terminal/light": ["rgb(250, 251, 252)", "rgb(26, 35, 48)", "rgb(22, 155, 133)", "rgb(255, 255, 255)", "rgb(213, 219, 223)", "3px"],
  "glass/dark": ["rgba(14, 17, 24, 0.84)", "rgb(241, 244, 249)", "rgba(255, 255, 255, 0.92)", "rgb(22, 32, 44)", "rgba(255, 255, 255, 0.22)", "999px"],
  "glass/light": ["rgba(242, 244, 248, 0.98)", "rgb(26, 31, 43)", "rgba(22, 155, 133, 0.95)", "rgb(255, 255, 255)", "rgba(0, 0, 0, 0.18)", "999px"],
};

const sprite = {
  id: "bmo",
  character: "BMO",
  animation: "idle",
  frame_index: 0,
  x: 600,
  y: 400,
  width: 64,
  height: 64,
  mirror: 1,
  dialogue: null,
  thinking: false,
  asking: false,
  bubble: true,
  cue: null,
  possessed: true,
  behaviors: [],
};

function run() {
  const stub = `
<script type="module">
  ${pageWait()}
  const handlers = {};
  window.__TAURI__ = {
    core: { invoke: () => Promise.resolve() },
    event: {
      listen(name, handler) {
        handlers[name] = handler;
        listeningNow(name);
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "overlay-test" }) },
  };
  const read = (el, props) => {
    const style = getComputedStyle(el);
    return Object.fromEntries(props.map((p) => [p, style[p]]));
  };
  // Both buttons whole and inside the card's padding, and Release on one line,
  // in the page's font and in Verdana, as wide as the Linux runner's DejaVu Sans.
  const wide = document.createElement("style");
  wide.textContent = ".possession, .possession * { font-family: Verdana, 'DejaVu Sans', sans-serif !important; }";
  const lines = (el) => {
    const range = document.createRange();
    range.selectNodeContents(el);
    return new Set([...range.getClientRects()].map((r) => Math.round(r.top))).size;
  };
  const fitsNow = (card) => {
    const right = card.getBoundingClientRect().right - parseFloat(getComputedStyle(card).paddingRight);
    const buttons = [...card.querySelectorAll(".possession-release, .possession-focus")];
    return lines(card.querySelector(".possession-release")) === 1 && buttons.every(
      (b) => b.scrollWidth <= b.clientWidth && b.getBoundingClientRect().right <= right + 0.5,
    );
  };
  const fits = async (card) => {
    const plain = fitsNow(card);
    document.head.append(wide);
    await settle();
    const wideFont = fitsNow(card);
    wide.remove();
    await settle();
    return { plain, wideFont };
  };
  async function drive() {
    console.log("__PROBE_READY__");
    await ready("frame");
    const seen = {};
    for (const chatUi of ${JSON.stringify(Object.keys(CHAT_UIS))}) {
      const [chat_ui, chat_appearance] = chatUi.split("/");
      const frame = (paused) => ({
        sprites: [{ ...${JSON.stringify(sprite)}, paused }],
        visible: true,
        fade_ms: 0,
        sound: false,
        chat_ui,
        chat_appearance,
      });
      handlers.frame({ payload: frame(false) });
      await until(() => document.querySelector(".possession.visible .possession-release"));
      for (let i = 0; i < 2; i += 1) await settle();
      const card = document.querySelector(".possession");
      const releaseEl = card.querySelector(".possession-release");
      const live = {
        fits: await fits(card),
        card: read(card, ["backgroundColor", "color"]),
        release: read(releaseEl, [
          "backgroundColor", "color", "borderTopColor", "borderTopLeftRadius",
        ]),
        pressed: read(card.querySelector('.possession-focus[aria-pressed="true"]'), [
          "backgroundColor", "color",
        ]),
      };
      handlers.frame({ payload: frame(true) });
      await until(() => document.querySelector(".possession.paused .possession-focus"));
      for (let i = 0; i < 2; i += 1) await settle();
      const raised = read(document.querySelector(".possession-focus"), [
        "backgroundColor", "color", "borderTopLeftRadius", "boxShadow",
      ]);
      seen[chatUi] = { ...live, raised };
    }
    console.log("__PROBE_RESULT__" + JSON.stringify(seen));
  }
  window.addEventListener("load", drive);
</script>`;
  const html = readFileSync(join(SRC, "index.html"), "utf8").replace("<head>", `<head>${stub}`);
  return probePage(html, { timeout: 20000 });
}

let report = null;
const probe = () => (report ??= run());

for (const [chatUi, [panel, ink, accent, accentInk, line, radius]] of Object.entries(CHAT_UIS)) {
  test(`the card is ${chatUi}'s panel and ink`, { skip: chromeSkip, timeout: 60000 }, async () => {
    const { card } = (await probe())[chatUi];
    assert.deepEqual(card, { backgroundColor: panel, color: ink });
  });

  test(`Release is ${chatUi}'s plain button, outlined in its line`, { skip: chromeSkip, timeout: 60000 }, async () => {
    const { release } = (await probe())[chatUi];
    assert.deepEqual(release, {
      backgroundColor: "rgba(0, 0, 0, 0)",
      color: ink,
      borderTopColor: line,
      borderTopLeftRadius: radius,
    });
  });

  test(`Release and Focus show their whole labels inside the ${chatUi} card, in a wide font too`, { skip: chromeSkip, timeout: 60000 }, async () => {
    const { fits } = (await probe())[chatUi];
    assert.deepEqual(fits, { plain: true, wideFont: true });
  });

  test(`a raised Focus is ${chatUi}'s flat accent button`, { skip: chromeSkip, timeout: 60000 }, async () => {
    const { raised } = (await probe())[chatUi];
    assert.deepEqual(raised, {
      backgroundColor: accent,
      color: accentInk,
      borderTopLeftRadius: radius,
      boxShadow: "none",
    });
  });

  test(`a pressed Focus reads in ${chatUi}'s ink, not the accent it sits on`, { skip: chromeSkip, timeout: 60000 }, async () => {
    const { pressed } = (await probe())[chatUi];
    assert.equal(pressed.color, ink);
    assert.notEqual(pressed.backgroundColor, accent);
  });
}
