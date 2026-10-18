// The "What's new" strip: the landing page names the Latest release and the
// bullets under its `## Highlights` heading. scripts/make-landing-page.py reads
// them at deploy, so these tests drive the generator against a captured v0.6.0
// body — `gh release view v0.6.0 --json body` output, kept with LF endings
// because the repo's pre-commit forces LF; one test rebuilds the CRLF form gh
// writes.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, before, test } from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(ROOT, "scripts", "make-landing-page.py");
const PAGE = "index.html";
const RELEASES = "https://github.com/omesser/fidget/releases";
const REAL_BODY = readFileSync(join(ROOT, "tests", "fixtures", "release-body-v0.6.0.md"), "utf8");

// The first three bullets under that body's `## Highlights`, as the strip renders
// them. Only a release's own written lines reach the page: GitHub's generated
// entries — `* fix(scope): Subject by @author in <pull request>` — are commit
// subjects, and a release carrying nothing else shows no strip.
const FIRST_THREE = [
  "<b>Quick message pill on Windows:</b> when the pill closes, Fidget gives the keyboard"
    + " back to the window you were using, and a pill opened by hover stays open.",
  "<b>Hover delay setting:</b> a row on the Development tab in Settings sets how long the"
    + " pointer rests on the character before the pill opens.",
  "<b>Chat:</b> a failed turn shows its error in Chat, and an ask's title shows as sent,"
    + " with backticked spans as code.",
];

// The two notes sections GitHub writes on its own, from that same body.
const GENERATED = [
  "## What's Changed",
  "* test(verify): Prove macOS Chat perch clicks with command traces by @omesser in https://github.com/omesser/fidget/pull/1512",
  "* fix(site): Make download buttons consistent by moving 'Download for' to a label by @omesser in https://github.com/omesser/fidget/pull/1513",
  "* build(deps): bump serde_json from 1.0.151 to 1.0.154 by @dependabot[bot] in https://github.com/omesser/fidget/pull/1531",
].join("\n");

let scratch;
before(() => { scratch = mkdtempSync(join(tmpdir(), "landing-whats-new-")); });
after(() => rmSync(scratch, { recursive: true, force: true }));

function pageWith(name, release) {
  const out = join(scratch, name);
  const args = [SCRIPT, "--out", out];
  if (release) {
    const file = join(scratch, `${name}.json`);
    writeFileSync(file, JSON.stringify(release));
    args.push("--release", file);
  }
  execFileSync("python3", args, { cwd: ROOT, stdio: "pipe" });
  return readFileSync(join(out, PAGE), "utf8");
}

const strip = (page) => page.match(/<p class="whats-new">([\s\S]*?)<\/p>/)?.[1] ?? null;
const tail = ` <a href="${RELEASES}">All releases →</a>`;

test("the strip names the tag and the first three Highlights bullets of a real release body", () => {
  const page = pageWith("real", { tagName: "v0.6.0", assets: [], body: REAL_BODY });
  assert.equal(strip(page), `<b>New in v0.6.0</b> · ${FIRST_THREE.join(" · ")}${tail}`);
  // Under the hero, above the features.
  assert.ok(page.indexOf('class="whats-new"') > page.indexOf("<h1>"), "the strip is not below the hero");
  assert.ok(page.indexOf('class="whats-new"') < page.indexOf("What it does"), "the strip is not above the features");
});

test("a release whose body carries only GitHub's generated notes shows no strip", () => {
  const page = pageWith("generated", { tagName: "v0.6.0", assets: [], body: GENERATED });
  assert.equal(strip(page), null, "a commit subject reached the landing page");
  assert.ok(!page.includes("All releases"), "the page linked All releases with nothing to show");
  for (const dropped of ["Make download buttons consistent", "bump serde_json", "Prove macOS Chat perch"]) {
    assert.ok(!page.includes(dropped), `the page showed ${dropped}`);
  }
});

test("a Highlights heading with no bullets of its own shows no strip", () => {
  const page = pageWith("empty", { tagName: "v1", assets: [],
    body: "## Highlights\n\n## Summary\n- **Chat:** a Summary bullet, which is not a Highlight\n" });
  assert.equal(strip(page), null, "the strip read a bullet outside its own Highlights list");
});

test("a Highlights bullet renders as inline Markdown, and a link the page cannot resolve stays text", () => {
  const page = pageWith("markdown", { tagName: "v1", assets: [], body:
    "## Highlights\n- **Chat** can be a `perch` now\n- A [stray](docs/x.md) link\n" });
  assert.equal(strip(page), "<b>New in v1</b> · <b>Chat</b> can be a <code>perch</code> now"
    + ` · A [stray](docs/x.md) link${tail}`);
});

test("a CRLF release body — what gh release view writes — reads the same as an LF one", () => {
  const crlf = pageWith("crlf", { tagName: "v0.6.0", assets: [], body: REAL_BODY.replace(/\n/g, "\r\n") });
  const lf = pageWith("lf", { tagName: "v0.6.0", assets: [], body: REAL_BODY });
  assert.equal(strip(crlf), `<b>New in v0.6.0</b> · ${FIRST_THREE.join(" · ")}${tail}`);
  assert.equal(strip(lf), strip(crlf));
});

test("no release file hides the strip", () => {
  const page = pageWith("absent");
  assert.equal(strip(page), null, "the page drew a strip with no release to name");
  assert.ok(!page.includes("All releases"), "the page linked All releases with no release to name");
});
