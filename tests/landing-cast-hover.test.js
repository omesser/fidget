// The Packaged Fidgets cards on the assembled landing page: each links its
// Character's section in the gallery and plays a gesture while hovered, and the
// gallery offers a pointer only on the tiles a click visibly replays.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { before, test } from "node:test";
import { chromeSkip, chromePath, launchWithPipe, createPageTarget, captureConsoleLog, cleanupChrome } from "./chrome-cdp.js";
import { tempDir } from "./temp-path.js";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const PROBE_MS = 5000;

// Hovers a card with a mouse and reads its sprite at once, then leaves and
// reads it again past the 600ms the card waits before resting again.
const landingProbe = `
<script>
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    const wait = (ms) => new Promise((done) => setTimeout(done, ms));
    const file = (img) => img.getAttribute("src").split("/").pop();
    const cards = {};
    for (const card of document.querySelectorAll("#characters .cast > a")) {
      const img = card.querySelector("img");
      const rest = [...JSON.parse(img.dataset.frames), ...JSON.parse(img.dataset.walkFrames)];
      const resting = rest.includes(img.getAttribute("src"));
      card.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse" }));
      const hovered = file(img);
      await wait(50);
      card.dispatchEvent(new PointerEvent("pointerleave", { pointerType: "mouse" }));
      await wait(700);
      cards[img.alt] = { href: card.getAttribute("href"), resting, hovered, after: file(img) };
    }
    console.log("__PROBE_RESULT__" + JSON.stringify(cards));
  });
</script>`;

const galleryProbe = `
<script>
  window.addEventListener("load", () => {
    console.log("__PROBE_READY__");
    const cursors = (selector) => [...new Set([...document.querySelectorAll(selector)]
      .map((stage) => getComputedStyle(stage).cursor))];
    const back = document.querySelector(".controls > a");
    console.log("__PROBE_RESULT__" + JSON.stringify({
      ids: [...document.querySelectorAll("section.character")].map((s) => s.id),
      target: document.querySelector("section.character:target")?.id ?? null,
      once: cursors(".tile:not(.missing) .stage.once"),
      looping: cursors(".tile:not(.missing) .stage:not(.once)"),
      back: back && { href: back.getAttribute("href"), first: back === back.parentElement.firstElementChild },
    }));
  });
</script>`;

// Every card's visible box, which should be one size however its name wraps.
const sizeProbe = `
<script>
  window.addEventListener("load", () => {
    console.log("__PROBE_READY__");
    console.log("__PROBE_RESULT__" + JSON.stringify([...document.querySelectorAll("#characters .cast > a.card > figure")]
      .map((card) => { const box = card.getBoundingClientRect(); return { name: card.textContent.trim(), width: box.width, height: box.height }; })));
  });
</script>`;

// Every frame Rubber Duck's card shows from the first, until a whole resting
// sequence and the first frame of the next have played.
const restProbe = `
<script>
  const shown = [];
  new MutationObserver(() => {
    const img = document.querySelector('img[alt="Rubber Duck"]');
    const src = img && img.getAttribute("src").split("/").pop();
    if (src && src !== shown.at(-1)) shown.push(src);
  }).observe(document.documentElement, { subtree: true, childList: true, attributes: true, attributeFilter: ["src"] });
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    while (shown.length < 38) await new Promise((done) => setTimeout(done, 100));
    console.log("__PROBE_RESULT__" + JSON.stringify(shown));
  });
</script>`;

let site;
before((t) => {
  site = join(tempDir(t, "landing-cast-hover-"), "_site");
  const built = spawnSync("bash", ["scripts/assemble-site.sh", site, "/dev/null"], { cwd: ROOT, encoding: "utf8" });
  assert.equal(built.status, 0, `assemble-site.sh failed: ${built.stderr}`);
  for (const [page, probe, name] of [["index.html", landingProbe, "probe-index.html"],
    ["characters.html", galleryProbe, "probe-characters.html"], ["index.html", sizeProbe, "probe-sizes.html"],
    ["index.html", restProbe, "probe-rest.html"]]) {
    writeFileSync(join(site, name), readFileSync(join(site, page), "utf8").replace("<head>", `<head>${probe}`));
  }
});

async function probe(url, width = 1280) {
  const cdp = launchWithPipe(chromePath, [
    "--headless=new", "--disable-gpu", "--no-sandbox", "--disable-dev-shm-usage", "--mute-audio",
    "--allow-file-access-from-files", `--window-size=${width},800`,
  ]);
  try {
    const { sessionId, navigate, blockURLs } = await createPageTarget(cdp);
    await blockURLs(["http://*", "https://*"]);
    return await captureConsoleLog(cdp, sessionId, "__PROBE_RESULT__", PROBE_MS * 4, () => navigate(url));
  } finally {
    await cleanupChrome(cdp);
  }
}

test("a hovered cast card plays its gesture, links its gallery section, and rests again after", { skip: chromeSkip }, async () => {
  const cards = await probe(pathToFileURL(join(site, "probe-index.html")).href);
  const gallery = await probe(pathToFileURL(join(site, "probe-characters.html")).href);
  assert.equal(Object.keys(cards).length, gallery.ids.length, "a card per gallery section");
  for (const [name, card] of Object.entries(cards)) {
    const id = card.href.replace(/^characters\.html#/, "");
    assert.notEqual(id, card.href, `${name} links ${card.href}`);
    assert.ok(gallery.ids.includes(id), `${name} links #${id}, which the gallery lacks`);
    assert.match(card.after, /^idle-0\.png$/, `${name} did not rest again from its first idle after the mouse left`);
  }
  assert.deepEqual(cards["Rubber Duck"], { href: "characters.html#rubber-duck", resting: true,
    hovered: "wave-0.png", after: "idle-0.png" });
  assert.deepEqual(cards.BMO, { href: "characters.html#bmo", resting: true, hovered: "react-0.png",
    after: "idle-0.png" });
  assert.equal(cards["Black Mage"].hovered, "react-0.png", "a greet that plays react, then talk, starts on react");
});

test("the gallery lands a deep link on its section, returns to the cast, and points only at once tiles", { skip: chromeSkip }, async () => {
  const gallery = await probe(pathToFileURL(join(site, "probe-characters.html")).href + "#rubber-duck");
  assert.equal(gallery.target, "rubber-duck");
  assert.deepEqual(gallery.back, { href: "./#characters", first: true });
  assert.deepEqual(gallery.once, ["pointer"]);
  assert.deepEqual(gallery.looping, ["auto"]);
});

test("every cast card is the same size at desktop, tablet and phone widths", { skip: chromeSkip }, async () => {
  // 960 puts Tux alone on a second row, where only its own one-line name set its height.
  for (const width of [1280, 960, 700, 390]) {
    const cards = await probe(pathToFileURL(join(site, "probe-sizes.html")).href, width);
    assert.ok(cards.length > 1, `no cast cards at ${width}px`);
    const [first] = cards;
    for (const card of cards) {
      // A grid splits leftover pixels between columns, so widths may differ by a fraction.
      assert.ok(Math.abs(card.width - first.width) < 0.5 && Math.abs(card.height - first.height) < 0.5,
        `at ${width}px ${card.name} is ${card.width}x${card.height}, ${first.name} is ${first.width}x${first.height}`);
    }
  }
});

test("a resting cast card plays idle, idle, walk, walk, idle, then starts over", { skip: chromeSkip }, async () => {
  const shown = await probe(pathToFileURL(join(site, "probe-rest.html")).href);
  const idle = ["idle-0.png", "idle-1.png", "idle-2.png", "idle-3.png", "idle-4.png", "idle-5.png", "idle-6.png"];
  const walk = ["walk-0.png", "walk-1.png", "walk-2.png", "walk-3.png", "walk-4.png", "walk-5.png", "walk-6.png", "walk-7.png"];
  assert.deepEqual(shown.slice(0, 38), [...idle, ...idle, ...walk, ...walk, ...idle, "idle-0.png"]);
});
