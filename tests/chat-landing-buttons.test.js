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
const SIGN_IN = {
  ...opening(harness({ name: "codex", alive: true, login: "codex login" })),
  sign_in: [
    { id: "chatgpt", label: "ChatGPT" },
    { id: "apikey", label: "API Key" },
  ],
};
// A model API source: no Harness, and the endpoint is on or off.
const MODEL_API = { ...opening(null), configured: true, model: "gpt-4o-mini", host: "https://api.example.test/v1" };
const MODEL_API_OFF = { ...MODEL_API, enabled: false };

function drive() {
  const stub = `
<script>
  const heard = {};
  const calls = [];
  let releaseSignIn = () => {};
  window.__TAURI__ = {
    core: { invoke(name, args) {
      calls.push([name, args?.harness ?? null]);
      if (name === "sign_in") return new Promise((r) => { releaseSignIn = r; });
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
    const signIn = document.getElementById("landing-sign-in");
    return {
      composer: !document.getElementById("line").disabled,
      signIn: signIn.hidden ? [] : [...signIn.querySelectorAll(".sign-in-btn")].map((b) => b.textContent),
      disabled: [...document.querySelectorAll(".connect-btn, #landing-retry")].filter((b) => b.disabled).length,
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
    show(${JSON.stringify(FAILED_PICKS.died)});
    await settle();
    out.firstOfTwo = seen();
    show(${JSON.stringify(FAILED_PICKS.missingLauncher)});
    await settle();
    out.secondOfTwo = seen();

    show(${JSON.stringify(SIGN_IN)});
    await settle();
    out.signIn = seen();
    calls.length = 0;
    document.querySelector("#landing-sign-in .sign-in-btn").click();
    await settle();
    out.signingIn = seen();
    document.querySelector('.connect-btn[data-harness="claude"]').click();
    document.getElementById("landing-retry").click();
    await settle();
    out.signingInCalls = calls.slice();
    releaseSignIn();
    await settle();
    out.signedIn = seen();

    for (const [state, payload] of [["modelApi", ${JSON.stringify(MODEL_API)}], ["modelApiOff", ${JSON.stringify(MODEL_API_OFF)}]]) {
      show(${JSON.stringify(opening(null))});
      await settle();
      document.querySelector('.connect-btn[data-harness="claude"]').click();
      await settle();
      show(${JSON.stringify(FAILED_PICKS.signedOut)});
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
  for (const state of ["died", "refusedByPreflight", "missingLauncher", "notRunning"]) {
    assert.equal(seen[state].retry, false, `${state} has nothing to retry`);
  }
});

test("a failed pick followed by another failed pick keeps all the buttons", { skip, timeout: 60000 }, () => {
  const seen = drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual([seen.firstOfTwo.title, seen.firstOfTwo.buttons], ["Claude Code couldn't start", BRANDED]);
  assert.deepEqual([seen.secondOfTwo.title, seen.secondOfTwo.buttons], ["Claude Code needs npx", BRANDED]);
});

test("the sign-in landing draws its sign-in buttons beside all the branded buttons", { skip, timeout: 60000 }, () => {
  const seen = drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(
    { title: seen.signIn.title, buttons: seen.signIn.buttons, signIn: seen.signIn.signIn, retry: seen.signIn.retry },
    { title: "Codex needs login", buttons: BRANDED, signIn: ["ChatGPT", "API Key"], retry: true },
  );
});

test("a sign-in in flight disables the branded buttons and Retry, and a press does nothing", { skip, timeout: 60000 }, () => {
  const seen = drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.equal(seen.signingIn.disabled, 11, "ten branded buttons and Retry");
  assert.deepEqual(seen.signingInCalls, [["sign_in", null]], "no pick or retry reached the Shell");
  assert.equal(seen.signedIn.disabled, 0, "they come back when the sign-in ends");
});

test("a model API that connects clears the buttons, and a switched-off one does too", { skip, timeout: 60000 }, () => {
  const seen = drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(
    { landing: seen.modelApi.landing, buttons: seen.modelApi.buttons, composer: seen.modelApi.composer },
    { landing: false, buttons: [], composer: true },
  );
  assert.deepEqual(
    { landing: seen.modelApiOff.landing, buttons: seen.modelApiOff.buttons, composer: seen.modelApiOff.composer },
    { landing: false, buttons: [], composer: false },
  );
});

test("a button on the failed landing picks that Harness, and the buttons go once one answers", { skip, timeout: 60000 }, () => {
  const seen = drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(seen.pickedAgain, [["select_harness", "codex"]]);
  assert.equal(seen.answering.landing, false);
  assert.deepEqual(seen.answering.buttons, []);
});
