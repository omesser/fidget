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
// reads it again past the 600ms the card waits before walking again.
const landingProbe = `
<script>
  window.addEventListener("load", async () => {
    console.log("__PROBE_READY__");
    const wait = (ms) => new Promise((done) => setTimeout(done, ms));
    const file = (img) => img.getAttribute("src").split("/").pop();
    const cards = {};
    for (const card of document.querySelectorAll("#characters .cast > a")) {
      const img = card.querySelector("img");
      const walk = JSON.parse(img.dataset.frames);
      const walking = walk.includes(img.getAttribute("src"));
      card.dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse" }));
      const hovered = file(img);
      await wait(50);
      card.dispatchEvent(new PointerEvent("pointerleave", { pointerType: "mouse" }));
      await wait(700);
      cards[img.alt] = { href: card.getAttribute("href"), walking, hovered,
        walksAgain: walk.includes(img.getAttribute("src")) };
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

let site;
before((t) => {
  site = join(tempDir(t, "landing-cast-hover-"), "_site");
  const built = spawnSync("bash", ["scripts/assemble-site.sh", site, "/dev/null"], { cwd: ROOT, encoding: "utf8" });
  assert.equal(built.status, 0, `assemble-site.sh failed: ${built.stderr}`);
  for (const [page, probe] of [["index.html", landingProbe], ["characters.html", galleryProbe]]) {
    writeFileSync(join(site, `probe-${page}`), readFileSync(join(site, page), "utf8").replace("<head>", `<head>${probe}`));
  }
});

async function probe(url) {
  const cdp = launchWithPipe(chromePath, [
    "--headless=new", "--disable-gpu", "--no-sandbox", "--disable-dev-shm-usage", "--mute-audio",
    "--allow-file-access-from-files", "--window-size=1280,800",
  ]);
  try {
    const { sessionId, navigate, blockURLs } = await createPageTarget(cdp);
    await blockURLs(["http://*", "https://*"]);
    return await captureConsoleLog(cdp, sessionId, "__PROBE_RESULT__", PROBE_MS * 4, () => navigate(url));
  } finally {
    await cleanupChrome(cdp);
  }
}

test("a hovered cast card plays its gesture, links its gallery section, and walks again after", { skip: chromeSkip }, async () => {
  const cards = await probe(pathToFileURL(join(site, "probe-index.html")).href);
  const gallery = await probe(pathToFileURL(join(site, "probe-characters.html")).href);
  assert.equal(Object.keys(cards).length, gallery.ids.length, "a card per gallery section");
  for (const [name, card] of Object.entries(cards)) {
    const id = card.href.replace(/^characters\.html#/, "");
    assert.notEqual(id, card.href, `${name} links ${card.href}`);
    assert.ok(gallery.ids.includes(id), `${name} links #${id}, which the gallery lacks`);
    assert.ok(card.walksAgain, `${name} is still on its gesture after the mouse left`);
  }
  assert.deepEqual(cards["Rubber Duck"], { href: "characters.html#rubber-duck", walking: true,
    hovered: "wave-0.png", walksAgain: true });
  assert.deepEqual(cards.BMO, { href: "characters.html#bmo", walking: true, hovered: "react-0.png",
    walksAgain: true });
  assert.equal(cards["Black Mage"].hovered, "react-0.png", "a greet that plays react, then talk, starts on react");
});

test("the gallery lands a deep link on its section, returns to the cast, and points only at once tiles", { skip: chromeSkip }, async () => {
  const gallery = await probe(pathToFileURL(join(site, "probe-characters.html")).href + "#rubber-duck");
  assert.equal(gallery.target, "rubber-duck");
  assert.deepEqual(gallery.back, { href: "./#characters", first: true });
  assert.deepEqual(gallery.once, ["pointer"]);
  assert.deepEqual(gallery.looping, ["auto"]);
});
