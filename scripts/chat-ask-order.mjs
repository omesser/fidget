#!/usr/bin/env node
// Replays a permission-ask turn on the real Chat surface in headless Chromium
// (`window.__TAURI__` stubbed). Exit 1: answer above its ask, or the command's
// <code> lacks kind/paths or paints unlike reply code. FIDGET_CHROME picks Chromium.

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { probePage } from "../tests/chrome-cdp.js";
import { pageWait } from "../tests/page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
// findChrome() only looks on PATH, so keep the Playwright shell as this
// script's own fallback instead of reporting Chromium missing.
process.env.FIDGET_CHROME ??= join(
  homedir(),
  "Library/Caches/ms-playwright/chromium_headless_shell-1228/chrome-headless-shell-mac-arm64/chrome-headless-shell",
);

const OPENING = {
  name: "BMO",
  character: "Buddy Bot",
  configured: true,
  enabled: true,
  harness: { alive: true, missing: false, initializing: false },
  model: "",
  host: "",
  chat_ui: "minimal",
  login: null,
  harness_name: "cursor-agent",
  instructions: "",
  personality: "",
  instance_prompt: "",
  prompt_limit: 2000,
};

// The shape `cursor-agent` sends: the command is the call's title, written as
// one code span, and the shell's own note is the content.
const ASK = {
  request: "5",
  title: "`open -a Calculator`",
  kind: "execute",
  content: ["Shell allowlist is empty"],
  input: null,
  locations: ["/Applications/Calculator.app"],
  options: [
    { id: "allow-once", name: "Yes", kind: "allow_once" },
    { id: "allow-with-updates", name: "Yes, and don't ask again", kind: "allow_always" },
    { id: "reject-once", name: "No", kind: "reject_once" },
  ],
};

const ANSWER = "Hmm. BMO still sees only shapes, no names. Try `open -a Calculator` yourself.";

const stub = `
<script>
  ${pageWait()}
  const handlers = {};
  const calls = [];
  window.__TAURI__ = {
    core: {
      invoke(name, args) {
        calls.push(name);
        if (name === "chat_opening") return Promise.resolve(${JSON.stringify(OPENING)});
        return Promise.resolve();
      },
    },
    event: {
      listen(name, handler) {
        handlers[name] = handler;
        listeningNow(name);
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "chat-test" }) },
  };
  async function drive() {
    console.log("__PROBE_READY__");
    await ready("chat-permission-settled");
    await until(() => !document.getElementById("line").disabled);
    document.getElementById("line").value = "ok, tell me what you see now";
    document.getElementById("composer").requestSubmit();
    await until(() => calls.includes("chat_send"));
    handlers["chat-permission"]({ payload: ${JSON.stringify(ASK)} });
    await until(() => document.querySelector('[data-option="allow-with-updates"]'));
    document.querySelector('[data-option="allow-with-updates"]').click();
    await until(() => calls.includes("permission_answer"));
    handlers["chat-permission-settled"]({ payload: { request: "5", option: "allow-with-updates" } });
    handlers["chat"]({ payload: { said: ${JSON.stringify(ANSWER)}, busy: false, reacting_to: null, you: false, at: null, error: null, superseded_by: null } });
    await until(() => [...document.querySelectorAll("#log .row")].some((row) =>
      row.querySelector(".said")?.textContent.trim().startsWith(${JSON.stringify(ANSWER.slice(0, 20))})));
    const rows = [...document.querySelectorAll("#log .row")].map((row) => ({
      who: row.querySelector(".who-label")?.textContent ?? "",
      said: row.querySelector(".said")?.textContent.trim().slice(0, 60) ?? "",
      ask: row.classList.contains("ask"),
      code: row.querySelector(".ask-title code")?.textContent ?? "",
      paint: ((code) => {
        if (!code) return "";
        const s = getComputedStyle(code);
        return [s.backgroundColor, s.borderTopWidth, s.borderTopColor, s.fontFamily].join(" | ");
      })(row.querySelector(".said code")),
      metadata: row.querySelector(".ask-metadata")?.textContent ?? "",
    }));
    console.log("__PROBE_RESULT__" + JSON.stringify(rows));
  }
  window.addEventListener("load", drive);
</script>
`;

const html = readFileSync(join(SRC, "chat.html"), "utf8")
  .replace("<head>", `<head><base href="${pathToFileURL(SRC).href}">${stub}`);

let rows;
try {
  rows = await probePage(html);
} catch (why) {
  console.error("the harness never reported:", why.message);
  process.exit(2);
}
for (const [i, row] of rows.entries()) {
  console.log(`${i + 1}. ${row.ask ? "[ask] " : ""}${row.who}: ${row.said}`);
}
const askAt = rows.findIndex((row) => row.ask);
const answerAt = rows.findIndex((row) => row.said.startsWith(ANSWER.slice(0, 20)));
if (askAt === -1 || answerAt === -1) {
  console.error("expected both an ask row and the answer row");
  process.exit(2);
}
if (rows[askAt].code !== "open -a Calculator" || rows[askAt].metadata !== "execute · /Applications/Calculator.app") {
  console.error("FAIL: the command is not code with separate metadata");
  process.exit(1);
}
if (rows[askAt].paint !== rows[answerAt].paint) {
  console.error(`FAIL: the ask's command paints unlike reply code\n  ask:   ${rows[askAt].paint}\n  reply: ${rows[answerAt].paint}`);
  process.exit(1);
}
if (answerAt < askAt) {
  console.error(`FAIL: the answer (row ${answerAt + 1}) sits above the ask it followed (row ${askAt + 1})`);
  process.exit(1);
}
console.log("ok: the command is code painted like a reply's, and the answer landed below the ask");
