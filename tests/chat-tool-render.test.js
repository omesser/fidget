// A live tool call is a row in the log, not only a phase line. Two updates
// of one id redraw that row. Opening it shows the same pieces a replay draws.

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
const CHROME_OR_CI = chrome || process.env.CI ? false : "headless Chromium is not installed";

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
  function snap(row) {
    return {
      label: row.querySelector(".who-label").textContent,
      open: row.querySelector(".thinking-toggle").getAttribute("aria-expanded"),
      hidden: row.querySelector(".said").hidden,
      classes: [...row.classList].filter((cls) => cls !== "restored").sort(),
      pieces: [...row.querySelectorAll(".said > *")].map((piece) => ({
        className: piece.className,
        text: piece.textContent,
      })),
    };
  }
  async function run() {
    while (!heard.chat || !heard["chat-tool"] || !heard["chat-restored"] || !heard["chat-session"]) await tick();
    while (document.getElementById("line").disabled) await tick();
${steps}
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify(report);
    document.body.append(out);
  }
  window.addEventListener("load", () => run().catch((why) => {
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify({ error: String(why) });
    document.body.append(out);
  }));
</script>`;

  const dir = mkdtempSync(join(tmpdir(), "chat-tool-render-"));
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
  return JSON.parse(
    match[1].replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">"),
  );
}

const CALL = {
  id: "t-edit",
  title: "Edit main.rs",
  kind: "edit",
  status: "completed",
  locations: [
    { path: "/Users/oded/src/main.rs", line: 12 },
    { path: "/tmp/page.html", line: null },
  ],
  content: [
    { type: "diff", path: "/Users/oded/src/main.rs", added: 3, removed: 1, approximate: false },
    { type: "terminal", id: "term-7" },
    { type: "text", text: "wrote [x](https://evil.example) and **2** hunks" },
    { type: "mark", markdown: "[spec](https://example.com/spec.md)" },
  ],
};

const DRAWN = {
  label: "Edit main.rs · completed",
  open: "false",
  hidden: true,
  classes: ["row", "thinking", "tool"],
  pieces: [
    { className: "tool-piece tool-location", text: "/Users/oded/src/main.rs:12" },
    { className: "tool-piece tool-location", text: "/tmp/page.html" },
    { className: "tool-piece", text: "/Users/oded/src/main.rs (+3/-1) · full diff not drawn" },
    { className: "tool-piece", text: "Terminal term-7" },
    { className: "tool-piece", text: "wrote [x](https://evil.example) and **2** hunks" },
    { className: "tool-piece", text: "spec" },
  ],
};

test(
  "two updates of one tool call redraw one folded row, and opening it shows what the call touched",
  { skip: CHROME_OR_CI, timeout: 60000 },
  () => {
    const report = drive(`
    const report = {};
    emit("chat", { you: true, said: "read the roster", busy: false, reacting_to: null, thought: false, at: null, error: null, streaming: false });
    emit("chat-tool", {
      id: "t1", title: "Read file", kind: "read", status: "in_progress",
      locations: [{ path: "/tmp/roster.json", line: 4 }],
      content: [{ type: "text", text: "waiting" }],
    });
    report.phase = document.querySelector(".row.them .phase")?.textContent ?? null;
    report.first = snap(document.querySelector("#log .row.tool"));
    emit("chat-tool", {
      id: "t1", title: null, kind: null, status: "completed", locations: null,
      content: [{ type: "text", text: "names" }],
    });
    const rows = [...document.querySelectorAll("#log .row.tool")];
    report.count = rows.length;
    const row = rows[0];
    report.folded = snap(row);
    row.querySelector(".thinking-toggle").click();
    report.opened = { open: row.querySelector(".thinking-toggle").getAttribute("aria-expanded"), hidden: row.querySelector(".said").hidden };
    emit("chat-session", "the harness restarted");
    emit("chat-tool", { id: "t1", title: "Read file", kind: "read", status: "pending", locations: [], content: [] });
    report.afterSession = snap(document.querySelector("#log .row.tool"));
    report.afterCount = document.querySelectorAll("#log .row.tool").length;`);
    assert.equal(report.error, undefined, report.error);
    assert.equal(report.phase, "Running: Read file");
    assert.equal(report.count, 1);
    assert.equal(report.first.label, "Read file · in_progress");
    assert.equal(report.first.open, "false");
    assert.equal(report.first.hidden, true);
    assert.deepEqual(report.folded.pieces, [
      { className: "tool-piece tool-location", text: "/tmp/roster.json:4" },
      { className: "tool-piece", text: "names" },
    ]);
    assert.equal(report.folded.label, "Read file · completed");
    assert.equal(report.folded.open, "false");
    assert.equal(report.folded.hidden, true);
    assert.deepEqual(report.opened, { open: "true", hidden: false });
    assert.equal(report.afterCount, 1);
    assert.equal(report.afterSession.label, "Read file · pending");
    assert.deepEqual(report.afterSession.pieces, []);
  },
);

test(
  "a live tool call draws the same row as its replay",
  { skip: CHROME_OR_CI, timeout: 60000 },
  () => {
    const bare = {
      id: "t-bare",
      title: "",
      kind: "read",
      status: "failed",
      locations: [],
      content: [{ type: "text", text: "nope" }],
    };
    const report = drive(`
    const report = {};
    emit("chat-restored", [${JSON.stringify({ ...CALL, type: "tool_call" })}, ${JSON.stringify({ ...bare, type: "tool_call" })}]);
    emit("chat-tool", ${JSON.stringify(CALL)});
    emit("chat-tool", ${JSON.stringify(bare)});
    const rows = [...document.querySelectorAll("#log .row.tool")].map(snap);
    report.rows = rows;`);
    assert.equal(report.error, undefined, report.error);
    assert.deepEqual(report.rows[0], DRAWN);
    assert.deepEqual(report.rows[2], DRAWN);
    assert.deepEqual(report.rows[1], {
      label: "read · failed",
      open: "false",
      hidden: true,
      classes: ["row", "thinking", "tool"],
      pieces: [{ className: "tool-piece", text: "nope" }],
    });
    assert.deepEqual(report.rows[1], report.rows[3]);
  },
);
