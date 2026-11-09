// Honesty of the overlay-expression Generated page (ADR-0011). Arithmetic
// already lives in bubble.js; this file only checks that the published page
// loads that module rather than restating it.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import { bubbleDuration } from "../src/bubble.js";
import { tempDir } from "./temp-path.js";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(ROOT, "scripts", "make-expression-page.py");
const FORMULA = /900\s*\+\s*55/;

// The generator owns the line that names the page's sources, and its own
// self-check already asserts the page carries it. Read it from there: a third
// copy is a third thing to forget when a source is added.
const SOURCE_LINE = readFileSync(SCRIPT, "utf8").match(
  /^SOURCE_LINE = "(.+)"$/m,
)[1];

function python(args, options = {}) {
  return execFileSync("python3", [SCRIPT, ...args], {
    cwd: ROOT,
    encoding: "utf8",
    ...options,
  });
}

test("the generator self-check passes", () => {
  const out = python(["--self-check"]);
  assert.match(out, /self-check:/);
});

test("the published page loads bubble.js and calls its machine", (t) => {
  const outDir = tempDir(t, "expression-page-");
  python(["--out", outDir]);
  const html = readFileSync(join(outDir, "expression.html"), "utf8");
  assert.match(html, /type="module"/);
  assert.match(html, /from\s+["']\.\/bubble\.js["']/);
  assert.match(html, /\bbubbleDuration\s*\(/);
  assert.match(html, /\bcreateBubbleMachine\s*\(/);
  assert.doesNotMatch(html, FORMULA, "reading time must come from bubble.js");
  assert.doesNotMatch(
    html,
    /<meta\s+name="robots"\s+content="noindex"/i,
    "Generated pages stay indexed",
  );
  assert.ok(
    html.includes(SOURCE_LINE),
    `the page must name its sources as the generator does: ${SOURCE_LINE}`,
  );
  assert.match(html, /id="wake"/, "a proactive wake must be operable");
  assert.match(
    html,
    /thinking:\s*false/,
    "a proactive wake frames the machine with thinking off",
  );
  assert.doesNotMatch(
    html,
    /THINKING_GRACE_MS\s*=\s*250|THINKING_MIN_HOLD_MS\s*=\s*600/,
    "grace and hold are imported, not restated",
  );

  const css = readFileSync(join(outDir, "bubble.css"), "utf8");
  assert.match(
    css,
    /--bubble-panel:\s*#14171e/,
    "the published page must resolve the bubble tokens, not leave --bubble-* unset",
  );
  assert.match(css, /\.bubble\.visible/);
  assert.match(css, /\[data-mode="speech"\]/);
  assert.match(css, /\[data-mode="thinking"\]/);
  assert.match(css, /\.thinking-dots/);
  assert.doesNotMatch(css, /background:\s*transparent/, "overlay fullscreen rules stay out");

  const copied = readFileSync(join(outDir, "bubble.js"), "utf8");
  assert.equal(copied, readFileSync(join(ROOT, "src", "bubble.js"), "utf8"));
});

test("the copied module is the same bubbleDuration the tests already own", async (t) => {
  const outDir = tempDir(t, "expression-bubble-");
  python(["--out", outDir]);
  const copied = await import(pathToFileURL(join(outDir, "bubble.js")).href);
  const sample = "a".repeat(30);
  assert.equal(copied.bubbleDuration(sample), bubbleDuration(sample));
  assert.equal(typeof copied.createBubbleMachine, "function");
});
