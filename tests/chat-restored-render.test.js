// A line typed after a restart can meet the loaded session's whole replay
// while it waits, and the answer has to land on that line. A window that
// opens as the load lands hears the history twice.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

// The renderer job in CI has Chromium, so a missing browser there is a broken
// gate, not a reason to skip: the test runs, and fails on the missing binary.
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
  window.__invoked = [];
  window.__TAURI__ = {
    core: {
      invoke(name, args) {
        window.__invoked.push({ name, args });
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
    console.log("__PROBE_READY__");
    await ready("chat-opening");
    await ready("chat-restored");
    await until(() => !document.getElementById("line").disabled);
${steps}
    console.log("__PROBE_RESULT__" + JSON.stringify({
      rows: rows(),
      livePlan: !document.getElementById("plan").hidden,
      links: [...document.querySelectorAll("#log .md-link")].map((link) => link.dataset.href),
      tools: [...document.querySelectorAll("#log .row.tool")].map((row) => ({
        label: row.querySelector(".who-label").textContent,
        pieces: [...row.querySelectorAll(".said > *")].map((piece) => piece.textContent),
        spaces: [...row.querySelectorAll(".tool-location, .tool-name")].map((piece) => getComputedStyle(piece).whiteSpace),
      })),
      anchors: document.querySelectorAll("#log a").length,
      opened: window.__invoked.filter((call) => call.name === "open_link").map((call) => call.args.url),
    }));
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

const HISTORY = [
  { type: "prompt", text: "what just happened: they typed\nthey said: hi" },
  { type: "thought", text: "Weighing it" },
  { type: "reply", text: "wave\nHello from **before**" },
  { type: "tool_call", id: "t1", title: "Read roster.json", kind: "read", status: "failed", locations: [], content: [{ type: "text", text: "no such file" }] },
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
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const report = await drive(`
    document.getElementById("line").value = "are you back?";
    document.getElementById("composer").requestSubmit();
    await until(() => document.querySelector("#log > .row.them"));
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
  "a typed line in a replayed prompt is the user's row above the folded frame",
  { skip: CHROME_OR_CI, timeout: 60000 },
  async () => {
    const history = [
      { type: "typed", text: "are you <b>there</b>?" },
      { type: "prompt", text: "what just happened: spoken to\nthey said: are you <b>there</b>?" },
      { type: "reply", text: "Still here" },
    ];
    const report = await drive(`emit("chat-restored", ${JSON.stringify(history)});`);
    assert.equal(report.error, undefined, report.error);
    assert.deepEqual(report.rows, [
      { kind: "note", label: null, open: null, stamped: false, text: "Earlier in this session." },
      { kind: "restored", label: "You", open: null, stamped: false, text: "are you <b>there</b>?" },
      { kind: "prompt", label: "Prompt", open: "false", stamped: false, text: "what just happened: spoken to\nthey said: are you <b>there</b>?" },
      { kind: "restored", label: "Buddy Bot", open: null, stamped: false, text: "Still here" },
    ]);
  },
);

test(
  "a replayed thought opens on a click, like a landed live one",
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const report = await drive(`
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
  { skip: chromeSkip, timeout: 60000 },
  async () => {
    const report = await drive(`
    emit("chat-restored", ${JSON.stringify(HISTORY)});
    emit("chat-restored", ${JSON.stringify(HISTORY)});`);
    assert.equal(report.error, undefined, report.error);
    assert.deepEqual(report.rows, DRAWN);
  },
);

// ADR-0028: a resource link is a link. Clicked in a reply, a replayed thought
// or a live one, it goes to the one opener the reply links use, and a link
// Chat will not open is drawn as its words and its uri.
test(
  "a resource link opens through open_link wherever it lands, and an unsafe one is text",
  { skip: CHROME_OR_CI, timeout: 60000 },
  async () => {
    const history = [
      { type: "thought", text: "Saw [spec](https://example.com/spec.md)\n[link passwd file:///etc/passwd]" },
      { type: "reply", text: "[brief](https://example.com/brief)" },
    ];
    const report = await drive(`
    document.getElementById("line").value = "show me";
    document.getElementById("composer").requestSubmit();
    await until(() => document.querySelector("#log > .row.them"));
    emit("chat-restored", ${JSON.stringify(history)});
    emit("chat-thought", "Live [live](https://example.com/live)\\n**kept as written**");
    for (const link of document.querySelectorAll("#log .md-link")) link.click();`);
    assert.equal(report.error, undefined, report.error);
    const text = (kind) => report.rows.find((row) => row.kind === kind).text;
    assert.equal(text("thought"), "Saw spec\n[link passwd file:///etc/passwd]");
    assert.equal(text("restored"), "brief");
    assert.equal(
      report.rows.find((row) => row.kind === "note" && row.label === "Thinking").text,
      "Live live\n**kept as written**",
    );
    const urls = ["https://example.com/spec.md", "https://example.com/brief", "https://example.com/live"];
    assert.deepEqual([...report.links].sort(), [...urls].sort());
    assert.deepEqual([...report.opened].sort(), [...urls].sort());
    assert.equal(report.anchors, 0, "an <a href> would navigate the Chat webview");
  },
);

// A replayed Prompt row draws what another client sent as written, with only
// its links live: a typed `[x](https://...)` opens, and a file link is words.
test(
  "a link in a replayed prompt opens, and a file link in it does not",
  { skip: CHROME_OR_CI, timeout: 60000 },
  async () => {
    const history = [{ type: "prompt", text: "see [x](https://example.com) and [y](file:///a)\n**kept**" }];
    const report = await drive(`
    emit("chat-restored", ${JSON.stringify(history)});
    for (const link of document.querySelectorAll("#log .md-link")) link.click();`);
    assert.equal(report.error, undefined, report.error);
    assert.equal(report.rows.find((row) => row.kind === "prompt").text, "see x and y\n**kept**");
    assert.deepEqual(report.links, ["https://example.com"]);
    assert.deepEqual(report.opened, ["https://example.com"]);
    assert.equal(report.anchors, 0);
  },
);

// A replayed tool call draws every field the wire sent. Locations come first,
// as the path and line ACP gave. A diff is its path, its counts, and a note
// that the full diff is not drawn. Text is as sent, never Markdown.
const TOOL_HISTORY = [
  {
    type: "tool_call",
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
      { type: "mark", markdown: "[image image/png]" },
      { type: "mark", markdown: "[link passwd file:///etc/passwd]" },
    ],
  },
];

test(
  "a replayed tool call draws its locations, diff note and counts, terminal id, text and marks",
  { skip: CHROME_OR_CI, timeout: 60000 },
  async () => {
    const report = await drive(`
    emit("chat-restored", ${JSON.stringify(TOOL_HISTORY)});
    for (const link of document.querySelectorAll("#log .md-link")) link.click();`);
    assert.equal(report.error, undefined, report.error);
    assert.deepEqual(report.tools, [
      {
        label: "Edit main.rs · completed",
        pieces: [
          "/Users/oded/src/main.rs:12",
          "/tmp/page.html",
          "/Users/oded/src/main.rs (+3/-1) · full diff not drawn",
          "Terminal term-7",
          "wrote [x](https://evil.example) and **2** hunks",
          "spec",
          "[image image/png]",
          "[link passwd file:///etc/passwd]",
        ],
        spaces: ["normal", "normal", "normal", "normal"],
      },
    ]);
    assert.deepEqual(report.links, ["https://example.com/spec.md"]);
    assert.deepEqual(report.opened, ["https://example.com/spec.md"]);
    assert.equal(report.anchors, 0, "an <a href> would navigate the Chat webview");
  },
);

// A path or an id is the Harness's, and a line break or a right-to-left
// override in it must not make a second line or reorder the text around it.
test(
  "a replayed tool row keeps a hostile path or terminal id on one line, with no override",
  { skip: CHROME_OR_CI, timeout: 60000 },
  async () => {
    const history = [
      {
        type: "tool_call",
        id: "t-hostile",
        title: "Edit",
        kind: "edit",
        status: "completed",
        locations: [{ path: "/tmp/a\nb\u202ec.rs", line: 2 }],
        content: [
          { type: "diff", path: "/tmp/x\r\ny\u202ez.rs", added: 1, removed: 0, approximate: false },
          { type: "terminal", label: "Terminal t\n1\u202e2" },
          { type: "text", text: "kept\nlines\u202e" },
        ],
      },
    ];
    const report = await drive(`emit("chat-restored", ${JSON.stringify(history)});`);
    assert.equal(report.error, undefined, report.error);
    assert.deepEqual(report.tools[0].pieces, [
      "/tmp/a\nbc.rs:2",
      "/tmp/x\r\nyz.rs (+1/-0) · full diff not drawn",
      "Terminal t\n12",
      "kept\nlines",
    ]);
    assert.deepEqual(report.tools[0].spaces, ["normal", "normal", "normal"]);
  },
);

test(
  "a diff too big to count exactly says its counts are approximate",
  { skip: CHROME_OR_CI, timeout: 60000 },
  async () => {
    const history = [
      {
        type: "tool_call",
        id: "t-big",
        title: "Rewrite",
        kind: "edit",
        status: "completed",
        locations: [],
        content: [{ type: "diff", path: "/big.rs", added: 100000, removed: 90000, approximate: true }],
      },
    ];
    const report = await drive(`emit("chat-restored", ${JSON.stringify(history)});`);
    assert.equal(report.error, undefined, report.error);
    assert.deepEqual(report.tools[0].pieces, ["/big.rs (+100000/-90000, approximate) · full diff not drawn"]);
  },
);
