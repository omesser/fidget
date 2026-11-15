// The built landing page shows the README's Instance Prompts word for word,
// each prompt's one wake, and every Character's line from website/twists.json.
// The pickers are clicked in headless Chrome.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { before, test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { tempDir } from "./temp-path.js";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const TWISTS = JSON.parse(readFileSync(join(ROOT, "website", "twists.json"), "utf8"));
const README = readFileSync(join(ROOT, "README.md"), "utf8");
const PROMPTS = [...README.split("### Give It an Instance Prompt")[1].split("\n## ")[0]
  .matchAll(/^\| `([^`]+)` \|/gm)].map((row) => row[1]);
const ANY_MODEL = "Works with any attached model.";
const CONSENT = "Needs window names consent.";
const LABEL = "Examples pre-generated with real in-app and character prompts. The site isn't connected to an LLM. "
  + '<a href="#get">Download Fidget</a> to try the real thing.';

let scratch;
let page;

before((t) => {
  scratch = tempDir(t, "landing-twist-");
  execFileSync("python3", [join(ROOT, "scripts", "make-landing-page.py"), "--out", scratch],
    { cwd: ROOT, stdio: "pipe" });
  page = readFileSync(join(scratch, "index.html"), "utf8");
});

const section = () => page.match(/<section id="twist">([\s\S]*?)<\/section>/)[1];
const data = () => JSON.parse(section().match(/<script type="application\/json" id="twist-data">([\s\S]*?)<\/script>/)[1]);
// Builds the expected text the way the generator escapes it. Nothing here is sanitized.
// nosemgrep: javascript.audit.detect-replaceall-sanitization.detect-replaceall-sanitization
const escaped = (text) => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
  .replaceAll('"', "&quot;").replaceAll("'", "&#x27;");

test("each prompt is the README's, word for word", () => {
  const shown = data().prompts.map((p) => p.text);
  assert.equal(shown.length, 4);
  for (const text of shown) assert.ok(PROMPTS.includes(text), `the README holds: ${text}`);
  assert.deepEqual(data().prompts.map((p) => p.name), TWISTS.prompts.map((p) => p.name));
});

test("each prompt carries one wake that every Character answers", () => {
  assert.deepEqual(data().prompts.map((p) => p.wake), [
    "Thrown across the screen.", 'You said: "Leftover pizza."', "Perched on Spotify, playing Dancing Queen.",
    'You said: "Three meetings, then the dentist."',
  ]);
  const cast = data().characters;
  assert.deepEqual(cast.map((c) => c.character), ["bmo", "black-mage", "cat", "trump", "buddy-bot"]);
  for (const one of cast) {
    assert.equal(one.lines.length, 4);
    one.lines.forEach((said, p) => {
      assert.equal(said.line, TWISTS.prompts[p].lines[one.character].line);
      assert.ok(said.loop.frames[0].startsWith(`characters/${one.character}/`));
      assert.deepEqual(Object.keys(said).sort(), ["line", "loop"], "a Character's line carries no wake of its own");
    });
  }
});

test("without JavaScript BMO answers the stunt double wake, and nothing shows a Before", () => {
  const body = section();
  const line = escaped(TWISTS.prompts[0].lines.bmo.line);
  assert.ok(body.includes('<figure class="twist-layer"><figcaption>What you\'d type in the app\'s Prompt tab</figcaption>'
    + `<blockquote class="twist-prompt" id="twist-prompt">${escaped(data().prompts[0].text)}</blockquote></figure>`),
    "the prompt is a captioned quote, not a field");
  assert.ok(body.includes('<p class="twist-when"><span class="twist-what">What happened:</span> '
    + '<span id="twist-when">Thrown across the screen.</span></p>'), "the wake is labelled as what happened");
  assert.ok(body.includes(`<div class="say" id="twist-say" aria-hidden="true">${line}</div>`
    + `<p class="twist-live" id="twist-live" aria-live="polite">${line}</p>`),
    "screen readers get the line from the live region, not the typing bubble");
  assert.doesNotMatch(body, /Before|Personality only|twist-before|twist-arrow/);
  assert.match(body, /<div class="twist-pick" id="twist-pick" hidden>/, "the pickers wait for JavaScript");
});

test("only Theatre critic asks for window names consent in the small print", () => {
  assert.deepEqual(data().prompts.map((p) => [p.name, p.needs]), [
    ["Stunt double", ANY_MODEL], ["Food critic", ANY_MODEL], ["Theatre critic", CONSENT], ["Forecast", ANY_MODEL],
  ]);
  assert.ok(section().includes(`<p class="twist-needs" id="twist-needs">${ANY_MODEL}</p>`));
});

test("the label and the README link are always on show", () => {
  const body = section();
  assert.ok(body.includes(`<p class="twist-label">${LABEL}</p>`));
  assert.match(page, /<div class="get" id="get"/, "Download Fidget links to the download buttons on the page");
  assert.ok(body.includes('<a href="https://github.com/omesser/fidget#give-it-an-instance-prompt">Write your own: how →</a>'));
});

const pickEverything = `
  const say = (id) => document.getElementById(id).textContent;
  const typed = (line) => new Promise((done) => {
    const target = document.getElementById("twist-say");
    if (target.textContent === line) return done();
    const watch = new MutationObserver(() => {
      if (target.textContent === line) { watch.disconnect(); done(); }
    });
    watch.observe(target, { childList: true, characterData: true, subtree: true });
  });
  const luminance = (rgb) => {
    const [r, g, b] = rgb.match(/\\d+/g).slice(0, 3).map((v) => {
      const c = Number(v) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  window.addEventListener("load", async () => {
    const twist = JSON.parse(document.getElementById("twist-data").textContent);
    const bubble = document.getElementById("twist-say");
    const sprite = document.getElementById("twist-sprite");
    const below = () => [".twist-art", ".twist-foot"].map((s) => document.querySelector(s).getBoundingClientRect().top + scrollY);
    const heights = new Set();
    const belowTops = twist.prompts.map(() => new Set());
    const seen = [];
    for (const one of twist.characters) {
      document.querySelector('#twist-pick [data-character="' + one.character + '"]').click();
      for (let p = 0; p < twist.prompts.length; p++) {
        document.querySelector('#twist-pick [data-prompt="' + p + '"]').click();
        const line = one.lines[p].line;
        const partial = say("twist-say");
        const announced = say("twist-live") === line;
        heights.add(bubble.getBoundingClientRect().height);
        belowTops[p].add(below().join());
        await typed(line);
        heights.add(bubble.getBoundingClientRect().height);
        belowTops[p].add(below().join());
        seen.push({
          prefix: line.startsWith(partial),
          prompt: say("twist-prompt") === twist.prompts[p].text,
          needs: say("twist-needs") === twist.prompts[p].needs,
          wake: say("twist-when") === twist.prompts[p].wake,
          sprite: one.lines[p].loop.frames.includes(sprite.getAttribute("src")),
          pressed: [...document.querySelectorAll("#twist-pick [aria-pressed=true]")].map((b) => b.textContent),
          partial: partial === line,
          announced,
        });
      }
    }
    const on = getComputedStyle(document.querySelector("#twist-pick [aria-pressed=true]"));
    const [light, dark] = [luminance(on.color), luminance(on.backgroundColor)].sort((a, b) => b - a);
    console.log("__PROBE_RESULT__" + JSON.stringify({
      shown: !document.getElementById("twist-pick").hidden,
      live: [document.getElementById("twist-live").getAttribute("aria-live"), bubble.getAttribute("aria-hidden")],
      bubbleHeights: heights.size, belowMoves: belowTops.map((tops) => tops.size),
      contrastPasses: (light + 0.05) / (dark + 0.05) >= 4.5, seen,
    }));
  });`;

async function pick(args) {
  return probePage(page.replace("</body>", `<script>${pickEverything}</script></body>`),
    // Injects this file's own picker script into the built page.
    // nosemgrep: javascript.lang.security.audit.unknown-value-with-script-tag.unknown-value-with-script-tag
    { srcHref: pathToFileURL(ROOT).href, timeout: 60000, args });
}

function expected(typedIn) {
  return {
    shown: true, live: ["polite", "true"], bubbleHeights: 1, belowMoves: [1, 1, 1, 1], contrastPasses: true,
    seen: TWISTS.characters.flatMap((one, i) => TWISTS.prompts.map((prompt) => ({
      prefix: true, prompt: true, needs: true, wake: true, sprite: true,
      pressed: [data().characters[i].name, prompt.name], partial: !typedIn, announced: true,
    }))),
  };
}

test("in a narrow window every pair types its line into a bubble that never resizes", { skip: chromeSkip }, async () => {
  assert.deepEqual(await pick(["--window-size=390,900"]), expected(true));
});

test("at desktop width under reduced motion the line lands whole", { skip: chromeSkip }, async () => {
  assert.deepEqual(await pick(["--window-size=1280,900", "--force-prefers-reduced-motion"]), expected(false));
});
