// "The same moment" on the built landing page: every scene in
// docs/design/moments.json, the first one showing without JavaScript, and the
// button that cycles them, clicked in headless Chrome.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromeSkip, probePage } from "./chrome-cdp.js";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const MOMENTS = JSON.parse(readFileSync(join(ROOT, "docs", "design", "moments.json"), "utf8"));

let scratch;
let page;

before(() => {
  scratch = mkdtempSync(join(tmpdir(), "landing-moments-"));
  execFileSync("python3", [join(ROOT, "scripts", "make-landing-page.py"), "--out", scratch],
    { cwd: ROOT, stdio: "pipe" });
  page = readFileSync(join(scratch, "index.html"), "utf8");
});

after(() => rmSync(scratch, { recursive: true, force: true }));

const section = () => page.match(/<section id="same-moment">([\s\S]*?)<\/section>/)[1];
const scenes = () => [...section().matchAll(/<div class="moment"( hidden)?>([\s\S]*?)<!-- \/moment -->/g)];
const escaped = (text) => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
  .replaceAll('"', "&quot;").replaceAll("'", "&#x27;");

test("every authored scene is on the page, the first one showing", () => {
  const found = scenes();
  assert.equal(found.length, MOMENTS.scenes.length);
  assert.ok(MOMENTS.scenes.length >= 3, "the issue asks for three scenes");
  found.forEach(([, hidden, body], i) => {
    assert.equal(Boolean(hidden), i > 0, `scene ${i + 1} ${i ? "starts hidden" : "shows without JavaScript"}`);
    const scene = MOMENTS.scenes[i];
    for (const line of scene.moment) assert.ok(body.includes(escaped(line)), `scene ${i + 1} sets up: ${line}`);
    assert.equal(scene.reactions.length, 3);
    for (const one of scene.reactions) {
      assert.ok(body.includes(`<div class="say">${escaped(one.line)}</div>`), `scene ${i + 1} says: ${one.line}`);
      assert.ok(body.includes(escaped(one.quote)), `scene ${i + 1} quotes: ${one.quote}`);
      assert.match(body, new RegExp(`<figcaption><b>[^<]+</b> · ${one.behavior}</figcaption>`));
    }
  });
});

test("the authored-samples label sits outside every scene", () => {
  const label = "Authored samples, written by us to show the idea. The site never calls a model.";
  assert.ok(section().includes(`<p class="moment-label">${label}</p>`));
  for (const [, , body] of scenes()) assert.ok(!body.includes(label));
});

test("the button waits for JavaScript to show itself", () => {
  assert.match(section(), /<button type="button" class="moment-next" id="moment-next" hidden>Try another moment ▸<\/button>/);
});

test("Try another moment cycles the scenes and wraps around", { skip: chromeSkip }, async () => {
  const probe = `<script>
    window.addEventListener("load", () => {
      const button = document.getElementById("moment-next");
      const shown = () => [...document.querySelectorAll("#same-moment .moment")].findIndex((m) => !m.hidden);
      const seen = [shown()];
      for (let i = 0; i < ${MOMENTS.scenes.length}; i++) { button.click(); seen.push(shown()); }
      console.log("__PROBE_RESULT__" + JSON.stringify({ visible: !button.hidden, seen }));
    });
  </script>`;
  const result = await probePage(page.replace("</body>", `${probe}</body>`),
    { srcHref: pathToFileURL(ROOT).href, timeout: 10000 });
  const count = MOMENTS.scenes.length;
  assert.deepEqual(result, { visible: true, seen: [...Array(count + 1)].map((_, i) => i % count) });
});
