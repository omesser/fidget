// The landing page is Generated (ADR-0038): its words come from README.md and
// its cast from characters/. These tests assemble the site the way pages.yml
// does, so a page that names art the site lacks fails here, not after deploy.

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

// The Assemble block reads the file the step before it fetched from GitHub;
// here it reads a fixture shaped like `gh release view --json tagName,assets`.
function assembleScript(site, release) {
  const workflow = readFileSync(join(ROOT, ".github", "workflows", "pages.yml"), "utf8");
  const block = workflow.match(/- name: Assemble the site\n(?:\s+#.*\n)*\s+run: \|\n((?:\s{10}.*\n)+)/);
  assert.ok(block, "pages.yml has an Assemble the site step");
  assert.ok(block[1].includes("--release release.json"), "Assemble passes the release file to the landing page");
  return block[1].replaceAll("_site", site).replaceAll("release.json", release);
}

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
  [...page.match(/<div class="get" id="get">([\s\S]*?)<\/div>/)[1].matchAll(/href="([^"]+)"/g)].map((m) => m[1]);
// The lines under each button's OS name: its format, then the release tag when
// the button links that release's asset.
const lines = (page) =>
  [...page.match(/<div class="get" id="get">([\s\S]*?)<\/div>/)[1].matchAll(/<a [^>]*>([\s\S]*?)<\/a>/g)]
    .map((m) => [...m[1].matchAll(/<span[^>]*>([^<]*)<\/span>/g)].map((s) => s[1]));

let scratch;
let site;

before(() => {
  scratch = mkdtempSync(join(tmpdir(), "landing-page-"));
  site = join(scratch, "_site");
  const release = releaseFile("release.json", ASSETS);
  execFileSync("bash", ["-ec", assembleScript(site, release)], { cwd: ROOT, stdio: "pipe" });
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
  const loops = [...page.matchAll(/data-frames="([^"]+)"/g)].flatMap((m) =>
    JSON.parse(m[1].replaceAll("&quot;", '"')),
  );
  assert.ok(loops.length > srcs.length, "the cast and the hero carry their loops");
  for (const path of [...srcs, ...loops].filter((p) => !p.startsWith("http"))) {
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

test("each download button names the release tag under its format", () => {
  assert.deepEqual(lines(published(PAGE)), [
    ["Apple Silicon · .dmg", "v0.1.0"],
    ["x86_64 · installer", "v0.1.0"],
    ["x86_64 · AppImage", "v0.1.0"],
  ]);
});

test("a release without an asset sends only that button to the Latest release page", () => {
  const page = pageWith(releaseFile("no-windows.json", ASSETS.filter((asset) => !asset.endsWith(".exe"))));
  assert.deepEqual(buttons(page), [`${DOWNLOAD}/Fidget_0.1.0_aarch64.dmg`, LATEST,
    `${DOWNLOAD}/Fidget_0.1.0_amd64.AppImage`]);
  assert.deepEqual(lines(page), [["Apple Silicon · .dmg", "v0.1.0"], ["x86_64 · installer"],
    ["x86_64 · AppImage", "v0.1.0"]], "a button that links no asset names no version");
});

test("no release file sends every button to the Latest release page", () => {
  const page = pageWith(join(scratch, "absent.json"));
  assert.deepEqual(buttons(page), [LATEST, LATEST, LATEST]);
  assert.deepEqual(lines(page), [["Apple Silicon · .dmg"], ["x86_64 · installer"], ["x86_64 · AppImage"]],
    "no release, no version");
});
