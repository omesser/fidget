// "Give it your own twist" on the built landing page: the README's Instance
// Prompts word for word, every Character's before and after line from
// docs/design/twists.json, and the pickers, clicked in headless Chrome.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromeSkip, probePage } from "./chrome-cdp.js";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const TWISTS = JSON.parse(readFileSync(join(ROOT, "docs", "design", "twists.json"), "utf8"));
const README = readFileSync(join(ROOT, "README.md"), "utf8");
const PROMPTS = [...README.split("### Give It an Instance Prompt")[1].split("\n## ")[0]
  .matchAll(/^\| `([^`]+)` \|/gm)].map((row) => row[1]);
const NEEDS = "Needs an attached Harness that may use its tools, and window names consent.";

let scratch;
let page;

before(() => {
  scratch = mkdtempSync(join(tmpdir(), "landing-twist-"));
  execFileSync("python3", [join(ROOT, "scripts", "make-landing-page.py"), "--out", scratch],
    { cwd: ROOT, stdio: "pipe" });
  page = readFileSync(join(scratch, "index.html"), "utf8");
});

after(() => rmSync(scratch, { recursive: true, force: true }));

const section = () => page.match(/<section id="twist">([\s\S]*?)<\/section>/)[1];
const data = () => JSON.parse(section().match(/<script type="application\/json" id="twist-data">([\s\S]*?)<\/script>/)[1]);
const escaped = (text) => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
  .replaceAll('"', "&quot;").replaceAll("'", "&#x27;");

test("each prompt is the README's, word for word", () => {
  const shown = data().prompts.map((p) => p.text);
  assert.equal(shown.length, 3);
  for (const text of shown) assert.ok(PROMPTS.includes(text), `the README holds: ${text}`);
  assert.deepEqual(data().prompts.map((p) => p.name), TWISTS.prompts.map((p) => p.name));
});

test("four Characters each carry a before line and an after line for every prompt", () => {
  const cast = data().characters;
  assert.deepEqual(cast.map((c) => c.character), ["bmo", "black-mage", "cat", "trump"]);
  cast.forEach((one, i) => {
    const authored = TWISTS.characters[i];
    assert.equal(one.before.line, authored.before.line);
    assert.ok(one.before.loop.frames[0].startsWith(`characters/${one.character}/`));
    assert.equal(one.after.length, 3);
    one.after.forEach((line, p) => {
      const written = authored.after[TWISTS.prompts[p].name];
      assert.equal(line.line, written.line);
      assert.equal(line.when, written.when || TWISTS.prompts[p].when);
      assert.ok(line.loop.frames.length > 0);
    });
  });
});

test("without JavaScript BMO answers the rubber duck prompt", () => {
  const body = section();
  const bmo = TWISTS.characters[0];
  assert.ok(body.includes(`<p class="twist-prompt" id="twist-prompt">${escaped(data().prompts[0].text)}</p>`));
  assert.ok(body.includes(`<div class="say" id="twist-before-say">${escaped(bmo.before.line)}</div>`));
  assert.ok(body.includes(`<div class="say" id="twist-after-say">${escaped(bmo.after["Rubber duck"].line)}</div>`));
  assert.match(body, /<div class="twist-pick" id="twist-pick" hidden>/, "the pickers wait for JavaScript");
});

test("the setup, the label and the README link are always on show", () => {
  const body = section();
  assert.ok(body.includes(`<p class="twist-needs">${NEEDS}</p>`));
  assert.ok(body.includes('<p class="twist-label">Authored samples. The site never calls a model.</p>'));
  assert.ok(body.includes('<a href="https://github.com/omesser/fidget#give-it-an-instance-prompt">Write your own: how →</a>'));
});

const pickEverything = `
  const say = (id) => document.getElementById(id).textContent;
  const typed = (line) => new Promise((done) => {
    const target = document.getElementById("twist-after-say");
    if (target.textContent === line) return done();
    const watch = new MutationObserver(() => {
      if (target.textContent === line) { watch.disconnect(); done(); }
    });
    watch.observe(target, { childList: true, characterData: true, subtree: true });
  });
  window.addEventListener("load", async () => {
    const twist = JSON.parse(document.getElementById("twist-data").textContent);
    const seen = [];
    for (const one of twist.characters) {
      document.querySelector('#twist-pick [data-character="' + one.character + '"]').click();
      for (let p = 0; p < twist.prompts.length; p++) {
        document.querySelector('#twist-pick [data-prompt="' + p + '"]').click();
        const line = one.after[p].line;
        const partial = say("twist-after-say");
        await typed(line);
        seen.push({
          prefix: line.startsWith(partial),
          prompt: say("twist-prompt") === twist.prompts[p].text,
          before: say("twist-before-say") === one.before.line,
          when: say("twist-when") === one.after[p].when,
          sprites: document.getElementById("twist-before-sprite").getAttribute("src").startsWith("characters/" + one.character + "/")
            && one.after[p].loop.frames.includes(document.getElementById("twist-after-sprite").getAttribute("src")),
          pressed: [...document.querySelectorAll("#twist-pick [aria-pressed=true]")].map((b) => b.textContent),
          partial: partial === line,
        });
      }
    }
    console.log("__PROBE_RESULT__" + JSON.stringify({ shown: !document.getElementById("twist-pick").hidden, seen }));
  });`;

async function pick(args) {
  return probePage(page.replace("</body>", `<script>${pickEverything}</script></body>`),
    { srcHref: pathToFileURL(ROOT).href, timeout: 60000, args });
}

function expected(typedIn) {
  return TWISTS.characters.flatMap((one, i) => TWISTS.prompts.map((prompt) => ({
    prefix: true, prompt: true, before: true, when: true, sprites: true,
    pressed: [data().characters[i].name, prompt.name], partial: !typedIn,
  })));
}

test("every Character and prompt pair shows its lines, the after line typing in", { skip: chromeSkip }, async () => {
  assert.deepEqual(await pick([]), { shown: true, seen: expected(true) });
});

test("under reduced motion the after line lands whole", { skip: chromeSkip }, async () => {
  assert.deepEqual(await pick(["--force-prefers-reduced-motion"]), { shown: true, seen: expected(false) });
});
