// The Chat surface itself, driven headless the way chat-thinking-render.test.js
// drives it: a line typed after a restart, the loaded session's whole replay
// arriving while it waits, and the answer landing on the line (#1393). And a
// window that opens as the load lands, which hears the history twice.

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

const OPENING = {
  name: "Buddy Bot",
  character: "Buddy Bot",
  configured: true,
  enabled: true,
  harness: { name: "claude", session: "fd4be1a2", alive: true, missing: false, initializing: false, login: null },
  model: "",
  host: "",
  chat_ui: "minimal",
  login: null,
  harness_name: "claude",
  instructions: "",
  personality: "",
  instance_prompt: "",
  prompt_limit: 2000,
};

function drive(steps) {
  const stub = `
<script>
  const heard = {};
  window.__TAURI__ = {
    core: {
      invoke(name) {
        if (name === "chat_opening") return Promise.resolve(${JSON.stringify(OPENING)});
        return Promise.resolve();
      },
    },
    event: {
      listen(name, handler) {
        heard[name] = handler;
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "chat-test" }) },
  };
  const tick = () => new Promise((r) => setTimeout(r, 0));
  const emit = (name, payload) => heard[name]({ payload });
  const KINDS = ["prompt", "thought", "tool", "plan-steps", "restored", "you", "them"];
  function rows() {
    return [...document.querySelectorAll("#log > .row, #log > .note")].map((row) => ({
      kind: KINDS.find((kind) => row.classList.contains(kind)) ?? "note",
      label: row.querySelector(".who-label")?.textContent ?? null,
      open: row.querySelector(".thinking-toggle")?.getAttribute("aria-expanded") ?? null,
      stamped: Boolean(row.querySelector("time.when")?.dateTime),
      text: (row.querySelector(".said") ?? row).textContent,
    }));
  }
  async function run() {
    while (!heard["chat-opening"] || !heard["chat-restored"]) await tick();
    while (document.getElementById("line").disabled) await tick();
${steps}
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify({ rows: rows(), livePlan: !document.getElementById("plan").hidden });
    document.body.append(out);
  }
  window.addEventListener("load", () => run().catch((why) => {
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify({ error: String(why) });
    document.body.append(out);
  }));
</script>`;

  const dir = mkdtempSync(join(tmpdir(), "chat-restored-render-"));
  const page = join(dir, "harness.html");
  const html = readFileSync(join(SRC, "chat.html"), "utf8").replace(
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
      "--virtual-time-budget=3000",
      "--window-size=420,560",
      "--dump-dom",
      pathToFileURL(page).href,
    ],
    { encoding: "utf8", maxBuffer: 1 << 24, timeout: 40000, killSignal: "SIGKILL" },
  );
  const match = run.stdout.match(/<pre id="probe"[^>]*>(.*?)<\/pre>/s);
  assert.ok(match, `the Chat surface did not report. ${run.stderr?.slice(-500) ?? ""}`);
  return JSON.parse(match[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">"));
}

const HISTORY = [
  { type: "prompt", text: "what just happened: they typed\nthey said: hi" },
  { type: "thought", text: "Weighing it" },
  { type: "reply", text: "wave\nHello from **before**" },
  { type: "tool_call", id: "t1", title: "Read roster.json", kind: "read", status: "failed", content: ["no such file"] },
  { type: "plan", steps: [{ content: "Read the roster", priority: "medium", status: "completed" }] },
  { type: "reply", text: "Done." },
];

const DRAWN = [
  { kind: "note", label: null, open: null, stamped: false, text: "Earlier in this session." },
  { kind: "prompt", label: "Prompt", open: "false", stamped: false, text: "what just happened: they typed\nthey said: hi" },
  { kind: "thought", label: "Thinking", open: "false", stamped: false, text: "Weighing it" },
  { kind: "restored", label: "Buddy Bot", open: null, stamped: false, text: "wave\nHello from before" },
  { kind: "tool", label: "Read roster.json · failed", open: "false", stamped: false, text: "no such file" },
  { kind: "plan-steps", label: "Plan", open: null, stamped: false, text: "Read the roster" },
  { kind: "restored", label: "Buddy Bot", open: null, stamped: false, text: "Done." },
];

test(
  "a loaded session's whole replay sits above the waiting line, and the answer still lands on it",
  { skip: chrome ? false : "headless Chromium is not installed", timeout: 60000 },
  () => {
    const report = drive(`
    document.getElementById("line").value = "are you back?";
    document.getElementById("composer").requestSubmit();
    while (!document.querySelector("#log > .row.them")) await tick();
    emit("chat-restored", ${JSON.stringify(HISTORY)});
    emit("chat", { said: "Back now.", busy: false, reacting_to: null, you: false, at: null, error: null });`);
    assert.equal(report.error, undefined, report.error);
    assert.deepEqual(report.rows, [
      ...DRAWN,
      { kind: "you", label: "You", open: null, stamped: true, text: "are you back?" },
      { kind: "them", label: "Buddy Bot", open: null, stamped: true, text: "Back now." },
    ]);
    assert.equal(report.livePlan, false, "a replayed plan is history, not the plan on the wire");
  },
);

test(
  "a replayed thought opens on a click, like a landed live one",
  { skip: chrome ? false : "headless Chromium is not installed", timeout: 60000 },
  () => {
    const report = drive(`
    emit("chat-restored", ${JSON.stringify(HISTORY)});
    document.querySelector("#log > .row.thought .thinking-toggle").click();`);
    assert.equal(report.error, undefined, report.error);
    assert.deepEqual(report.rows[2], {
      kind: "thought",
      label: "Thinking",
      open: "true",
      stamped: false,
      text: "Weighing it",
    });
  },
);

// The load lands after this window listens but before `chat_ready` reads the
// log, so the Shell sends the history live and again on the replay.
test(
  "a window that hears the history twice draws it once",
  { skip: chrome ? false : "headless Chromium is not installed", timeout: 60000 },
  () => {
    const report = drive(`
    emit("chat-restored", ${JSON.stringify(HISTORY)});
    emit("chat-restored", ${JSON.stringify(HISTORY)});`);
    assert.equal(report.error, undefined, report.error);
    assert.deepEqual(report.rows, DRAWN);
  },
);
