// A model or effort the Harness did not take reaches the Settings page as a
// notice under its field, and the field shows the value Fidget went back to
// (#1434). The page itself, driven headless the way chat-restored-render.test.js
// drives Chat: the snapshot the Shell would send, and the refresh that follows.

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

export const MODEL_NOTICE =
  'Model did not apply. The Harness refused it: unknown model. Fidget put back "gpt-4o-mini".';
export const EFFORT_NOTICE =
  "Reasoning effort did not apply. This Harness doesn't offer a reasoning effort setting, so Fidget can't change it. Fidget put back the Harness default.";

// `before` is what the page holds after Apply. `after` is the snapshot the
// refresh brings once the next wake found the refusal.
export function drive({ before, after, steps, shot = null, size = "760,640" }) {
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
  void after;
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
  // The Model / API row is off while a Harness drives, and says nothing of it.
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
