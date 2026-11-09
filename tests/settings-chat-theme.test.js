// Settings reads the Chat surface's tokens (ADR-0029), so it paints the Chat UI
// and the Light/Dark Mode the user picked, not Minimal under the OS scheme.
// This drives the real Settings page headless with the snapshot it is sent.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
const FIXTURES = new URL("./fixtures/", import.meta.url);
const read = (name) => JSON.parse(readFileSync(new URL(name, FIXTURES), "utf8"));

// The page's ink, `--ui-ink` per design block in ui-tokens.css.
const INK = {
  "Minimal/Dark": "rgb(232, 235, 242)",
  "Minimal/Light": "rgb(26, 29, 36)",
  "Terminal/Dark": "rgb(198, 208, 220)",
  "Terminal/Light": "rgb(26, 35, 48)",
  "Glass/Dark": "rgb(241, 244, 249)",
  "Glass/Light": "rgb(26, 31, 43)",
};

// What a theme paints on the page itself: the body's pane and the root behind it,
// and whether the page outgrows the window, which scrolls the root.
const PAINT = `
  function paint() {
    const body = getComputedStyle(document.body);
    const root = document.documentElement;
    return {
      overflowX: root.scrollWidth > window.innerWidth,
      overflowY: root.scrollHeight > window.innerHeight,
      backgroundColor: body.backgroundColor,
      backgroundImage: body.backgroundImage,
      borderTopWidth: body.borderTopWidth,
      borderTopLeftRadius: body.borderTopLeftRadius,
      boxShadow: body.boxShadow,
      backdropFilter: body.backdropFilter,
      webkitBackdropFilter: body.webkitBackdropFilter,
      color: body.color,
      rootBackgroundImage: getComputedStyle(document.documentElement).backgroundImage,
    };
  }`;

const THEMES = ["Minimal", "Terminal", "Glass"].flatMap((design) => [
  [design, "Light"],
  [design, "Dark"],
]);

// The real Chat page, repainted through the events Chat listens on.
function chatPaints(themes, platform = "macos") {
  const opening = {
    name: "Buddy",
    character: "Buddy Bot",
    configured: true,
    enabled: true,
    harness: { name: "cursor-agent", session: "s", alive: true, missing: false, initializing: false, login: null },
    model: "",
    host: "",
    chat_ui: "minimal",
    platform,
    login: null,
    harness_name: "cursor-agent",
    instructions: "",
    personality: "",
    instance_prompt: "",
    prompt_limit: 2000,
  };
  const stub = `
<script>
  ${pageWait()}
  const heard = {};
  window.__TAURI__ = {
    core: { invoke(name) { return name === "chat_opening" ? Promise.resolve(${JSON.stringify(opening)}) : Promise.resolve(); } },
    event: { listen(name, handler) { heard[name] = handler; listeningNow(name); return Promise.resolve(() => {}); } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "chat-test" }) },
  };
  ${PAINT}
  async function run() {
    console.log("__PROBE_READY__");
    await ready("chat-ui");
    await ready("chat-appearance");
    const out = [];
    for (const [design, appearance] of ${JSON.stringify(themes)}) {
      heard["chat-ui"]({ payload: design.toLowerCase() });
      heard["chat-appearance"]({ payload: appearance.toLowerCase() });
      await settle();
      out.push(paint());
    }
    console.log("__PROBE_RESULT__" + JSON.stringify(out));
  }
  window.addEventListener("load", () => run().catch((why) => console.log("__PROBE_RESULT__" + JSON.stringify({ error: String(why) }))));
</script>`;
  const html = readFileSync(join(SRC, "chat.html"), "utf8").replace("<head>", `<head>${stub}`);
  return probePage(html, { args: ["--window-size=420,560", "--blink-settings=preferredColorScheme=1"], timeout: 6000 });
}

// The browser prefers light (Blink's 1), so a Dark pick that paints is the pick.
function settingsThemes(first, later, platform = "macos") {
  const form = read("settings-snapshot-harnessDriving.json");
  const values = read("settings-values-harnessDriving.json");
  const view = ([chat_ui, chat_appearance]) => ({ ...values, chat_ui, chat_appearance });
  const stub = `
<script>
  ${pageWait()}
  const heard = {};
  let snapshot = ${JSON.stringify({ form, view: view(first), reveal: null, platform })};
  window.__TAURI__ = {
    core: { invoke(name) { return name === "settings_snapshot" ? Promise.resolve(snapshot) : Promise.resolve(); } },
    event: { listen(name, handler) { heard[name] = handler; listeningNow(name); return Promise.resolve(() => {}); } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "settings-test" }) },
  };
  const report = (value) => console.log("__PROBE_RESULT__" + JSON.stringify(value));
  ${PAINT}
  const seen = () => {
    const root = document.documentElement;
    return {
      designs: [...root.classList].filter((name) => name.startsWith("chat-ui-")),
      palette: root.dataset.chatPalette,
      ink: getComputedStyle(document.body).color,
      paint: paint(),
    };
  };
  async function run() {
    console.log("__PROBE_READY__");
    await ready("settings-refresh");
    await until(() => document.querySelector(".set-form").childElementCount > 0);
    const out = { first: seen(), later: [] };
    for (const pick of ${JSON.stringify(later.map(view))}) {
      snapshot = { ...snapshot, view: pick };
      heard["settings-refresh"]();
      await settle();
      await settle();
      out.later.push(seen());
    }
    report(out);
  }
  window.addEventListener("load", () => run().catch((why) => report({ error: String(why) })));
</script>`;
  const html = readFileSync(join(SRC, "settings.html"), "utf8").replace("<head>", `<head>${stub}`);
  return probePage(html, { args: ["--window-size=760,640", "--blink-settings=preferredColorScheme=1"], timeout: 6000 });
}

const expect = ([design, appearance]) => ({
  designs: [`chat-ui-${design.toLowerCase()}`],
  palette: appearance.toLowerCase(),
  ink: INK[`${design}/${appearance}`],
});
const picked = ({ designs, palette, ink }) => ({ designs, palette, ink });

test("Settings opens in the picked Chat UI and Light/Dark Mode", { skip: chromeSkip, timeout: 60000 }, async () => {
  const seen = await settingsThemes(["Glass", "Dark"], []);
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(picked(seen.first), expect(["Glass", "Dark"]));
});

test("an open Settings page repaints when the Chat UI or the mode changes", { skip: chromeSkip, timeout: 60000 }, async () => {
  const picks = [
    ["Minimal", "Dark"],
    ["Terminal", "Dark"],
    ["Terminal", "Light"],
    ["Glass", "Light"],
    ["Minimal", "Light"],
  ];
  const seen = await settingsThemes(["Minimal", "Light"], picks);
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(seen.later.map(picked), picks.map(expect));
});

test("System follows the computer", { skip: chromeSkip, timeout: 60000 }, async () => {
  const seen = await settingsThemes(["Terminal", "System"], []);
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(picked(seen.first), expect(["Terminal", "Light"]));
});

test("Settings paints each Chat UI and mode as the Chat window does", { skip: chromeSkip, timeout: 60000 }, async () => {
  const chat = await chatPaints(THEMES);
  assert.equal(chat.error, undefined, chat.error);
  const settings = await settingsThemes(THEMES[0], THEMES.slice(1));
  assert.equal(settings.error, undefined, settings.error);
  const seen = [settings.first, ...settings.later].map((page) => page.paint);
  THEMES.forEach(([design, appearance], i) => {
    assert.deepEqual(seen[i], chat[i], `${design}/${appearance}`);
  });
  // Equal only means something if the themes paint differently.
  assert.equal(new Set(chat.map((one) => JSON.stringify(one))).size, THEMES.length);
});

test("Settings tints Glass on Linux as the Chat window does", { skip: chromeSkip, timeout: 60000 }, async () => {
  const glass = THEMES.filter(([design]) => design === "Glass");
  const chat = await chatPaints(glass, "linux");
  assert.equal(chat.error, undefined, chat.error);
  const settings = await settingsThemes(glass[0], glass.slice(1), "linux");
  assert.equal(settings.error, undefined, settings.error);
  const seen = [settings.first, ...settings.later].map((page) => page.paint);
  glass.forEach(([design, appearance], i) => {
    assert.deepEqual(seen[i], chat[i], `${design}/${appearance} on linux`);
  });
  // Equal only means something if Linux paints Glass differently.
  const mac = await chatPaints(glass);
  glass.forEach((_, i) => assert.notDeepEqual(chat[i], mac[i]));
});

test("no Chat UI makes Chat or Settings overflow its window", { skip: chromeSkip, timeout: 60000 }, async () => {
  const chat = await chatPaints(THEMES);
  assert.equal(chat.error, undefined, chat.error);
  const settings = await settingsThemes(THEMES[0], THEMES.slice(1));
  assert.equal(settings.error, undefined, settings.error);
  const pages = { chat, settings: [settings.first, ...settings.later].map((page) => page.paint) };
  const overflowing = Object.entries(pages).flatMap(([page, paints]) =>
    THEMES.filter((_, i) => paints[i].overflowX || paints[i].overflowY).map(([design, mode]) => `${page} ${design}/${mode}`),
  );
  assert.deepEqual(overflowing, []);
});
