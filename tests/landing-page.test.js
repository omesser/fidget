// The landing page is Generated (ADR-0038): its words come from README.md and
// its cast from characters/. These tests assemble the site with the script
// pages.yml runs, so a page that names art the site lacks fails here, not after deploy.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, before, test } from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SCRIPT = join(ROOT, "scripts", "make-landing-page.py");
const PAGE = "index.html";

// scripts/assemble-site.sh is what pages.yml runs; it reads the file the step
// before it fetched from GitHub, here a fixture shaped like
// `gh release view --json tagName,assets`.
const ASSEMBLE = join(ROOT, "scripts", "assemble-site.sh");

const DOWNLOAD = "https://github.com/omesser/fidget/releases/download/v0.1.0";
const LATEST = "https://github.com/omesser/fidget/releases/latest";
const ASSETS = ["Fidget_0.1.0_aarch64.dmg", "Fidget_0.1.0_amd64.AppImage", "Fidget_0.1.0_amd64.deb",
  "Fidget_0.1.0_x64-setup.exe"];

function releaseFile(name, assets) {
  const path = join(scratch, name);
  writeFileSync(path, JSON.stringify({
    tagName: "v0.1.0",
    assets: assets.map((asset) => ({ name: asset, url: `${DOWNLOAD}/${asset}` })),
  }));
  return path;
}

const buttons = (page) =>
  [...page.match(/<div class="get" id="get"[^>]*>([\s\S]*?)<\/div>/)[1].matchAll(/href="([^"]+)"/g)].map((m) => m[1]);
// The lines under each button's OS name: the release tag when the button links
// that release's asset, then its format.
const lines = (page) =>
  [...page.match(/<div class="get" id="get"[^>]*>([\s\S]*?)<\/div>/)[1].matchAll(/<a [^>]*>([\s\S]*?)<\/a>/g)]
    .map((m) => [...m[1].matchAll(/<span[^>]*>([^<]*)<\/span>/g)].map((s) => s[1]));

let scratch;
let site;

before(() => {
  scratch = mkdtempSync(join(tmpdir(), "landing-page-"));
  site = join(scratch, "_site");
  const release = releaseFile("release.json", ASSETS);
  execFileSync(ASSEMBLE, [site, release], { cwd: ROOT, stdio: "pipe" });
});

after(() => rmSync(scratch, { recursive: true, force: true }));

const published = (name) => readFileSync(join(site, name), "utf8");

test("the generator self-check passes", () => {
  const out = execFileSync("python3", [SCRIPT, "--self-check"], { cwd: ROOT, encoding: "utf8" });
  assert.match(out, /self-check:/);
});

test("the page leads with the README headline", () => {
  const readme = readFileSync(join(ROOT, "README.md"), "utf8");
  const headline = readme.match(/^# (.+)$/m)[1];
  assert.ok(published(PAGE).includes(`<h1>${headline}</h1>`), `the page leads with: ${headline}`);
});

test("the page names every Character a manifest declares", () => {
  const page = published(PAGE);
  const names = readdirSync(join(ROOT, "characters"))
    .filter((dir) => existsSync(join(ROOT, "characters", dir, "character.manifest")))
    .map((dir) =>
      readFileSync(join(ROOT, "characters", dir, "character.manifest"), "utf8").match(/^name = "(.+)"$/m)[1],
    );
  assert.ok(names.length > 1, "characters/ holds the cast");
  for (const name of names) {
    assert.ok(page.includes(`<figcaption>${name}</figcaption>`), `the cast shows ${name}`);
  }
});

test("every frame the page names is in the assembled site", () => {
  const page = published(PAGE);
  const srcs = [...page.matchAll(/src="([^"]+)"/g)].map((m) => m[1]);
  // A <picture> source names its file only in srcset, so a missing copy there
  // breaks the image just for the browsers that pick that source.
  const sets = [...page.matchAll(/srcset="([^"]+)"/g)].flatMap((m) =>
    m[1].split(",").map((c) => c.trim().split(/\s+/)[0]),
  );
  assert.ok(sets.length > 0, "the hero banner names its sources");
  const loops = [...page.matchAll(/data-frames="([^"]+)"/g)].flatMap((m) =>
    JSON.parse(m[1].replaceAll("&quot;", '"')),
  );
  assert.ok(loops.length > srcs.length, "the cast and the hero carry their loops");
  for (const path of [...srcs, ...sets, ...loops].filter((p) => !p.startsWith("http"))) {
    assert.ok(existsSync(join(site, path)), `${path} is published`);
  }
});

test("a fixture README reaches the page, and one with no H1 fails the build", () => {
  const readme = join(scratch, "README.md");
  const out = join(scratch, "fixture");
  const rest = readFileSync(join(ROOT, "README.md"), "utf8").replace(/^# .+$/m, "");

  writeFileSync(readme, `# A fixture headline\n${rest}`);
  execFileSync("python3", [SCRIPT, "--readme", readme, "--out", out], { cwd: ROOT, stdio: "pipe" });
  assert.match(readFileSync(join(out, PAGE), "utf8"), /<h1>A fixture headline<\/h1>/);

  writeFileSync(readme, rest);
  assert.throws(
    () => execFileSync("python3", [SCRIPT, "--readme", readme, "--out", out], { cwd: ROOT, stdio: "pipe" }),
    (error) => /no H1/.test(error.stderr.toString()),
  );
});

// The hero video is the first attachment URL alone on its line, which is how
// GitHub embeds a video in the README. Without one the hero is the desk scene.
function heroFrom(videoLine) {
  const readme = join(scratch, "README.md");
  const out = join(scratch, "hero");
  const source = readFileSync(join(ROOT, "README.md"), "utf8")
    .replace(/^https:\/\/github\.com\/user-attachments\/.*$/gm, "")
    .replace("## What It Does", `${videoLine}\n\n## What It Does`);
  writeFileSync(readme, source);
  execFileSync("python3", [SCRIPT, "--readme", readme, "--out", out], { cwd: ROOT, stdio: "pipe" });
  return readFileSync(join(out, PAGE), "utf8");
}

test("an attachment line in the README becomes the hero video", () => {
  const page = heroFrom("https://github.com/user-attachments/assets/0a1b2c3d-4e5f-6789-abcd-ef0123456789");
  const videos = [...page.matchAll(/<video [^>]*src="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(videos, ["https://github.com/user-attachments/assets/0a1b2c3d-4e5f-6789-abcd-ef0123456789"]);
});

test("a README with no attachment line keeps the sprite hero", () => {
  const page = heroFrom("");
  assert.ok(!page.includes("<video"), "no video without an attachment line");
  assert.ok(page.includes('id="hero-sprite"'), "the desk scene holds the hero sprite");
});

test("a bare URL that is not an attachment is not a video", () => {
  const page = heroFrom("https://github.com/omesser/fidget/releases/download/v0.0.1-dev/demo.mp4");
  assert.ok(!page.includes("<video"), "only a user-attachments URL embeds as a video");
});

// The URL plan in #1210: the root is the landing page, the directory moves to
// /design.html, and every design page it lists stays published.
test("the root is the landing page and every design page stays reachable", () => {
  const pages = ["characters.html", "cues.html", "bubble.html", "chat.html", "chat-mockups.html",
    "expression.html", "window-titles-hint.html"];
  const root = published("index.html");
  const directory = published("design.html");
  assert.match(root, /<link rel="canonical" href="https:\/\/omesser\.github\.io\/fidget\/">/);
  assert.match(root, /<a href="design\.html">Design pages<\/a>/);
  assert.match(directory, /<title>Fidget design pages<\/title>/);
  for (const page of pages) {
    assert.ok(existsSync(join(site, page)), `${page} is published`);
    assert.ok(directory.includes(`href="${page}"`), `design.html links ${page}`);
  }
  assert.ok(!existsSync(join(site, "landing.html")), "nothing publishes landing.html");
});

// #1538: the README links Fidget Cues, so the landing page does too.
test("the landing page links the published Fidget Cues page", () => {
  const root = published("index.html");
  assert.match(root, /<a href="cues\.html">Try the gestures in your browser<\/a>/);
  assert.match(root, /<a href="cues\.html">Fidget Cues<\/a>/);
  assert.ok(existsSync(join(site, "cues.html")), "cues.html is published");
});

test("the root carries a description and an Open Graph image the site publishes", () => {
  const root = published("index.html");
  const description = root.match(/<meta name="description" content="([^"]+)">/);
  assert.ok(description, "the root has a description");
  assert.ok(root.includes(`<meta property="og:description" content="${description[1]}">`), "og:description matches");
  const image = root.match(/<meta property="og:image" content="https:\/\/omesser\.github\.io\/fidget\/([^"]+\.png)">/);
  assert.ok(image, "og:image is an absolute PNG URL on the site");
  assert.ok(existsSync(join(site, image[1])), `${image[1]} is published`);
});

// The asset names carry the version, so each button finds its asset by suffix
// in the Latest release that pages.yml reads at build time.
test("each download button links its asset in the release", () => {
  assert.deepEqual(buttons(published(PAGE)), [
    `${DOWNLOAD}/Fidget_0.1.0_aarch64.dmg`,
    `${DOWNLOAD}/Fidget_0.1.0_x64-setup.exe`,
    `${DOWNLOAD}/Fidget_0.1.0_amd64.AppImage`,
  ]);
});

function pageWith(release) {
  const out = join(scratch, "downloads");
  execFileSync("python3", [SCRIPT, "--out", out, "--release", release], { cwd: ROOT, stdio: "pipe" });
  return readFileSync(join(out, PAGE), "utf8");
}

test("each download button names the release tag above its format", () => {
  assert.deepEqual(lines(published(PAGE)), [
    ["v0.1.0", "Apple Silicon · .dmg"],
    ["v0.1.0", "x86_64 · installer"],
    ["v0.1.0", "x86_64 · AppImage"],
  ]);
});

test("a release without an asset sends only that button to the Latest release page", () => {
  const page = pageWith(releaseFile("no-windows.json", ASSETS.filter((asset) => !asset.endsWith(".exe"))));
  assert.deepEqual(buttons(page), [`${DOWNLOAD}/Fidget_0.1.0_aarch64.dmg`, LATEST,
    `${DOWNLOAD}/Fidget_0.1.0_amd64.AppImage`]);
  assert.deepEqual(lines(page), [["v0.1.0", "Apple Silicon · .dmg"], ["x86_64 · installer"],
    ["v0.1.0", "x86_64 · AppImage"]], "a button that links no asset names no version");
});

test("no release file sends every button to the Latest release page", () => {
  const page = pageWith(join(scratch, "absent.json"));
  assert.deepEqual(buttons(page), [LATEST, LATEST, LATEST]);
  assert.deepEqual(lines(page), [["Apple Silicon · .dmg"], ["x86_64 · installer"], ["x86_64 · AppImage"]],
    "no release, no version");
});

// The OS lead happens in the browser. The generated page must stay the
// default one, with every row the lead script can pick and every note it can
// unhide present but hidden.
test("the static download group keeps three equal buttons and three hidden install lines", () => {
  const page = published(PAGE);
  assert.match(page, /<p class="download-label" id="download-label">Download for<\/p>/,
    "the lead script has no label to name the row it leads with");
  const group = page.match(/<div class="get" id="get"[^>]*>([\s\S]*?)<\/div>/)[1];
  const rows = [...group.matchAll(/data-os="([a-z]+)"/g)].map((m) => m[1]);
  assert.deepEqual(rows, ["macos", "windows", "linux"], "the lead script cannot find its rows in order");
  assert.ok(!page.includes('class="btn primary"') && !page.includes('class="btn quiet"'),
    "a static page led with a row before any OS was known");
  assert.ok(page.includes('<p class="note" data-os-note="macos" hidden>Or install with Homebrew: '
    + "<code>brew install --cask omesser/fidget/fidget</code> "
    + '<button type="button" class="copy" data-copy="brew install --cask omesser/fidget/fidget">Copy</button></p>'),
    "the macOS brew note lost its command or its Copy button");
  assert.ok(page.includes('<p class="note" data-os-note="linux" hidden>Prefer a .deb? '
    + '<a href="https://github.com/omesser/fidget/releases">Get it from Releases</a>.</p>'),
    "the Linux .deb note is wrong or not hidden");
  assert.ok(page.includes("<p class=\"note\" data-os-note=\"unsupported\" hidden>There's no build for your machine yet. "
    + 'You can <a href="https://github.com/omesser/fidget#get-it">run it from source</a>.</p>'),
    "the run-from-source note is wrong or not hidden");
});

test("the OS lead runs as a second plain script with no network calls", () => {
  const page = published(PAGE);
  const blocks = page.match(/<script>\n[\s\S]*?<\/script>/g) ?? [];
  assert.equal(blocks.length, 2);
  assert.ok(blocks[1].includes("download-label"), "the OS lead is not the second script");
  assert.ok(!blocks[1].includes("fetch(") && !blocks[1].includes("XMLHttpRequest"),
    "the OS lead reached the network");
});

test("a README that stops advertising Homebrew empties the install line", () => {
  const readme = join(scratch, "README.md");
  const out = join(scratch, "no-brew");
  writeFileSync(readme, readFileSync(join(ROOT, "README.md"), "utf8")
    .replace("brew install --cask omesser/fidget/fidget", ""));
  execFileSync("python3", [SCRIPT, "--readme", readme, "--out", out], { cwd: ROOT, stdio: "pipe" });
  const page = readFileSync(join(out, PAGE), "utf8");
  assert.ok(page.includes('<p class="note" data-os-note="macos" hidden></p>'),
    "the empty Homebrew note did not stay empty and hidden");
  assert.ok(!page.includes("Or install with Homebrew"),
    "the page claimed a Homebrew line the README does not make");
});

test("the How it works section is a two-lane picture from the README", () => {
  const page = published(PAGE);
  const section = page.match(/<section id="how-it-works">([\s\S]*?)<\/section>/)?.[1];
  assert.ok(section, "the page has no How it works section");
  assert.match(section, /<h2>How it works<\/h2>/);
  assert.match(section, /A pet first\. The AI is the part that makes it someone\./);
  assert.ok(section.includes("<svg"), "the picture is not inline SVG");
  assert.ok(!/mermaid/i.test(section), "the section shipped mermaid");
  assert.ok(!/<script[^>]*(src="[^"]*mermaid|class="[^"]*mermaid)/i.test(page),
    "the page loaded a mermaid script");

  const always = section.match(/<svg[^>]*data-lane="always"[^>]*>[\s\S]*?<\/svg>/)?.[0];
  const director = section.match(/<svg[^>]*data-lane="director"[^>]*>[\s\S]*?<\/svg>/)?.[0];
  assert.ok(always, "lane 1 is not its own svg");
  assert.ok(director, "lane 2 is not its own svg");
  assert.ok(always.includes(">Frame loop</") && always.includes("Walks, falls, lands, perches"),
    "lane 1 svg lost Frame loop");
  assert.ok(always.includes(">You</") && always.includes("Poke, throw, drag, summon."),
    "lane 1 svg lost You");
  assert.ok(always.includes(">Your fidget</"), "lane 1 svg lost Your fidget");
  assert.ok(director.includes(">Personality</") && director.includes("Who it is. A plain text file."),
    "lane 2 svg lost Personality");
  assert.ok(director.includes(">Instance Prompt</") && director.includes("Your own twist, on top."),
    "lane 2 svg lost Instance Prompt");
  assert.ok(director.includes(">Window titles</") && director.includes("Only if you say yes. Never pixels."),
    "lane 2 svg lost Window titles");
  assert.ok(director.includes(">Director</") && director.includes("Wakes now and then"),
    "lane 2 svg lost Director");
  assert.ok(director.includes("attach over ACP on stdio") && director.includes("the Harness calls back over MCP"),
    "lane 2 svg lost the ACP/MCP phrases");

  const withoutDl = section.replace(/<dl[\s\S]*?<\/dl>/, "");
  assert.ok(!withoutDl.includes("<dl"), "the section has more than one dl");
  const alwaysWithoutDl = withoutDl.match(/<svg[^>]*data-lane="always"[^>]*>[\s\S]*?<\/svg>/)?.[0];
  assert.ok(alwaysWithoutDl?.includes(">Frame loop</") && alwaysWithoutDl.includes(">You</"),
    "lane names were only in the dl");

  assert.match(section, /data-lane="always"/);
  assert.match(section, /data-lane="director"/);
  assert.match(section, /data-light="frame"/);
  assert.match(section, /data-light="director"/);
  assert.match(section, /data-light="behavior"/);
  assert.ok(!/class="[^"]*how-motion/.test(page) && !/class="[^"]*how-lit-/.test(page),
    "the published page armed the light-up classes");

  const blocks = page.match(/<script>\n[\s\S]*?<\/script>/g) ?? [];
  assert.equal(blocks.length, 2);
  assert.ok(blocks[0].includes("IntersectionObserver"), "the observer is not in the first script");
  assert.match(blocks[0], /if \(how && !still\)/);
  assert.match(blocks[0], /\[[^\]]*"frame"[^\]]*"director"[^\]]*"behavior"[^\]]*\]/);
  assert.ok(!blocks[0].includes("fetch(") && !blocks[1].includes("fetch("),
    "a How it works script reached the network");

  assert.match(section, /<a href="design\.html">The full design →<\/a>/);
  assert.match(section, /<pre[^>]*>[\s\S]*Opening wake/);
  assert.match(section, /<dl class="how-alt" id="how-alt">[\s\S]*Frame loop[\s\S]*Your agent/);
});
