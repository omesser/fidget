// What the overlay tells the Shell it draws outside the art. Windows keeps every
// rect in the window's region so none is clipped, and the frame loop takes a
// click only over the clickable ones. Drives the real src/index.html and main.js.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

function chromeBin() {
  if (process.env.FIDGET_CHROME) {
    return process.env.FIDGET_CHROME;
  }
  for (const name of ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]) {
    const found = spawnSync("bash", ["-lc", `command -v ${name}`], { encoding: "utf8" });
    const path = found.stdout.trim();
    if (found.status === 0 && path) {
      return path;
    }
  }
  return null;
}

const chrome = chromeBin();

const sprite = (id, x, extra) => ({
  id,
  character: "Buddy Bot",
  animation: "idle",
  frame_index: 0,
  x,
  y: 400,
  width: 64,
  height: 64,
  mirror: 1,
  bubble: true,
  ...extra,
});

// Long enough to truncate, which is what puts "Open chat" in the bubble.
const longLine = "word ".repeat(200).trim();

const frame = {
  sprites: [
    sprite("talker", 100, { dialogue: longLine }),
    sprite("thinker", 600, { thinking: true }),
    sprite("idler", 1000),
  ],
  visible: true,
  fade_ms: 0,
  sound: false,
};

// Every bubble moved to another display's overlay.
const handedOff = {
  ...frame,
  sprites: frame.sprites.map(({ dialogue, thinking, ...rest }) => ({ ...rest, bubble: false })),
};

// One launch for every test here: a headless Chrome start is most of the cost.
let probe = null;

function drive() {
  probe ??= launch();
  return probe;
}

function launch() {
  const stub = `
<script type="module">
  const handlers = {};
  window.__invoked = [];
  window.__TAURI__ = {
    core: {
      invoke(name, args) {
        window.__invoked.push({ name, args: structuredClone(args) });
        if (name === "character") return Promise.resolve({ characters: {} });
        if (name === "overlay_hit_tests_hotspots") return Promise.resolve(true);
        return Promise.resolve();
      },
    },
    event: {
      listen(name, handler) {
        handlers[name] = handler;
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "overlay-test" }) },
  };
  const tick = () => new Promise((r) => setTimeout(r, 50));
  const box = (el, tail = 0) => {
    const r = el.getBoundingClientRect();
    return [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height) + tail];
  };
  async function drive() {
    while (!handlers.frame) await tick();
    handlers.frame({ payload: ${JSON.stringify(frame)} });
    const quick = document.querySelector('.quick-message[data-instance="idler"]');
    document.querySelector('.sprite[data-instance="idler"]').dispatchEvent(new PointerEvent("pointerenter"));
    while (!quick.classList.contains("visible")) await tick();
    for (let i = 0; i < 10; i += 1) await tick();
    const bubble = (id) => document.querySelector('.bubble:not(.quick-message)[data-instance="' + id + '"]');
    const rects = () => window.__invoked.filter((c) => c.name === "overlay_rects").map((c) => c.args.rects);
    const reported = rects().at(-1) ?? null;
    const expected = {
      openChat: box(bubble("talker").querySelector(".bubble-more")),
      speech: box(bubble("talker"), 10),
      dots: box(bubble("thinker"), 10),
      pill: box(quick),
    };
    handlers.frame({ payload: ${JSON.stringify(handedOff)} });
    for (let i = 0; i < 10; i += 1) await tick();
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify({
      reported,
      expected,
      handedOff: rects().at(-1) ?? null,
      sent: rects().map((r) => JSON.stringify(r)),
    });
    document.body.append(out);
  }
  window.addEventListener("load", drive);
</script>`;

  const dir = mkdtempSync(join(tmpdir(), "overlay-rects-"));
  const page = join(dir, "harness.html");
  const html = readFileSync(join(SRC, "index.html"), "utf8").replace(
    "<head>",
    `<head><base href="${pathToFileURL(SRC).href}">${stub}`,
  );
  writeFileSync(page, html);

  const run = spawnSync(
    chrome,
    [
      "--headless",
      "--disable-gpu",
      "--no-sandbox",
      "--disable-dev-shm-usage",
      `--user-data-dir=${join(dir, "profile")}`,
      "--allow-file-access-from-files",
      "--window-size=1400,900",
      "--virtual-time-budget=8000",
      "--dump-dom",
      pathToFileURL(page).href,
    ],
    { encoding: "utf8", maxBuffer: 1 << 24, timeout: 40000, killSignal: "SIGKILL" },
  );
  const match = run.stdout.match(/<pre id="probe"[^>]*>(.*?)<\/pre>/s);
  assert.ok(match, `the overlay did not report. ${run.stderr?.slice(-500) ?? ""}`);
  return JSON.parse(match[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&"));
}

const skip = chrome ? false : "headless Chromium is not installed";

test("Open chat and the pill take clicks; the bubble and its dots are only drawn", { skip, timeout: 60000 }, () => {
  const { reported, expected } = drive();
  assert.ok(reported, "the overlay reported its rects");
  const rows = [
    ["Open chat", expected.openChat, true],
    ["the quick pill", expected.pill, true],
    ["a line of speech", expected.speech, false],
    ["the thinking dots", expected.dots, false],
  ];
  for (const [name, rect, clickable] of rows) {
    assert.ok(
      reported.some((r) => JSON.stringify(r) === JSON.stringify({ rect, clickable })),
      `${name}: want ${JSON.stringify({ rect, clickable })} in ${JSON.stringify(reported)}`,
    );
  }
  assert.equal(reported.length, rows.length, "nothing else is reported");
});

test("an overlay that hands its bubbles on clears its rects, and only a change is sent", { skip, timeout: 60000 }, () => {
  const { handedOff, sent } = drive();
  assert.deepEqual(handedOff, [], "an empty list clears");
  const repeats = sent.filter((rects, i) => i > 0 && rects === sent[i - 1]);
  assert.deepEqual(repeats, [], "no report repeats the one before it");
});
