#!/usr/bin/env node
// Records the possess and unpossess cues as GIFs, for a PR or a review.
// The real overlay page (src/index.html) runs in headless Chrome under a
// stubbed Tauri, and each frame is a screenshot at a seeked animation time,
// so the GIF plays at real speed whatever the machine.
//
//   node scripts/record-possess-gifs.mjs OUT_DIR
//
// Needs ffmpeg, and Chrome where tests/chrome-cdp.js finds it (FIDGET_CHROME).

import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromePath, cleanupChrome, createPageTarget, injectBase, launchWithPipe } from "../tests/chrome-cdp.js";

const ROOT = fileURLToPath(new URL("../", import.meta.url));
const out = process.argv[2];
if (!out || !chromePath) {
  console.error(out ? "no Chrome: set FIDGET_CHROME" : "usage: record-possess-gifs.mjs OUT_DIR");
  process.exit(2);
}

const FPS = 50;
const VIEW = { width: 800, height: 480 };
const DESKTOP = { r: 94, g: 110, b: 138, a: 1 };
const png = (path) => `data:image/png;base64,${readFileSync(join(ROOT, path)).toString("base64")}`;
const CAST = {
  BMO: { art: { idle: [png("characters/bmo/frames/idle-0.png")] }, smooth: true, size: [126, 128] },
  "Black Mage": { art: { idle: [png("characters/black-mage/frames/idle-0.png")] }, smooth: false, size: [111, 99] },
};

// Each runs for `ms` of animation, with a still before and after so a loop reads.
const SHOTS = [
  { name: "possess-mid", cue: "possess", who: "BMO", x: 337, y: 300, ms: 600 },
  { name: "unpossess-mid", cue: "unpossess", who: "BMO", x: 337, y: 300, ms: 360 },
  { name: "possess-right-edge", cue: "possess", who: "Black Mage", x: VIEW.width - 115, y: 330, mirror: -1, ms: 600 },
  { name: "possess-top", cue: "possess", who: "Black Mage", x: 340, y: 8, ms: 600 },
  { name: "possess-reduced-motion", cue: "possess", who: "BMO", x: 337, y: 300, ms: 220, reduced: true },
];

function page() {
  const stub = `
<script>
  const handlers = {};
  const cast = ${JSON.stringify(Object.fromEntries(Object.entries(CAST).map(([k, v]) => [k, { art: v.art, smooth: v.smooth }])))};
  window.__TAURI__ = {
    core: { invoke: (name) => Promise.resolve(name === "character" ? { characters: cast } : undefined) },
    event: { listen(name, handler) { handlers[name] = handler; return Promise.resolve(() => {}); } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "overlay-gif" }) },
  };
  const settle = () => new Promise((resolve) => requestAnimationFrame(resolve));
  async function until(pred) { while (!pred()) await settle(); }
  window.__cue = async (shot, size) => {
    await until(() => handlers.frame);
    const sprite = { id: "gif", character: shot.who, animation: "idle", frame_index: 0, x: shot.x, y: shot.y,
      width: size[0], height: size[1], mirror: shot.mirror ?? 1, dialogue: null, thinking: false, asking: false,
      bubble: true, cue: null };
    const frame = (cue) => ({ sprites: [{ ...sprite, cue }], visible: true, fade_ms: 0, sound: false });
    handlers.frame({ payload: frame(null) });
    const img = () => document.querySelector('.sprite[data-instance="gif"]');
    await until(() => img()?.currentSrc && img().complete);
    await settle();
    handlers.frame({ payload: frame(shot.cue) });
    for (const a of document.getAnimations()) a.pause();
  };
  window.__seek = (ms) => {
    for (const a of document.getAnimations()) a.currentTime = ms;
  };
</script>`;
  return injectBase(readFileSync(join(ROOT, "src/index.html"), "utf8").replace("<head>", `<head>${stub}`), pathToFileURL(join(ROOT, "src/")).href);
}

// The ghost reaches about 175 px off the sprite's centre at most, upward or to a side.
function crop(shot) {
  const [w, h] = CAST[shot.who].size;
  const cx = shot.x + w / 2;
  const x = Math.max(0, cx - 175);
  const y = Math.max(0, shot.y - 190);
  const right = Math.min(VIEW.width, cx + 175);
  const bottom = Math.min(VIEW.height, shot.y + h + 12);
  return { x, y, width: right - x, height: bottom - y, scale: 1 };
}

async function record(cdp, shot, dir) {
  const { sessionId, navigate } = await createPageTarget(cdp);
  const send = (method, params) => cdp.send(method, params, sessionId);
  await send("Emulation.setDeviceMetricsOverride", { ...VIEW, deviceScaleFactor: 2, mobile: false });
  await send("Emulation.setDefaultBackgroundColorOverride", { color: DESKTOP });
  await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: shot.reduced ? "reduce" : "no-preference" }] });
  const file = join(dir, "page.html");
  writeFileSync(file, page());
  await navigate(pathToFileURL(file).href);
  const evaluate = async (expression) => {
    const { exceptionDetails } = await send("Runtime.evaluate", { expression, awaitPromise: true });
    if (exceptionDetails) throw new Error(`${shot.name}: ${exceptionDetails.exception?.description ?? exceptionDetails.text}`);
  };
  await evaluate(`new Promise((r) => document.readyState === "complete" ? r() : addEventListener("load", r, { once: true }))`);
  await evaluate(`window.__cue(${JSON.stringify(shot)}, ${JSON.stringify(CAST[shot.who].size)})`);
  const clip = crop(shot);
  const frames = [];
  const shoot = async () => (await send("Page.captureScreenshot", { format: "png", clip })).data;
  await evaluate("window.__seek(0)");
  const still = await shoot();
  const hold = (ms) => Array.from({ length: Math.round((ms * FPS) / 1000) }, () => still);
  frames.push(...hold(400));
  for (let ms = 0; ms <= shot.ms; ms += 1000 / FPS) {
    await evaluate(`window.__seek(${ms})`);
    frames.push(await shoot());
  }
  frames.push(...Array.from({ length: Math.round((600 * FPS) / 1000) }, () => frames.at(-1)));
  frames.forEach((data, i) => writeFileSync(join(dir, `${String(i).padStart(4, "0")}.png`), Buffer.from(data, "base64")));
}

mkdirSync(out, { recursive: true });
for (const shot of SHOTS) {
  const dir = join(out, `${shot.name}.frames`);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir);
  const cdp = launchWithPipe(chromePath, ["--headless=new", "--disable-gpu", "--no-sandbox", "--mute-audio", "--allow-file-access-from-files"]);
  try {
    await record(cdp, shot, dir);
  } finally {
    await cleanupChrome(cdp);
  }
  const gif = join(out, `${shot.name}.gif`);
  const made = spawnSync("ffmpeg", ["-loglevel", "error", "-y", "-framerate", String(FPS), "-i", join(dir, "%04d.png"),
    "-vf", "split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle", gif], { encoding: "utf8" });
  if (made.status !== 0) throw new Error(`ffmpeg ${shot.name}: ${made.stderr}`);
  rmSync(dir, { recursive: true, force: true });
  console.log(`${gif} ${(statSync(gif).size / 1024).toFixed(0)} KiB`);
}
