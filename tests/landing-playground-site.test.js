// The published landing page, assembled the way the Pages job assembles
// `_site/`, played with in headless Chrome: a throw lands with the hard puff,
// a slow drop perches on the window, the desk stops painting while scrolled out
// of view, reduced motion keeps the still tiles, a double-click opens the chat
// mock, and hovering opens the quick-message pill.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { after, before, test } from "node:test";
import { chromeSkip, chromePath, launchWithPipe, createPageTarget, captureConsoleLog, cleanupChrome } from "./chrome-cdp.js";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const PROBE_MS = 13000;

const helpers = `
  const errors = [];
  window.addEventListener("error", (e) => errors.push(e.message));
  const frame = () => new Promise(requestAnimationFrame);
  const cueIn = (layer, name) => new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("no " + name + " cue")), 3000);
    new MutationObserver((_, observer) => {
      const cue = layer.querySelector(name);
      if (!cue) return;
      observer.disconnect();
      clearTimeout(timer);
      resolve(cue);
    }).observe(layer, { childList: true, subtree: true });
  });
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const pointer = (target, type, x, y) => target.dispatchEvent(new PointerEvent(type, {
    bubbles: true, button: 0, pointerId: 1, pointerType: "mouse", isPrimary: true, clientX: x, clientY: y,
  }));
`;

const probe = `
<script>${helpers}
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    try {
      await frame(); await frame();
      const desk = document.getElementById("play-desk");
      const sprite = document.getElementById("play-sprite");
      const layer = document.getElementById("play-cues");
      if (desk.hidden) {
        console.log("__PROBE_RESULT__" + JSON.stringify({ errors, tiles: !document.getElementById("play-tiles").hidden }));
        return;
      }
      desk.scrollIntoView({ block: "center" });
      await frame(); await frame();
      const inner = () => { const box = desk.getBoundingClientRect(); return { x: box.left + desk.clientLeft, y: box.top + desk.clientTop }; };
      const feetY = () => sprite.getBoundingClientRect().bottom - inner().y;
      let x, y;
      const fire = (type) => pointer(sprite, type, x, y);
      const press = () => { const box = sprite.getBoundingClientRect(); x = box.left + box.width / 2; y = box.top + box.height / 2; fire("pointerdown"); };

      press();
      for (const [dx, dy] of [[-10, 0], [-40, 0], [-40, 0], [-40, 0]]) { await frame(); x += dx; y += dy; fire("pointermove"); }
      const hard = cueIn(layer, ".cue-puff.hard");
      fire("pointerup");
      await hard;
      const thrown = { feetY: feetY(), floor: desk.clientHeight };

      await new Promise((resolve) => setTimeout(resolve, 700));
      const win = document.querySelector('[data-win="light"]').getBoundingClientRect();
      const windowTop = win.top - inner().y;
      press();
      const box = sprite.getBoundingClientRect();
      const goal = { x: win.left + win.width / 2, y: win.top - 40 - box.height / 2 };
      const start = { x, y };
      for (let i = 1; i <= 10; i++) { await frame(); x = start.x + (goal.x - start.x) * i / 10; y = start.y + (goal.y - start.y) * i / 10; fire("pointermove"); }
      await new Promise((resolve) => setTimeout(resolve, 200));
      const soft = cueIn(layer, ".cue-puff:not(.hard)");
      fire("pointerup");
      await soft;
      const dropped = { feetY: feetY(), windowTop };

      const repaints = async (ms) => {
        let count = 0;
        const observer = new MutationObserver((records) => { count += records.length; });
        observer.observe(sprite, { attributes: true, subtree: true });
        await new Promise((resolve) => setTimeout(resolve, ms));
        observer.disconnect();
        return count;
      };
      window.scrollTo(0, 0);
      await frame(); await frame();
      const before = feetY();
      const offscreen = await repaints(1000);
      desk.scrollIntoView({ block: "center" });
      const back = await repaints(1000);
      const paused = { offscreen, back, moved: Math.abs(feetY() - before) };
      console.log("__PROBE_RESULT__" + JSON.stringify({ errors, tiles: false, thrown, dropped, paused }));
    } catch (err) {
      console.log("__PROBE_RESULT__" + JSON.stringify({ errors: [...errors, String(err)] }));
    }
  });
</script>`;

const gestures = `
<script>${helpers}
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    try {
      const desk = document.getElementById("play-desk");
      const sprite = document.getElementById("play-sprite");
      const layer = document.getElementById("play-cues");
      const chat = document.getElementById("play-chat");
      const pill = document.getElementById("play-qm");
      const field = pill.querySelector("input");
      const tuning = JSON.parse(document.getElementById("play-data").textContent).tuning;
      desk.scrollIntoView({ block: "center" });
      await frame(); await frame();
      const center = () => { const box = sprite.getBoundingClientRect(); return [box.left + box.width / 2, box.top + box.height / 2]; };

      const closedAtFirst = chat.hidden;
      const summon = cueIn(layer, ".cue-ring.in");
      for (let i = 0; i < 2; i++) { pointer(sprite, "pointerdown", ...center()); pointer(sprite, "pointerup", ...center()); }
      await summon;
      const rings = { summon: layer.querySelectorAll(".cue-ring.in").length, poke: layer.querySelectorAll(".cue-ring:not(.in)").length };
      const opened = !chat.hidden;
      const composer = chat.querySelector(".composer textarea");
      const sendButton = chat.querySelector(".composer button");
      const look = { panel: getComputedStyle(chat).backgroundColor, composer: !composer.disabled && composer.placeholder,
        send: !sendButton.disabled && getComputedStyle(sendButton).borderRadius };
      composer.focus();
      document.execCommand("insertText", false, "hi there");
      const typedInChat = composer.value;
      composer.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      const afterEnter = composer.value;
      document.execCommand("insertText", false, "again");
      sendButton.click();
      const talk = { typed: typedInChat, afterEnter, afterSend: composer.value, stillOpen: !chat.hidden,
        replies: chat.querySelectorAll(".log .row").length };
      const bar = chat.querySelector(".bar").getBoundingClientRect();
      const from = chat.getBoundingClientRect();
      const grip = [bar.left + 40, bar.top + bar.height / 2];
      pointer(chat.querySelector(".bar"), "pointerdown", ...grip);
      pointer(chat.querySelector(".bar"), "pointermove", grip[0] - 60, grip[1] + 30);
      pointer(chat.querySelector(".bar"), "pointerup", grip[0] - 60, grip[1] + 30);
      const to = chat.getBoundingClientRect();
      chat.querySelector(".close").click();
      const chatResult = { closedAtFirst, opened, rings, moved: [to.left - from.left, to.top - from.top], closed: chat.hidden, look, talk };

      await wait(tuning.pokeCooldownMs);
      sprite.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse", pointerId: 1 }));
      await wait(tuning.hoverDelayMs - 300);
      const early = pill.classList.contains("visible");
      await wait(600);
      const shown = pill.classList.contains("visible") && document.activeElement === field;
      document.execCommand("insertText", false, "hello there");
      const typed = field.value;
      const box = desk.getBoundingClientRect();
      pointer(desk, "pointerdown", box.right - 10, box.bottom - 10);
      const dismissed = !pill.classList.contains("visible");
      sprite.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse", pointerId: 1 }));
      await wait(tuning.hoverDelayMs + 300);
      const again = pill.classList.contains("visible");
      field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      const sent = !pill.classList.contains("visible") && field.value === "";
      sprite.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse", pointerId: 1 }));
      await wait(tuning.hoverDelayMs + 300);
      document.execCommand("insertText", false, "never mind");
      field.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      const escaped = !pill.classList.contains("visible") && field.value === "";
      const pillResult = { early, shown, typed, dismissed, again, sent, escaped };
      const lightBar = document.querySelector('[data-win="light"] .bar');
      const handle = lightBar.getBoundingClientRect();
      pointer(lightBar, "pointerdown", handle.left + 30, handle.top + 10);
      pointer(lightBar, "pointermove", handle.left + 30, desk.getBoundingClientRect().top - 200);
      pointer(lightBar, "pointerup", handle.left + 30, desk.getBoundingClientRect().top - 200);
      const raisedTop = lightBar.parentElement.getBoundingClientRect().top - desk.getBoundingClientRect().top - desk.clientTop;
      const moved = lightBar.getBoundingClientRect();
      pointer(lightBar, "pointerdown", moved.left + 30, moved.top + 10);
      pointer(lightBar, "pointermove", moved.left + 30 + 2000, moved.top + 10);
      pointer(lightBar, "pointerup", moved.left + 30 + 2000, moved.top + 10);
      const deskBox = desk.getBoundingClientRect();
      const pushedRight = Math.round(deskBox.left + desk.clientLeft + desk.clientWidth - lightBar.parentElement.getBoundingClientRect().right);
      const gestureBox = document.querySelector(".play-gestures").getBoundingClientRect(), panelBox = document.getElementById("play-panel").getBoundingClientRect();
      const beside = gestureBox.right < panelBox.left && gestureBox.top === panelBox.top;
      console.log("__PROBE_RESULT__" + JSON.stringify({ errors, chat: chatResult, pill: pillResult, raisedTop, pushedRight, beside,
        name: sprite.querySelector("img").alt }));
    } catch (err) {
      console.log("__PROBE_RESULT__" + JSON.stringify({ errors: [...errors, String(err)] }));
    }
  });
</script>`;

let dir;
let site;

before(() => {
  dir = mkdtempSync(join(tmpdir(), "landing-playground-"));
  site = join(dir, "_site");
  const built = spawnSync("bash", ["scripts/assemble-site.sh", site, "/dev/null"], { cwd: ROOT, encoding: "utf8" });
  assert.equal(built.status, 0, `assemble-site.sh failed: ${built.stderr}`);
});

after(() => rmSync(dir, { recursive: true, force: true, maxRetries: 3 }));

async function play(flags = [], script = probe) {
  const page = join(site, "probe.html");
  writeFileSync(page, readFileSync(join(site, "index.html"), "utf8").replace("<head>", `<head>${script}`));
  const cdp = launchWithPipe(chromePath, [
    "--headless=new", "--disable-gpu", "--no-sandbox", "--disable-dev-shm-usage", "--mute-audio",
    `--user-data-dir=${mkdtempSync(join(dir, "profile-"))}`, "--allow-file-access-from-files", "--window-size=1280,900",
    ...flags,
  ]);
  try {
    const { sessionId, navigate } = await createPageTarget(cdp);
    return await captureConsoleLog(cdp, sessionId, "__PROBE_RESULT__", PROBE_MS, () => navigate(pathToFileURL(page).href));
  } finally {
    await cleanupChrome(cdp);
  }
}

test("every frame the desk plays is in the assembled site", () => {
  const page = readFileSync(join(site, "index.html"), "utf8");
  const desk = JSON.parse(page.match(/<script type="application\/json" id="play-data">([\s\S]*?)<\/script>/)[1]);
  const frames = Object.values(desk.loops).flatMap((loop) => loop.frames);
  assert.ok(frames.length > 7, `the desk plays ${frames.length} frames`);
  for (const frame of frames) assert.ok(existsSync(join(site, frame)), `${frame} is published`);
});

test("the desk replaces the still tiles: a throw lands with the hard puff, a slow drop perches, offscreen pauses, reduced motion keeps the tiles", { skip: chromeSkip }, async () => {
  const result = await play();
  assert.deepEqual([result.errors, result.tiles], [[], false], "the desk mounted and the page threw nothing");
  assert.ok(Math.abs(result.thrown.feetY - result.thrown.floor) < 1, `landed on the floor: ${JSON.stringify(result.thrown)}`);
  assert.ok(Math.abs(result.dropped.feetY - result.dropped.windowTop) < 1, `perched on the window: ${JSON.stringify(result.dropped)}`);
  assert.equal(result.paused.offscreen, 0, `scrolled out of view, the desk painted nothing: ${JSON.stringify(result.paused)}`);
  assert.ok(result.paused.back > 0, `scrolled back, the desk animates again: ${JSON.stringify(result.paused)}`);
  assert.deepEqual(await play(["--force-prefers-reduced-motion"]), { errors: [], tiles: true });
});

test("a double-click opens the chat mock, which moves by its bar and closes; a hover opens the pill, which takes typing; a window drags to the desk top", { skip: chromeSkip }, async () => {
  const result = await play([], gestures);
  assert.deepEqual(result.errors, [], "the page threw nothing");
  const hex = readFileSync(join(ROOT, "src/chat-shared.css"), "utf8").match(/--shared-panel: #(\w{6});/)[1];
  const panel = `rgb(${hex.match(/../g).map((pair) => parseInt(pair, 16)).join(", ")})`;
  assert.deepEqual(result.chat, { closedAtFirst: true, opened: true, rings: { summon: 2, poke: 0 }, moved: [-60, 30], closed: true,
    look: { panel, composer: `Ask ${result.name}…`, send: "10px" },
    talk: { typed: "hi there", afterEnter: "", afterSend: "", stillOpen: true, replies: 0 } },
    "the Summon replaced the poke ring and opened a chat in the app's Chat panel, whose active composer takes typing and sends to nowhere, and it moves and closes");
  assert.deepEqual(result.pill, { early: false, shown: true, typed: "hello there", dismissed: true, again: true, sent: true, escaped: true },
    "the pill waits out the hover delay, focuses, types, and an outside click, Enter, or Escape dismisses it");
  assert.equal(result.raisedTop, 0, "a window drags all the way up to the desk's top edge");
  assert.ok(result.beside, "at 1280px the gestures sit to the left of the panel, top-aligned with it");
  assert.equal(result.pushedRight, 0, "beside the gestures, a window drags right up to the narrower desk's right edge");
});

test("the playground reads as Oded asked: its title and gestures, two windows that say drag me, Download below the desk, then the sections in order", () => {
  const page = readFileSync(join(site, "index.html"), "utf8");
  const play = page.match(/<section id="play">([\s\S]*?)<\/section>/)[1];
  assert.match(play, /<div class="play-panel" id="play-panel">\s*<p class="play-title">Try it here<\/p>\s*<div class="play-desk" id="play-desk"/,
    "the desk sits in a panel whose title bar is the invite, with no period, outside the desk the physics bounds");
  const order = ['id="play-desk"', '<h3 class="play-sub">Fidget gestures</h3>\n    <ul class="play-how">', 'class="play-get"']
    .map((mark) => play.indexOf(mark));
  assert.ok(order.every((at, i) => at > (order[i - 1] ?? play.indexOf('class="play-title"'))),
    `the titled panel, desk, Fidget gestures over the bullets, then the Download line: ${order}`);
  assert.ok(!play.includes("cues.html"), "the playground no longer links Fidget Cues");
  const gestures = [...play.matchAll(/<li>(.*?)<\/li>/g)].map((m) => m[1].replace(/<\/?b>/g, ""));
  assert.deepEqual(gestures, ["Left-click to poke", "Click and hold to drag it around and drop it",
    "Hover on Fidget to get the quick-message pill and type something, even while Chat is open", "Double-click to summon chat"]);
  const windows = [...play.matchAll(/data-win="(\w+)"/g)].map((m) => m[1]);
  assert.deepEqual(windows, ["dark", "light", "chat"]);
  assert.equal(play.match(/<span class="drag-me">drag me<\/span>/g).length, 3, "every window's bar says drag me");
  const chatBar = play.match(/data-win="chat"[^>]*>\s*<div class="bar">(.*?)<\/div>/)[1];
  assert.match(chatBar, /^<button class="close"[^>]*>&times;<\/button><i><\/i><i><\/i><span class="drag-me">drag me<\/span>$/,
    "the chat closes from its leftmost circle, with no × at the right");
  const cta = '<p class="play-get"><a href="#get">Download Fidget</a> to try it for real.</p>';
  assert.equal(play.split("Download Fidget").length - 1, 1, "one Download line in the playground, and no button");
  assert.ok(play.indexOf(cta) > play.indexOf('id="play-cues"'), "the linked Download line sits below the desk, not in the chat");
  const titles = [...page.matchAll(/<h2>(.*?)<\/h2>/g)].map((m) => m[1]);
  const at = titles.indexOf("Live Playground");
  assert.deepEqual(titles.slice(at, at + 4),
    ["Live Playground", "Packaged Fidgets", "Talks to the agent you already use", "How it works"]);
});
