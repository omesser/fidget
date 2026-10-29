// The page character on the published landing page is played in headless Chrome.
// A poke, a throw, the handover, and a phone press have to move that sprite.
// Resting at the threshold does not bounce it, and it draws no frames while away.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { before, test } from "node:test";
import { chromeSkip, chromePath, launchWithPipe, createPageTarget, captureConsoleLog, cleanupChrome } from "./chrome-cdp.js";
import { tempDir } from "./temp-path.js";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
// The run takes about 12 s alone. Beside the other Chrome tests on a busy
// machine, its reload alone has been measured at 36 s.
const PROBE_MS = 90000;

const helpers = `
  const errors = [];
  window.addEventListener("error", (e) => errors.push(e.message));
  const frame = () => new Promise(requestAnimationFrame);
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const until = async (what, check, ms) => {
    const end = performance.now() + ms;
    while (!check()) {
      if (performance.now() > end) throw new Error("timed out waiting for " + what);
      await frame();
    }
  };
  const cueIn = (layer, name) => new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("no " + name + " cue")), 3000);
    new MutationObserver((_, observer) => {
      if (!layer.querySelector(name)) return;
      observer.disconnect();
      clearTimeout(timer);
      resolve();
    }).observe(layer, { childList: true, subtree: true });
  });
  const pointer = (target, type, x, y) => target.dispatchEvent(new PointerEvent(type, {
    bubbles: true, button: 0, pointerId: 1, pointerType: "mouse", isPrimary: true, clientX: x, clientY: y,
  }));
  const done = (result) => console.log("__PROBE_RESULT__" + JSON.stringify({ errors, ...result }));
`;

const probe = `
<script>${helpers}
  let walkerFrames = 0;
  const raf = requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (callback) => {
    if (/walker\.js/.test(new Error().stack)) walkerFrames++;
    return raf(callback);
  };
  // Where it first shows, before load: load waits on every image, by when it may have walked.
  const entry = new Promise((resolve) => {
    const look = () => {
      const at = document.getElementById("fidget-at");
      if (!at || at.hidden) return requestAnimationFrame(look);
      const column = document.querySelector(".wrap");
      const pad = parseFloat(getComputedStyle(column).paddingLeft);
      const content = { left: column.getBoundingClientRect().left + pad, right: column.getBoundingClientRect().right - pad };
      const box = at.querySelector("#fidget-sprite").getBoundingClientRect();
      resolve({ inMargin: box.right <= content.left || box.left >= content.right, box: [box.left, box.right], content });
    };
    requestAnimationFrame(look);
  });
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    const at = document.getElementById("fidget-at");
    const sprite = document.getElementById("fidget-sprite");
    const back = document.getElementById("fidget-back");
    if (sessionStorage.getItem("probe-first")) {
      await frame(); await frame();
      const reloaded = { hidden: at.hidden, back: !back.hidden, layerEmpty: !document.querySelector("#fidget-say.visible") };
      const spot = back.getBoundingClientRect();
      const backInView = spot.width > 0 && spot.top >= 0 && spot.bottom <= innerHeight;
      document.getElementById("characters").scrollIntoView({ block: "start" });
      back.click();
      let returned = true;
      try {
        await until("the walk back on after Bring back", () => !at.hidden, 2000);
      } catch {
        returned = false;
      }
      done({ ...JSON.parse(sessionStorage.getItem("probe-first")), reloaded, backInView, returned, backAfter: !back.hidden });
      return;
    }
    try {
      const layer = document.getElementById("fidget-cues");
      const say = document.getElementById("fidget-say");
      const desk = document.getElementById("play-desk");
      const playSprite = document.getElementById("play-sprite");
      const img = sprite.querySelector("img");
      const data = JSON.parse(document.getElementById("play-data").textContent);
      const member = Object.values(data.cast).find((one) => one.name === img.alt);
      const feetY = () => sprite.getBoundingClientRect().bottom;
      // On the floor and still for a frame: the walk up moves it every frame,
      // and its last step can end a hair above the floor.
      let lastSpot = null;
      const onPage = () => {
        const spot = at.style.transform;
        const still = spot === lastSpot;
        lastSpot = spot;
        return !at.hidden && still && Math.abs(feetY() - innerHeight) < 1;
      };
      const center = () => { const box = sprite.getBoundingClientRect(); return [box.left + box.width / 2, box.top + box.height / 2]; };

      // The hero's stage shows its own fidget, so the page's waits below it.
      // The cast section is below the stage and clear of the playground too.
      const belowStage = () => document.getElementById("characters").scrollIntoView({ block: "start" });
      // Waits for something that must not happen: it walking on at the top.
      await wait(500);
      const atTop = at.hidden;
      belowStage();
      await until("the walk onto the page", onPage, 2000);
      const entered = await entry;
      const ring = cueIn(layer, ".cue-ring");
      pointer(sprite, "pointerdown", ...center());
      pointer(sprite, "pointerup", ...center());
      await ring;
      await frame();
      const reacted = member.loops.react.frames.some((src) => img.getAttribute("src") === src);
      await until("the bubble", () => say.classList.contains("visible"), 1000);
      const poke = { reacted, said: member.lines.includes(say.textContent), title: say.title };

      await until("the end of react", () => !member.loops.react.frames.includes(img.getAttribute("src")), 1000);
      let [x, y] = center();
      pointer(sprite, "pointerdown", x, y);
      for (const [dx, dy] of [[10, -10], [40, -60], [40, -60], [40, -60]]) { await frame(); x += dx; y += dy; pointer(sprite, "pointermove", x, y); }
      const hard = cueIn(layer, ".cue-puff.hard");
      pointer(sprite, "pointerup", x, y);
      await hard;
      const thrown = { feetY: feetY(), floor: innerHeight };

      let both = 0;
      const deskShows = () => { const box = desk.getBoundingClientRect(); return box.bottom > 0 && box.top < innerHeight; };
      const watch = () => { if (!at.hidden && !playSprite.hidden && deskShows()) both++; };
      desk.scrollIntoView({ block: "center" });
      await until("the playground's Character", () => { watch(); return at.hidden && !playSprite.hidden; }, 2000);
      const deskFloor = () => desk.getBoundingClientRect().top + desk.clientTop + desk.clientHeight;
      await until("the walk up onto the desk floor", () => Math.abs(playSprite.getBoundingClientRect().bottom - deskFloor()) < 1, 2000);
      const arrived = { playFeet: playSprite.getBoundingClientRect().bottom, deskFloor: deskFloor() };
      belowStage();
      await until("the walk back onto the page", () => { watch(); return onPage(); }, 2000);
      const handoff = { both, arrived };

      window.scrollTo(0, 0);
      // The Hide chip a press showed goes on a timer, not a frame.
      await until("the walk off at the top", () => at.hidden && !at.classList.contains("touched"), 2000);
      await frame();
      const writes = [];
      const quiet = new MutationObserver((records) => writes.push(...records.map((r) => r.attributeName)));
      quiet.observe(document.getElementById("fidget"), { attributes: true, subtree: true });
      const framesBefore = walkerFrames;
      // Waits for something that must not happen: a frame or a write while it is away.
      await wait(1000);
      quiet.disconnect();
      const idle = { writes, frames: walkerFrames - framesBefore };
      const framesAway = walkerFrames;
      const reacting = () => !at.hidden && member.loops.react.frames.includes(img.getAttribute("src"));
      let reacts = 0;
      let crossings = 0;
      let [wasReacting, wasAway] = [false, at.hidden];
      const stage = document.querySelector(".stage");
      // A reader's slow scroll past the hero, a step a frame, from 200px short of its end.
      window.scrollTo(0, scrollY + stage.getBoundingClientRect().bottom - 200);
      for (let i = 0, past = 0; i < 200 && past < 90; i++) {
        window.scrollBy(0, 4);
        if (stage.getBoundingClientRect().bottom < 0) past++;
        await frame();
        if (reacting() && !wasReacting) reacts++;
        if (at.hidden !== wasAway) crossings++;
        [wasReacting, wasAway] = [reacting(), at.hidden];
      }
      const read = { reacts, crossings };
      idle.resumed = walkerFrames > framesAway;

      // Most of the way to the threshold and back over it, a few px at a time.
      const edge = window.scrollY + desk.getBoundingClientRect().top - innerHeight + desk.offsetHeight * 0.4;
      let flips = 0;
      let wasHidden = at.hidden;
      for (let i = 0; i < 40; i++) {
        window.scrollTo(0, edge + (i % 2 ? 30 : -30));
        // Waits for something that must not happen: a flip at the threshold.
        await wait(50);
        if (at.hidden !== wasHidden) { flips++; wasHidden = at.hidden; }
      }
      const bounce = { flips, stayed: !at.hidden };

      document.getElementById("fidget-hide").click();
      await frame(); await frame();
      const hid = { hidden: at.hidden, back: !back.hidden, stored: localStorage.getItem("fidget.hidden") };
      sessionStorage.setItem("probe-first", JSON.stringify({ alt: img.alt, atTop, entered, poke, thrown, handoff, read, idle, bounce, hid }));
      location.reload();
    } catch (err) {
      done({ errors: [...errors, String(err)] });
    }
  });
</script>`;

const still = `
<script>${helpers}
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    try {
      const at = document.getElementById("fidget-at");
      const sprite = document.getElementById("fidget-sprite");
      const img = sprite.querySelector("img");
      const say = document.getElementById("fidget-say");
      const stage = document.querySelector(".stage");
      window.scrollTo(0, stage.getBoundingClientRect().bottom + 1);
      await until("the Character on the page", () => !at.hidden, 1000);
      const first = { src: img.getAttribute("src"), left: sprite.getBoundingClientRect().left };
      let changes = 0;
      const observer = new MutationObserver((records) => { changes += records.length; });
      observer.observe(at, { attributes: true, subtree: true });
      // Waits for something that must not happen: a step or a new frame.
      await wait(1000);
      observer.disconnect();
      const box = sprite.getBoundingClientRect();
      pointer(sprite, "pointerdown", box.left + box.width / 2, box.top + box.height / 2);
      pointer(sprite, "pointerup", box.left + box.width / 2, box.top + box.height / 2);
      await until("the bubble", () => say.classList.contains("visible"), 1000);
      done({ changes, moved: sprite.getBoundingClientRect().left - first.left, tiles: !document.getElementById("play-tiles").hidden,
        idle: Object.values(JSON.parse(document.getElementById("play-data").textContent).cast)
          .some((one) => one.name === img.alt && one.loops.idle.frames[0] === first.src) });
    } catch (err) {
      done({ errors: [...errors, String(err)] });
    }
  });
</script>`;

// A reload restored mid-page: the desk is in sight, but under the 0.6 that
// counts it in view, before either observer first reports.
const midway = `
<script>${helpers}
  const firsts = [];
  window.IntersectionObserver = class extends IntersectionObserver {
    constructor(callback, options) {
      super((entries, observer) => {
        for (const entry of entries) if (entry.target.id === "play-desk") firsts.push(entry.intersectionRatio);
        callback(entries, observer);
      }, options);
    }
    observe(target) {
      if (target.id === "play-desk" && !firsts.scrolled) {
        firsts.scrolled = true;
        window.scrollTo(0, scrollY + target.getBoundingClientRect().top - innerHeight + target.offsetHeight * 0.3);
      }
      super.observe(target);
    }
  };
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    try {
      const at = document.getElementById("fidget-at");
      await until("the walk onto the page", () => !at.hidden, 2000);
      done({ first: firsts[0], stageGone: document.querySelector(".stage").getBoundingClientRect().bottom < 0 });
    } catch (err) {
      done({ errors: [...errors, String(err)], first: firsts[0] });
    }
  });
</script>`;

// The twist section shows a Character of its own, so the page's sinks away
// once the section fills most of the screen and walks back once it is gone.
const twist = `
<script>${helpers}
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    const at = document.getElementById("fidget-at");
    const sprite = document.getElementById("fidget-sprite");
    const section = document.getElementById("twist");
    const seen = async (what, check) => { try { await until(what, check, 2000); return true; } catch { return false; } };
    try {
      document.getElementById("characters").scrollIntoView({ block: "start" });
      let lastSpot = null;
      const onPage = () => {
        const spot = at.style.transform;
        const still = spot === lastSpot;
        lastSpot = spot;
        return !at.hidden && still && Math.abs(sprite.getBoundingClientRect().bottom - innerHeight) < 1;
      };
      await until("the walk onto the page", onPage, 2000);
      // The probe's observer reports in the same update as the page's, and a
      // sink shows within two frames of that: the page draws each frame it ticks.
      const sighted = new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("no report of the twist section in sight")), 2000);
        new IntersectionObserver((entries, observer) => {
          if (!entries.some((entry) => entry.isIntersecting)) return;
          observer.disconnect();
          clearTimeout(timer);
          resolve();
        }).observe(section);
      });
      // Its top 30% up the screen: in sight, but not most of it.
      window.scrollTo(0, scrollY + section.getBoundingClientRect().top - innerHeight * 0.7);
      await sighted;
      await frame(); await frame();
      const sank = at.hidden || sprite.getBoundingClientRect().bottom > innerHeight + 0.5;
      section.scrollIntoView({ block: "end" });
      // Too tall for most of itself to fit, so only most of the screen can count.
      const taller = section.offsetHeight > innerHeight / 0.6;
      const left = await seen("the sink for the twist section", () => at.hidden);
      // Just past its end, none of it in sight.
      window.scrollTo(0, scrollY + section.getBoundingClientRect().bottom + 2);
      const back = await seen("the walk back on past it", onPage);
      done({ sliverKept: !sank, taller, left, back });
    } catch (err) {
      done({ errors: [...errors, String(err)] });
    }
  });
</script>`;

// Hide and Bring back pressed with real keys, which the page asks for by name.
const keys = `
<script>${helpers}
  const focused = () => document.activeElement.id || document.activeElement.tagName;
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    try {
      const at = document.getElementById("fidget-at");
      const back = document.getElementById("fidget-back");
      document.getElementById("characters").scrollIntoView({ block: "start" });
      await until("the walk onto the page", () => !at.hidden, 2000);
      document.getElementById("fidget-hide").focus();
      pressKey("Enter");
      await until("Hide", () => !back.hidden && at.hidden, 1000);
      const hid = focused();
      pressKey("Enter");
      await until("the walk back on", () => !at.hidden, 1000);
      await frame();
      const returned = focused();
      pressKey("Enter");
      await until("Hide again", () => !back.hidden && at.hidden, 1000);
      window.scrollTo(0, 0);
      await frame(); await frame();
      pressKey("Enter");
      await until("Bring back at the top", () => back.hidden, 1000);
      await frame();
      const heldOff = { focused: focused(), away: at.hidden };
      pressKey("Tab");
      await until("a Tab", () => document.activeElement !== document.body, 1000);
      done({ hid, returned, heldOff, tabbedToTop: document.activeElement === document.querySelector("a[href]") });
    } catch (err) {
      done({ errors: [...errors, String(err)] });
    }
  });
</script>`;

// A phone: a finger pressed on the sprite and moved scrolls the page there, so
// the sprite must not take the press as a grab.
const phone = `
<script>${helpers}
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    try {
      const at = document.getElementById("fidget-at");
      const sprite = document.getElementById("fidget-sprite");
      document.getElementById("characters").scrollIntoView({ block: "start" });
      let lastSpot = null;
      await until("the end of the walk up", () => {
        const spot = at.style.transform;
        const still = spot === lastSpot;
        lastSpot = spot;
        return !at.hidden && still && Math.abs(sprite.getBoundingClientRect().bottom - innerHeight) < 1;
      }, 2000);
      const touch = (type, x, y) => sprite.dispatchEvent(new PointerEvent(type, {
        bubbles: true, button: 0, pointerId: 1, pointerType: "touch", isPrimary: true, clientX: x, clientY: y,
      }));
      const box = sprite.getBoundingClientRect();
      const x = box.left + box.width / 2;
      let y = box.top + box.height / 2;
      const ring = cueIn(document.getElementById("fidget-cues"), ".cue-ring");
      touch("pointerdown", x, y);
      for (let i = 0; i < 4; i++) { await frame(); y -= 20; touch("pointermove", x, y); }
      // Waits for something that must not happen: past dragDelayMs, a press held this long grabs.
      await wait(250);
      const lifted = innerHeight - sprite.getBoundingClientRect().bottom;
      touch("pointerup", x, y);
      await ring;
      done({ narrow: matchMedia("(max-width: 480px)").matches, lifted: Math.round(lifted),
        touchAction: getComputedStyle(sprite).touchAction });
    } catch (err) {
      done({ errors: [...errors, String(err)] });
    }
  });
</script>`;

// A pick by keyboard while the page character walks on swaps it on the next
// frames. A pick while the desk has it shows when it next walks on.
const picks = `
<script>${helpers}
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    try {
      const data = JSON.parse(document.getElementById("play-data").textContent);
      const at = document.getElementById("fidget-at");
      const sprite = document.getElementById("fidget-sprite");
      const img = sprite.querySelector("img");
      const playImg = document.querySelector("#play-sprite img");
      const desk = document.getElementById("play-desk");
      const button = (id) => document.querySelector('#play-picker button[data-id="' + id + '"]');
      const wearing = (id) => ({ alt: img.alt, width: sprite.style.width, frame: Object.values(data.cast[id].loops)
        .some((loop) => loop.frames.includes(img.getAttribute("src"))) });
      document.getElementById("characters").scrollIntoView({ block: "start" });
      await until("the walk onto the page", () => !at.hidden, 2000);
      button("cat").focus({ preventScroll: true });
      pressKey("Enter");
      await until("the desk's Cat", () => playImg.alt === "Cat", 1000);
      await frame(); await frame();
      const onPage = { ...wearing("cat"), shown: !at.hidden };
      desk.scrollIntoView({ block: "center" });
      await until("the hand over to the desk", () => at.hidden, 2000);
      button("bmo").click();
      const desked = playImg.alt;
      document.getElementById("characters").scrollIntoView({ block: "start" });
      await until("the walk back on", () => !at.hidden, 2000);
      done({ onPage, desked, entry: wearing("bmo"), sizes: { cat: data.cast.cat.size[0] + "px", bmo: data.cast.bmo.size[0] + "px" } });
    } catch (err) {
      done({ errors: [...errors, String(err)] });
    }
  });
</script>`;

let site;

before((t) => {
  site = join(tempDir(t, "landing-walker-"), "_site");
  const built = spawnSync("bash", ["scripts/assemble-site.sh", site, "/dev/null"], { cwd: ROOT, encoding: "utf8" });
  assert.equal(built.status, 0, `assemble-site.sh failed: ${built.stderr}`);
});

async function play(script, flags = [], viewport = null) {
  const page = join(site, "probe.html");
  writeFileSync(page, readFileSync(join(site, "index.html"), "utf8").replace("<head>", `<head>${script}`));
  const cdp = launchWithPipe(chromePath, [
    "--headless=new", "--disable-gpu", "--no-sandbox", "--disable-dev-shm-usage", "--mute-audio",
    "--allow-file-access-from-files", "--window-size=1280,900",
    ...flags,
  ]);
  try {
    const { sessionId, navigate, blockURLs } = await createPageTarget(cdp);
    // The page plays from file://. The README's video, fetched from GitHub, can
    // take seconds under load and hold up a reload, so nothing goes to the network.
    await blockURLs(["http://*", "https://*"]);
    // A window narrower than 500px is clamped, so a phone's is emulated.
    if (viewport) await cdp.send("Emulation.setDeviceMetricsOverride", { ...viewport, deviceScaleFactor: 1, mobile: false }, sessionId);
    // A probe's pressKey(name) sends a real key, which a synthetic event is not.
    await cdp.send("Runtime.addBinding", { name: "pressKey" }, sessionId);
    cdp.on(sessionId, "Runtime.bindingCalled", ({ payload }) => {
      const [code, text] = { Enter: [13, "\r"], Tab: [9, ""] }[payload];
      const key = { key: payload, code: payload, windowsVirtualKeyCode: code };
      cdp.send("Input.dispatchKeyEvent", { type: "keyDown", ...key, text }, sessionId)
        .then(() => cdp.send("Input.dispatchKeyEvent", { type: "keyUp", ...key }, sessionId))
        // A probe can finish on the keyDown, and Chrome close before its keyUp.
        .catch(() => {});
    });
    return await captureConsoleLog(cdp, sessionId, "__PROBE_RESULT__", PROBE_MS, () => navigate(pathToFileURL(page).href));
  } finally {
    await cleanupChrome(cdp);
  }
}

test("the page character walks on, reacts, lands a throw, hands over to the playground and back, and Hide survives a reload", { skip: chromeSkip, timeout: PROBE_MS + 10000 }, async () => {
  const result = await play(probe);
  assert.deepEqual(result.errors, [], "the page threw nothing");
  assert.equal(result.alt, "Buddy Bot", "with no pick, Buddy Bot plays");
  assert.equal(result.atTop, true, "it stays off the page while the hero's stage is on screen");
  assert.ok(result.entered.inMargin, `it walked up in a margin, clear of the content column: ${JSON.stringify(result.entered)}`);
  assert.deepEqual(result.poke, { reacted: true, said: true, title: "A line from its personality file." },
    "a poke plays react and says a line from the Character's personality.txt");
  assert.ok(Math.abs(result.thrown.feetY - result.thrown.floor) < 1, `landed on the bottom edge: ${JSON.stringify(result.thrown)}`);
  assert.equal(result.handoff.both, 0, "the page never showed two at once");
  assert.ok(Math.abs(result.handoff.arrived.playFeet - result.handoff.arrived.deskFloor) < 1,
    `the playground's Character walked up onto the desk floor: ${JSON.stringify(result.handoff.arrived)}`);
  assert.ok(result.read.reacts === 0 && result.read.crossings === 1,
    `a slow scroll never startles the awake character, and it walks on once, past the hero: ${JSON.stringify(result.read)}`);
  assert.deepEqual(result.idle, { writes: [], frames: 0, resumed: true },
    "while it is away the page draws no frames and writes nothing, and frames resume when it walks back on");
  assert.deepEqual(result.bounce, { flips: 0, stayed: true }, "resting at the threshold does not bounce it");
  assert.deepEqual(result.hid, { hidden: true, back: true, stored: "1" });
  assert.deepEqual(result.reloaded, { hidden: true, back: true, layerEmpty: true }, "Hide is remembered across a reload");
  assert.equal(result.backInView, true, "Bring back shows in a corner of the viewport");
  assert.deepEqual([result.returned, result.backAfter], [true, false], "Bring back walks it on again and hides the link");
});

test("a pick on the desk swaps the page character at once while it is on the page, else on its next walk on", { skip: chromeSkip }, async () => {
  const result = await play(picks);
  assert.deepEqual(result.errors, [], "the page threw nothing");
  assert.deepEqual(result.onPage, { alt: "Cat", width: result.sizes.cat, frame: true, shown: true },
    "Enter on Cat swapped the page character it was walking as, without a reload");
  assert.equal(result.desked, "BMO", "a pick while the desk has the Character swaps the desk");
  assert.deepEqual(result.entry, { alt: "BMO", width: result.sizes.bmo, frame: true }, "the next walk on is BMO from its first frame");
});

test("under reduced motion the page character stays put on its first frame, and a poke still shows a bubble", { skip: chromeSkip }, async () => {
  const result = await play(still, ["--force-prefers-reduced-motion"]);
  assert.deepEqual(result, { errors: [], changes: 0, moved: 0, tiles: true, idle: true });
});

test("a page loaded with the desk partly in sight counts it out of view, so the page character walks on", { skip: chromeSkip }, async () => {
  const result = await play(midway);
  assert.ok(result.first > 0 && result.first < 0.6, `the first report saw the desk partly: ${result.first}`);
  assert.deepEqual(result, { errors: [], first: result.first, stageGone: true });
});

test("Hide moves focus to Bring back, and Bring back to the sprite's Hide, or to the page when it cannot walk on", { skip: chromeSkip }, async () => {
  const result = await play(keys);
  assert.deepEqual(result, { errors: [], hid: "fidget-back", returned: "fidget-hide", heldOff: { focused: "BODY", away: true },
    tabbedToTop: true }, "held off the page, focus goes to the page, whose next Tab starts at the top");
});

test("on a phone a press moved across the page character pokes it rather than grabbing it", { skip: chromeSkip }, async () => {
  const result = await play(phone, [], { width: 390, height: 844 });
  assert.deepEqual(result, { errors: [], narrow: true, lifted: 0, touchAction: "auto" });
});

// On a short landscape screen the section is taller than the screen / 0.6.
for (const [name, viewport, taller] of [["a desktop", null, false], ["a phone", { width: 412, height: 915 }, false],
  ["a landscape phone", { width: 915, height: 412 }, true]]) {
  test(`on ${name} the page character leaves while the twist section fills most of the screen, and comes back after`, { skip: chromeSkip }, async () => {
    const result = await play(twist, [], viewport);
    assert.equal(result.taller, taller, `the section is ${taller ? "" : "not "}taller than the screen / 0.6`);
    assert.deepEqual(result, { errors: [], sliverKept: true, taller, left: true, back: true });
  });
}
