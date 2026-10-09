// Chat's "More options in Settings" button sends the user to the AI source
// picker (#1457). This drives the real Settings page headless with the
// snapshot the Shell sends for that aim, then drives the real Chat landing and
// reads which command its button invokes.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
const FIXTURES = new URL("./fixtures/", import.meta.url);
const read = (name) => JSON.parse(readFileSync(new URL(name, FIXTURES), "utf8"));

function chromeBin() {
  if (process.env.FIDGET_CHROME) return process.env.FIDGET_CHROME;
  for (const name of ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]) {
    const found = spawnSync("bash", ["-lc", `command -v ${name}`], { encoding: "utf8" });
    const path = found.stdout.trim();
    if (found.status === 0 && path) return path;
  }
  return null;
}

const chrome = chromeBin();
const skip = chrome ? false : "headless Chromium is not installed";

// What `Reveal::AiSource.target()` serializes to. form.rs pins the same pair.
const AI_SOURCE = { tab: "AI", row: "harness", focus: "control" };
const PRIVACY_ROW = { tab: "Privacy", row: "consent_accessibility", focus: "row" };

function render(pageName, stub, { shot = null, size = "760,640" } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "settings-reveal-"));
  const page = join(dir, "page.html");
  const html = readFileSync(join(SRC, pageName), "utf8").replace(
    "<head>",
    `<head><base href="${pathToFileURL(SRC).href}">${stub}`,
  );
  writeFileSync(page, html);
  const args = [
    "--headless",
    "--disable-gpu",
    "--no-sandbox",
    "--disable-dev-shm-usage",
    `--user-data-dir=${join(dir, "profile")}`,
    "--allow-file-access-from-files",
    "--virtual-time-budget=4000",
    `--window-size=${size}`,
  ];
  args.push(shot ? `--screenshot=${shot}` : "--dump-dom");
  args.push(pathToFileURL(page).href);
  const run = spawnSync(chrome, args, { encoding: "utf8", maxBuffer: 1 << 24, timeout: 40000, killSignal: "SIGKILL" });
  if (shot) return null;
  const match = run.stdout.match(/<pre id="probe"[^>]*>(.*?)<\/pre>/s);
  assert.ok(match, `the page did not report. ${run.stderr?.slice(-500) ?? ""}`);
  return JSON.parse(match[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&#39;/g, "'"));
}

const REPORT = `
  const report = (value) => {
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify(value);
    document.body.append(out);
  };`;

export function settingsWith({ first, later = null, shot = null, size = "760,640", freeze = null, values: over = {} }) {
  const form = read("settings-snapshot-harnessDriving.json");
  if (freeze) {
    for (const tab of form.tabs) for (const section of tab.sections) for (const row of section.rows) {
      if (row.id === freeze) row.frozen = true;
    }
  }
  const values = read("settings-values-harnessDriving.json");
  const stub = `
<script>
  ${REPORT}
  const heard = {};
  let snapshot = ${JSON.stringify({ form, view: { ...values, ...over }, reveal: first })};
  window.__TAURI__ = {
    core: { invoke(name) { return name === "settings_snapshot" ? Promise.resolve(snapshot) : Promise.resolve(); } },
    event: { listen(name, handler) { heard[name] = handler; return Promise.resolve(() => {}); } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "settings-test" }) },
  };
  const tick = () => new Promise((r) => setTimeout(r, 0));
  const settle = async () => { for (let i = 0; i < 20; i++) await tick(); };
  function seen() {
    const at = document.activeElement;
    const style = at ? getComputedStyle(at) : null;
    const box = at?.getBoundingClientRect();
    return {
      tab: document.querySelector('[role="tab"][aria-selected="true"]')?.textContent ?? null,
      focus: at?.dataset?.id ?? at?.dataset?.row ?? at?.tagName ?? null,
      tag: at?.tagName ?? null,
      ring: at?.matches(":focus-visible") === true && style.outlineStyle === "solid" && parseFloat(style.outlineWidth) >= 2
        && style.outlineColor !== "rgba(0, 0, 0, 0)" && style.outlineColor !== "transparent",
      inView: Boolean(box) && box.top >= 0 && box.bottom <= window.innerHeight,
      checked: [...document.querySelectorAll('[data-row="consent_accessibility"] input')].map((i) => i.checked),
    };
  }
  async function run() {
    while (!heard["settings-refresh"]) await tick();
    await settle();
    const out = { first: seen() };
    ${later ? `snapshot = { ...snapshot, reveal: ${JSON.stringify(later)} };
    heard["settings-refresh"]();
    await settle();
    out.later = seen();` : ""}
    report(out);
  }
  window.addEventListener("load", () => run().catch((why) => report({ error: String(why) })));
</script>`;
  return render("settings.html", stub, { shot, size });
}

const without = ({ inView, checked, ...rest }) => rest;

test("Settings opened on the AI source aim shows the AI tab with the picker focused", { skip, timeout: 60000 }, () => {
  const seen = settingsWith({ first: AI_SOURCE });
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(without(seen.first), { tab: "AI", focus: "harness", tag: "SELECT", ring: true });
});

test("an open Settings page on another tab moves to the AI source picker", { skip, timeout: 60000 }, () => {
  const seen = settingsWith({ first: null, later: AI_SOURCE });
  assert.equal(seen.error, undefined, seen.error);
  assert.equal(seen.first.tab, "Presence");
  assert.deepEqual(without(seen.later), { tab: "AI", focus: "harness", tag: "SELECT", ring: true });
});

test("the Chat landing button opens Settings on the AI source, not plain Settings", { skip, timeout: 60000 }, () => {
  const opening = {
    name: "Buddy Bot", character: "Buddy Bot", configured: false, enabled: true, harness: null,
    model: "", host: "", chat_ui: "minimal", login: null, harness_name: null,
    instructions: "", personality: "", instance_prompt: "", prompt_limit: 2000,
  };
  const stub = `
<script>
  ${REPORT}
  const heard = {};
  const calls = [];
  window.__TAURI__ = {
    core: { invoke(name) {
      calls.push(name);
      return name === "chat_opening" ? Promise.resolve(${JSON.stringify(opening)}) : Promise.resolve();
    } },
    event: { listen(name, handler) { heard[name] = handler; return Promise.resolve(() => {}); } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "chat-test" }) },
  };
  const tick = () => new Promise((r) => setTimeout(r, 0));
  async function run() {
    while (!heard["chat-opening"]) await tick();
    heard["chat-opening"]({ payload: ${JSON.stringify(opening)} });
    for (let i = 0; i < 10; i++) await tick();
    calls.length = 0;
    document.getElementById("settings-btn").click();
    for (let i = 0; i < 10; i++) await tick();
    report({ calls });
  }
  window.addEventListener("load", () => run().catch((why) => report({ error: String(why) })));
</script>`;
  const seen = render("chat.html", stub, { size: "420,560" });
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(seen.calls, ["show_ai_source"]);
});

test("the Privacy row keeps the row focused and its consent box unchecked", { skip, timeout: 60000 }, () => {
  const seen = settingsWith({ first: PRIVACY_ROW, values: { consent_accessibility: false } });
  assert.equal(seen.error, undefined, seen.error);
  assert.equal(seen.first.tab, "Privacy");
  assert.deepEqual([seen.first.tag, seen.first.focus], ["DIV", "consent_accessibility"]);
  assert.deepEqual(seen.first.checked, [false], "the aim grants nothing");
});

test("a frozen picker leaves the focus on its row", { skip, timeout: 60000 }, () => {
  const seen = settingsWith({ first: AI_SOURCE, freeze: "harness" });
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual([seen.first.tab, seen.first.tag, seen.first.focus], ["AI", "DIV", "harness"]);
});

test("the focused picker is inside a short window", { skip, timeout: 60000 }, () => {
  const seen = settingsWith({ first: AI_SOURCE, size: "760,360" });
  assert.equal(seen.error, undefined, seen.error);
  assert.equal(seen.first.tag, "SELECT");
  assert.equal(seen.first.inView, true);
});
