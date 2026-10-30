// A live tool call is a row in the log, not only a phase line. Two updates
// of one id redraw that row. Opening it shows the same pieces a replay draws.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

const CHROME_OR_CI = process.env.CI ? false : chromeSkip;

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
  ${pageWait()}
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
        listeningNow(name);
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "chat-test" }) },
  };
  const emit = (name, payload) => heard[name]({ payload });
  function snap(row) {
    return {
      label: row.querySelector(".who-label").textContent,
      open: row.querySelector(".thinking-toggle")?.getAttribute("aria-expanded") ?? null,
      hidden: row.querySelector(".said")?.hidden ?? null,
      classes: [...row.classList].filter((cls) => cls !== "restored").sort(),
      pieces: [...row.querySelectorAll(".said > *")].map((piece) => ({
        className: piece.className,
        text: piece.textContent,
      })),
    };
  }
  async function run() {
    console.log("__PROBE_READY__");
    await ready("chat");
    await ready("chat-tool");
    await ready("chat-restored");
    await ready("chat-session");
    await until(() => !document.getElementById("line").disabled);
${steps}
    console.log("__PROBE_RESULT__" + JSON.stringify(report));
  }
  window.addEventListener("load", () => run().catch((why) => {
    console.log("__PROBE_RESULT__" + JSON.stringify({ error: String(why) }));
  }));
</script>`;

  const html = readFileSync(join(SRC, "chat.html"), "utf8").replace(
    "<head>",
    `<head>${stub}`,
  );
  return probePage(html, { args: ["--window-size=420,560"] });
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
    { type: "terminal", label: "Terminal term-7" },
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
    { className: "tool-piece tool-name", text: "/Users/oded/src/main.rs (+3/-1) · full diff not drawn" },
    { className: "tool-piece tool-name", text: "Terminal term-7" },
    { className: "tool-piece", text: "wrote [x](https://evil.example) and **2** hunks" },
    { className: "tool-piece", text: "spec" },
  ],
};

test(
  "two updates of one tool call redraw one folded row, and opening it shows what the call touched",
  { skip: CHROME_OR_CI, timeout: 60000 },
  async () => {
    const report = await drive(`
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
  async () => {
    const bare = {
      id: "t-bare",
      title: "",
      kind: "read",
      status: "failed",
      locations: [],
      content: [{ type: "text", text: "nope" }],
    };
    const report = await drive(`
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

test(
  "a cut-off title stays on one line and opens whole from a focusable disclosure",
  { skip: CHROME_OR_CI, timeout: 60000 },
  async () => {
    const title = "Web search: Trump Clarksville Tennessee rally October 10 2026 speech remarks";
    const call = { id: "t-search", title, kind: "search", status: "completed", locations: [], content: [] };
    const report = await drive(`
    const report = {};
    emit("chat-restored", [${JSON.stringify({ ...call, type: "tool_call" })}]);
    emit("chat-tool", ${JSON.stringify(call)});
    emit("chat-tool", { id: "t-short", title: "Web search", kind: "search", status: "completed", locations: [], content: [] });
    await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
    report.rows = [...document.querySelectorAll("#log .row.tool")].map((row) => {
      const label = row.querySelector(".who-label");
      return {
        label: label.textContent,
        hover: label.title,
        toggle: row.querySelector(".thinking-toggle")?.tagName ?? null,
        detail: row.querySelector(".said")?.textContent ?? null,
        nowrap: getComputedStyle(label).whiteSpace,
        cut: label.scrollWidth > label.clientWidth,
        oneLine: label.getClientRects().length === 1,
      };
    });`);
    assert.equal(report.error, undefined, report.error);
    const row = { label: `${title} · completed`, hover: `${title} · completed`, toggle: "BUTTON", detail: title, nowrap: "nowrap", cut: true, oneLine: true };
    const short = { label: "Web search · completed", hover: "Web search · completed", toggle: null, detail: null, nowrap: "nowrap", cut: false, oneLine: true };
    assert.deepEqual(report.rows, [row, row, short]);
  },
);

test("an empty call is a plain label in live and replay", { skip: CHROME_OR_CI, timeout: 60000 }, async () => {
  const call = { id: "t-plain", title: "", name: "web_search", kind: "search", status: "completed", locations: [], content: [] };
  const report = await drive(`
    const report = {};
    emit("chat-restored", [${JSON.stringify({ ...call, type: "tool_call" })}]);
    emit("chat-tool", ${JSON.stringify(call)});
    report.rows = [...document.querySelectorAll("#log .row.tool")].map(snap);`);
  assert.equal(report.error, undefined, report.error);
  assert.deepEqual(report.rows[0], report.rows[1]);
  assert.equal(report.rows[0].label, "web_search · completed");
  assert.equal(report.rows[0].open, null);
  assert.deepEqual(report.rows[0].pieces, []);
});

test("raw fields render as json fences beside content in live and replay", { skip: CHROME_OR_CI, timeout: 60000 }, async () => {
  const call = {
    id: "raw", title: "Read file", name: "read_file", kind: "read", status: "completed",
    rawInput: { path: "/tmp/a" }, rawOutput: { text: "done\u202e" },
    locations: [{ path: "/tmp/a", line: 3 }], content: [{ type: "text", text: "Visible result" }],
  };
  const report = await drive(`
    const report = {};
    emit("chat-restored", [${JSON.stringify({ ...call, type: "tool_call" })}]);
    emit("chat-tool", ${JSON.stringify(call)});
    report.rows = [...document.querySelectorAll("#log .row.tool")].map(snap);
    report.fences = [...document.querySelectorAll("#log .row.tool .tool-piece pre code")].map((code) => code.textContent);`);
  assert.equal(report.error, undefined, report.error);
  assert.deepEqual(report.rows[0], report.rows[1]);
  assert.deepEqual(report.rows[0].pieces.map((p) => p.className), [
    "tool-piece tool-location", "tool-piece", "tool-piece tool-input", "tool-piece tool-output",
  ]);
  const input = JSON.stringify({ path: "/tmp/a" }, null, 2);
  const output = JSON.stringify({ text: "done" }, null, 2);
  assert.deepEqual(report.fences.map((f) => f.trimEnd()), [input, output, input, output]);
});

test("a live call gains and loses a disclosure as details change", { skip: CHROME_OR_CI, timeout: 60000 }, async () => {
  const report = await drive(`
    const report = {};
    emit("chat-tool", { id: "t", title: "Read file", kind: "read", status: "pending", locations: [], content: [] });
    const row = document.querySelector("#log .row.tool");
    report.initial = snap(row);
    emit("chat-tool", { id: "t", content: [{ type: "text", text: "first" }] });
    report.withDetail = snap(row);
    row.querySelector(".thinking-toggle").click();
    emit("chat-tool", { id: "t", status: "completed" });
    report.openAfterUpdate = snap(row);
    emit("chat-tool", { id: "t", content: [] });
    report.cleared = snap(row);`);
  assert.equal(report.error, undefined, report.error);
  assert.deepEqual([report.initial.open, report.withDetail.open, report.openAfterUpdate.open, report.cleared.open], [null, "false", "true", null]);
  assert.deepEqual(report.openAfterUpdate.pieces.map((p) => p.text), ["first"]);
  assert.deepEqual(report.cleared.pieces, []);
});

test("raw updates retain earlier values when omitted or null", { skip: CHROME_OR_CI, timeout: 60000 }, async () => {
  const report = await drive(`
    const report = {};
    emit("chat-tool", { id: "t", title: "Fetch", kind: "fetch", rawInput: { url: "https://example.com" }, rawOutput: { ok: true } });
    const row = document.querySelector("#log .row.tool");
    emit("chat-tool", { id: "t", rawInput: null, rawOutput: null, status: "completed" });
    report.row = snap(row);`);
  assert.equal(report.error, undefined, report.error);
  assert.deepEqual(report.row.pieces.map((p) => p.text.trimEnd()), [
    JSON.stringify({ url: "https://example.com" }, null, 2), JSON.stringify({ ok: true }, null, 2),
  ]);
});

test("a call with blank display pieces stays a plain label", { skip: CHROME_OR_CI, timeout: 60000 }, async () => {
  const report = await drive(`
    const report = {};
    emit("chat-tool", { id: "blank", title: "Read file", kind: "read", content: [{ type: "text", text: "" }], locations: [{ path: "" }] });
    report.row = snap(document.querySelector("#log .row.tool"));`);
  assert.equal(report.error, undefined, report.error);
  assert.equal(report.row.open, null);
  assert.equal(report.row.hidden, null);
  assert.deepEqual(report.row.pieces, []);
});

test("multi-line and tabbed titles keep their text as sent in live and replay", { skip: CHROME_OR_CI, timeout: 60000 }, async () => {
  const calls = [
    { id: "multiline", title: "Read\nroster file", kind: "read", status: "completed", content: [], locations: [] },
    { id: "tabbed", title: "Read\troster file", kind: "read", status: "completed", content: [], locations: [] },
  ];
  const report = await drive(`
    const report = {};
    emit("chat-restored", ${JSON.stringify(calls.map((call) => ({ ...call, type: "tool_call" })))});
    for (const call of ${JSON.stringify(calls)}) emit("chat-tool", call);
    report.rows = [...document.querySelectorAll("#log .row.tool")].map((row) => ({
      label: row.querySelector(".who-label").textContent,
      oneLine: row.querySelector(".who-label").getClientRects().length === 1,
      detail: row.querySelector(".said")?.textContent ?? null,
    }));`);
  assert.equal(report.error, undefined, report.error);
  const multi = { label: "Read\nroster file · completed", oneLine: true, detail: "Read\nroster file" };
  const tab = { label: "Read\troster file · completed", oneLine: true, detail: null };
  assert.deepEqual(report.rows, [multi, tab, multi, tab]);
});
