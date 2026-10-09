// A model or effort the Harness did not take shows as a notice under its field,
// and the field shows the value Fidget restored (#1434). This drives the real
// Settings page headless, as chat-restored-render.test.js drives Chat: it sends
// the snapshot the Shell would send, then the refresh that follows.

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

// The sentences `ConfigFailure::notice` writes. harness.rs checks the same
// fixture, so the page draws what the Shell sends.
const NOTICES = read("settings-not-applied-notices.json");
export const MODEL_NOTICE = NOTICES.model_refused;
export const EFFORT_NOTICE = NOTICES.effort_unadvertised;

// `before` is what the page holds after Apply. The steps send the refresh that
// follows once the next wake finds the refusal.
export function drive({ before, steps, shot = null, size = "760,640" }) {
  const form = read("settings-snapshot-harnessDriving.json");
  const values = read("settings-values-harnessDriving.json");
  const stub = `
<script>
  const heard = {};
  let snapshot = ${JSON.stringify({ form, view: { ...values, ...before }, reveal: null })};
  window.__TAURI__ = {
    core: { invoke(name) { return name === "settings_snapshot" ? Promise.resolve(snapshot) : Promise.resolve(); } },
    event: { listen(name, handler) { heard[name] = handler; return Promise.resolve(() => {}); } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "settings-test" }) },
  };
  const tick = () => new Promise((r) => setTimeout(r, 0));
  const tab = (title) => [...document.querySelectorAll('[role="tab"]')].find((t) => t.textContent === title).click();
  const refresh = async (view) => {
    snapshot = { ...snapshot, view: { ...snapshot.view, ...view } };
    heard["settings-refresh"]();
    for (let i = 0; i < 20; i++) await tick();
  };
  const row = (id) => {
    const node = document.querySelector('[data-row="' + id + '"]');
    const notice = node?.querySelector(".set-notice");
    return {
      value: node?.querySelector("input")?.value ?? null,
      notice: notice?.textContent ?? null,
      alert: notice?.getAttribute("role") ?? null,
    };
  };
  async function run() {
    while (!heard["settings-refresh"]) await tick();
    for (let i = 0; i < 20; i++) await tick();
    const seen = {};
${steps}
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify(seen);
    document.body.append(out);
  }
  window.addEventListener("load", () => run().catch((why) => {
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify({ error: String(why) });
    document.body.append(out);
  }));
</script>`;
  const dir = mkdtempSync(join(tmpdir(), "settings-not-applied-"));
  const page = join(dir, "harness.html");
  const html = readFileSync(join(SRC, "settings.html"), "utf8").replace(
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
  if (shot) args.push(`--screenshot=${shot}`);
  else args.push("--dump-dom");
  args.push(pathToFileURL(page).href);
  const run = spawnSync(chrome, args, { encoding: "utf8", maxBuffer: 1 << 24, timeout: 40000, killSignal: "SIGKILL" });
  if (shot) return null;
  const match = run.stdout.match(/<pre id="probe"[^>]*>(.*?)<\/pre>/s);
  assert.ok(match, `the Settings page did not report. ${run.stderr?.slice(-500) ?? ""}`);
  return JSON.parse(match[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&#39;/g, "'"));
}

const skip = chrome ? false : "headless Chromium is not installed";

test("a refused model shows its notice and the old value is back in the field", { skip, timeout: 60000 }, () => {
  const seen = drive({
    before: { harness_model: "nope", director_model: "nope" },
    steps: `
    tab("AI");
    for (let i = 0; i < 5; i++) await tick();
    seen.applied = row("harness_model");
    await refresh({ harness_model: "gpt-4o-mini", director_model: "gpt-4o-mini",
      harness_model_notice: ${JSON.stringify(MODEL_NOTICE)}, director_model_notice: ${JSON.stringify(MODEL_NOTICE)} });
    seen.refused = row("harness_model");
    seen.twin = row("director_model");`,
  });
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(seen.applied, { value: "nope", notice: null, alert: null });
  assert.deepEqual(seen.refused, { value: "gpt-4o-mini", notice: MODEL_NOTICE, alert: "alert" });
  assert.match(seen.refused.notice, /You asked for "gpt-5"/, "the tried value is in the notice");
  // The Model / API row is off while a Harness drives, so it shows no notice.
  assert.deepEqual(seen.twin, { value: "gpt-4o-mini", notice: null, alert: null });
});

test("a Harness with no effort setting says so under the effort field", { skip, timeout: 60000 }, () => {
  const seen = drive({
    before: { director_reasoning_effort: "high" },
    steps: `
    tab("Development");
    for (let i = 0; i < 5; i++) await tick();
    seen.applied = row("director_reasoning_effort");
    await refresh({ director_reasoning_effort: "", director_reasoning_effort_notice: ${JSON.stringify(EFFORT_NOTICE)} });
    seen.refused = row("director_reasoning_effort");`,
  });
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(seen.applied, { value: "high", notice: null, alert: null });
  assert.deepEqual(seen.refused, { value: "", notice: EFFORT_NOTICE, alert: "alert" });
  assert.match(seen.refused.notice, /You asked for "high"/, "the tried value is in the notice");
});

test("a notice goes when the next snapshot carries none", { skip, timeout: 60000 }, () => {
  const seen = drive({
    before: { harness_model: "gpt-4o-mini", harness_model_notice: MODEL_NOTICE },
    steps: `
    tab("AI");
    for (let i = 0; i < 5; i++) await tick();
    seen.shown = row("harness_model");
    snapshot.view.harness_model_notice = undefined;
    delete snapshot.view.harness_model_notice;
    await refresh({});
    seen.cleared = row("harness_model");`,
  });
  assert.equal(seen.error, undefined, seen.error);
  assert.equal(seen.shown.notice, MODEL_NOTICE);
  assert.equal(seen.cleared.notice, null);
});

test("a tried value with markup is shown as text and injects nothing", { skip, timeout: 60000 }, () => {
  const hostile = 'Model did not apply. You asked for "<img src=x onerror=window.__pwn=1><b>x</b>". The Harness refused it: <script>window.__pwn=1</script>. Fidget put back the Harness default.';
  const seen = drive({
    before: { harness_model: "x" },
    steps: `
    tab("AI");
    for (let i = 0; i < 5; i++) await tick();
    await refresh({ harness_model: "", harness_model_notice: ${JSON.stringify(hostile).replace(/</g, "\\u003c")} });
    const notice = document.querySelector('[data-row="harness_model"] .set-notice');
    seen.text = notice.textContent;
    seen.elements = notice.children.length;
    seen.pwned = window.__pwn === 1;
    seen.injected = document.querySelectorAll("img[src='x'], .set-notice b, .set-notice script").length;`,
  });
  assert.equal(seen.error, undefined, seen.error);
  assert.equal(seen.text, hostile);
  assert.deepEqual([seen.elements, seen.pwned, seen.injected], [0, false, 0]);
});
