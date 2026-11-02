// After the first pick fails, the landing keeps every branded button so the
// user can quick-pick another Harness. The page is the real Chat surface.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

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

function drive({ focusDuringSignIn = false } = {}) {
  const stub = `
<script>
  ${pageWait()}
  const heard = {};
  const calls = [];
  let releaseSignIn = () => {};
  let currentOpening = ${JSON.stringify(opening(null))};
  let holdOpening = false;
  let releaseOpening = () => {};
  window.__TAURI__ = {
    core: { invoke(name, args) {
      calls.push([name, args?.harness ?? null]);
      if (name === "sign_in") return new Promise((r) => { releaseSignIn = r; });
      if (name === "chat_opening" && holdOpening) {
        return new Promise((resolve) => { releaseOpening = () => resolve(currentOpening); });
      }
      return name === "chat_opening" ? Promise.resolve(currentOpening) : Promise.resolve();
    } },
    event: { listen(name, handler) { heard[name] = handler; listeningNow(name); return Promise.resolve(() => {}); } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "chat-test" }) },
  };
  const show = (payload) => { currentOpening = payload; heard["chat-opening"]({ payload }); };
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
      signInDisabled: signIn.querySelector(".sign-in-btn")?.disabled ?? false,
      waiting: signIn.querySelector(".sign-in-waiting")?.textContent ?? "",
      disabled: [...document.querySelectorAll(".connect-btn, #landing-retry")].filter((b) => b.disabled).length,
      landing: !document.getElementById("empty").hidden && !document.getElementById("landing").hidden,
      title: document.getElementById("landing-title").textContent,
      buttons: buttons.map((b) => b.dataset.harness),
      retry: !retry.hidden,
    };
  }
  async function run() {
    console.log("__PROBE_READY__");
    await ready("chat-opening");
    await ready("tauri://focus");
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
    let focusRefresh;
    if (${focusDuringSignIn}) {
      holdOpening = true;
      focusRefresh = heard["tauri://focus"]({ payload: null });
    }
    document.querySelector("#landing-sign-in .sign-in-btn").click();
    await settle();
    out.signingIn = seen();
    if (${focusDuringSignIn}) {
      releaseOpening();
      holdOpening = false;
      await focusRefresh;
      await heard["tauri://focus"]({ payload: null });
      out.refocusedSigningIn = seen();
      document.querySelector("#landing-sign-in .sign-in-btn").click();
      document.querySelectorAll("#landing-sign-in .sign-in-btn")[1].click();
      out.refocusedSignInCalls = calls.filter(([name]) => name === "sign_in");
    }
    document.querySelector('.connect-btn[data-harness="claude"]').click();
    document.getElementById("landing-retry").click();
    await settle();
    out.signingInCalls = calls.slice();
    releaseSignIn();
    await settle();
    out.signedIn = seen();
    if (${focusDuringSignIn}) {
      currentOpening = ${JSON.stringify(ANSWERING)};
      await heard["tauri://focus"]({ payload: null });
      out.refocusedSignedIn = seen();
    }

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
    console.log("__PROBE_RESULT__" + JSON.stringify(out));
  }
  window.addEventListener("load", () => run().catch((why) => {
    console.log("__PROBE_RESULT__" + JSON.stringify({ error: String(why) }));
  }));
</script>`;
  const html = readFileSync(join(SRC, "chat.html"), "utf8").replace(
    "<head>",
    `<head>${stub}`,
  );
  return probePage(html, { args: ["--window-size=420,900"] });
}

test("every branded button stays after a failed first pick", { skip: chromeSkip, timeout: 60000 }, async () => {
  const seen = await drive();
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

test("a failed pick followed by another failed pick keeps all the buttons", { skip: chromeSkip, timeout: 60000 }, async () => {
  const seen = await drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual([seen.firstOfTwo.title, seen.firstOfTwo.buttons], ["Claude Code couldn't start", BRANDED]);
  assert.deepEqual([seen.secondOfTwo.title, seen.secondOfTwo.buttons], ["Claude Code needs npx", BRANDED]);
});

test("the sign-in landing draws its sign-in buttons beside all the branded buttons", { skip: chromeSkip, timeout: 60000 }, async () => {
  const seen = await drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(
    { title: seen.signIn.title, buttons: seen.signIn.buttons, signIn: seen.signIn.signIn, retry: seen.signIn.retry },
    { title: "Codex needs login", buttons: BRANDED, signIn: ["ChatGPT", "API Key"], retry: true },
  );
});

test("a sign-in in flight disables the branded buttons and Retry, and a press does nothing", { skip: chromeSkip, timeout: 60000 }, async () => {
  const seen = await drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.equal(seen.signingIn.disabled, 11, "ten branded buttons and Retry");
  assert.deepEqual(seen.signingInCalls, [["sign_in", null]], "no pick or retry reached the Shell");
  assert.equal(seen.signedIn.disabled, 0, "they come back when the sign-in ends");
});

test("refocusing during sign-in preserves browser progress until authentication finishes", { skip: chromeSkip, timeout: 60000 }, async () => {
  const seen = await drive({ focusDuringSignIn: true });
  assert.equal(seen.error, undefined, seen.error);
  assert.equal(seen.refocusedSigningIn.signInDisabled, true);
  assert.match(seen.refocusedSigningIn.waiting, /Finish signing in in your browser/);
  assert.equal(seen.refocusedSigningIn.disabled, 11);
  assert.deepEqual(seen.refocusedSignInCalls, [["sign_in", null]]);
  assert.equal(seen.refocusedSignedIn.composer, true);
  assert.equal(seen.refocusedSignedIn.landing, false);
});

test("a model API that connects clears the buttons, and a switched-off one does too", { skip: chromeSkip, timeout: 60000 }, async () => {
  const seen = await drive();
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

test("a button on the failed landing picks that Harness, and the buttons go once one answers", { skip: chromeSkip, timeout: 60000 }, async () => {
  const seen = await drive();
  assert.equal(seen.error, undefined, seen.error);
  assert.deepEqual(seen.pickedAgain, [["select_harness", "codex"]]);
  assert.equal(seen.answering.landing, false);
  assert.deepEqual(seen.answering.buttons, []);
});
