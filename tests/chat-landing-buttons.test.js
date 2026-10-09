// After the first pick fails, the landing keeps every branded button so the
// user can quick-pick another Harness (#1458). This drives the real Chat page
// headless: it presses a branded button, then sends the opening the Shell
// pushes after each kind of failed pick, and reads what the page draws.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

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

const BRANDED = ["claude", "codex", "copilot", "cursor-agent", "goose", "grok", "hermes", "opencode", "pi", "antigravity"];

const opening = (harness) => ({
  name: "Buddy Bot",
  character: "Buddy Bot",
  configured: harness !== null,
  enabled: true,
  harness,
  model: "",
  host: "",
  chat_ui: "minimal",
  login: harness?.login ?? null,
  harness_name: harness?.name ?? null,
  instructions: "",
  personality: "",
  instance_prompt: "",
  prompt_limit: 2000,
});

const harness = (extra) => ({
  name: "claude", session: null, alive: false, missing: null, initializing: false, failed: null, login: null, ...extra,
});

const FAILED_PICKS = {
  signedOut: opening(harness({ alive: true, login: "claude /login" })),
  died: opening(harness({ failed: { command: "npx -y adapter", reason: "exited with code 1", output: "boom", node_check: null } })),
  refusedByPreflight: opening(harness({ unhealthy: { command: "npx --version", reason: "timed out", output: null, node_check: null } })),
  missingLauncher: opening(harness({ missing: "npx", install: "https://nodejs.org/" })),
  notRunning: opening(harness({})),
};
const ANSWERING = opening(harness({ alive: true, session: "fd4be1a2" }));

function drive() {
  const stub = `
<script>
  const heard = {};
  const calls = [];
  window.__TAURI__ = {
    core: { invoke(name, args) {
      calls.push([name, args?.harness ?? null]);
      return name === "chat_opening" ? Promise.resolve(${JSON.stringify(opening(null))}) : Promise.resolve();
    } },
    event: { listen(name, handler) { heard[name] = handler; return Promise.resolve(() => {}); } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "chat-test" }) },
  };
  const tick = () => new Promise((r) => setTimeout(r, 0));
  const settle = async () => { for (let i = 0; i < 10; i++) await tick(); };
  const show = (payload) => heard["chat-opening"]({ payload });
  function seen() {
    const buttons = [...document.querySelectorAll(".connect-btn")].filter((b) => {
      const box = b.getBoundingClientRect();
      return box.width > 0 && box.height > 0 && getComputedStyle(b).visibility === "visible";
    });
    const retry = document.getElementById("landing-retry");
    return {
      landing: !document.getElementById("empty").hidden && !document.getElementById("landing").hidden,
      title: document.getElementById("landing-title").textContent,
      buttons: buttons.map((b) => b.dataset.harness),
      retry: !retry.hidden,
    };
  }
  async function run() {
    while (!heard["chat-opening"]) await tick();
    const out = {};
    for (const [state, payload] of Object.entries(${JSON.stringify(FAILED_PICKS)})) {
      show(${JSON.stringify(opening(null))});
      await settle();
      document.querySelector('.connect-btn[data-harness="claude"]').click();
      await settle();
      show(payload);
      await settle();
      out[state] = seen();
    }
    show(${JSON.stringify(FAILED_PICKS.signedOut)});
    await settle();
    calls.length = 0;
    document.querySelector('.connect-btn[data-harness="codex"]').click();
    await settle();
    out.pickedAgain = calls;
    show(${JSON.stringify(ANSWERING)});
    await settle();
    out.answering = seen();
    const pre = document.createElement("pre");
    pre.id = "probe";
    pre.textContent = JSON.stringify(out);
    document.body.append(pre);
  }
  window.addEventListener("load", () => run().catch((why) => {
    const pre = document.createElement("pre");
    pre.id = "probe";
    pre.textContent = JSON.stringify({ error: String(why) });
    document.body.append(pre);
  }));
</script>`;
  const dir = mkdtempSync(join(tmpdir(), "chat-landing-buttons-"));
  const page = join(dir, "chat.html");
  writeFileSync(
    page,
    readFileSync(join(SRC, "chat.html"), "utf8").replace("<head>", `<head><base href="${pathToFileURL(SRC).href}">${stub}`),
  );
  const run = spawnSync(
    chrome,
    [
      "--headless", "--disable-gpu", "--no-sandbox", "--disable-dev-shm-usage",
      `--user-data-dir=${join(dir, "profile")}`,
      "--allow-file-access-from-files",
      "--virtual-time-budget=10000",
      "--window-size=420,900",
      "--dump-dom",
      pathToFileURL(page).href,
    ],
    { encoding: "utf8", maxBuffer: 1 << 24, timeout: 40000, killSignal: "SIGKILL" },
  );
  const match = run.stdout.match(/<pre id="probe"[^>]*>(.*?)<\/pre>/s);
  assert.ok(match, `the Chat surface did not report. ${run.stderr?.slice(-500) ?? ""}`);
  return JSON.parse(match[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">"));
}

test("every branded button stays after a failed first pick", { skip, timeout: 60000 }, () => {
  const seen = drive();
  assert.equal(seen.error, undefined, seen.error);

  const titles = {
    signedOut: "Claude Code needs login",
    died: "Claude Code couldn't start",
    refusedByPreflight: "Claude Code couldn't start",
    missingLauncher: "Claude Code needs npx",
    notRunning: "Claude Code is not running",
  };
  for (const [state, title] of Object.entries(titles)) {
    assert.deepEqual(
      { landing: seen[state].landing, title: seen[state].title, buttons: seen[state].buttons },
      { landing: true, title, buttons: BRANDED },
      state,
    );
  }
  assert.equal(seen.signedOut.retry, true, "the signed-out landing keeps Retry beside the buttons");
});

test("a button on the failed landing picks that Harness, and the buttons go once one answers", { skip, timeout: 60000 }, () => {
  const seen = drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(seen.pickedAgain, [["select_harness", "codex"]]);
  assert.equal(seen.answering.landing, false);
  assert.deepEqual(seen.answering.buttons, []);
});
