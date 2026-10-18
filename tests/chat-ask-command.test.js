// The stand-in in chat-ask-row.test.js cannot see paint, and paint is half the
// promise: the command reads as a command because it is drawn the way reply code
// is. `cursor-agent` sends it as one code span in the call's title.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { findChrome, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

// Captured from `cursor-agent acp` (2026.10.01-e373342) for "Set an alarm for
// 21:20 using a shell command.": the title is the command as a code span, the
// content is the shell's own note, and the kind is the tail.
const COMMAND = "(sleep 120 && osascript -e 'display notification \"It is 21:20.\"') &";
const NOTE = "Shell allowlist is empty";
// A span inside a longer title, with a line ending and an underscored path
// beside it: the span is code, and everything else keeps its characters.
const MARKED = "Allow `python3` for this turn\n(Read __init__.py first)";
const REPLY = "Done: `open -a Calculator` is up.";

function ask(request, title, content) {
  return {
    request,
    title,
    kind: "execute",
    content,
    input: null,
    locations: [],
    options: [
      { id: "allow-once", name: "Allow once", kind: "allow_once" },
      { id: "reject-once", name: "Reject", kind: "reject_once" },
    ],
  };
}

const chrome = findChrome();

function surface(theme) {
  const opening = {
    name: "BMO",
    character: "Buddy Bot",
    configured: true,
    enabled: true,
    harness: { alive: true, missing: false, initializing: false },
    model: "",
    host: "",
    chat_ui: theme,
    login: null,
    harness_name: "cursor-agent",
    instructions: "",
    personality: "",
    instance_prompt: "",
    prompt_limit: 2000,
  };
  const stub = `
<script>
  ${pageWait()}
  const handlers = {};
  window.__TAURI__ = {
    core: {
      invoke(name) {
        if (name === "chat_opening") return Promise.resolve(${JSON.stringify(opening)});
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
  function paintOf(node) {
    if (!node) return null;
    const style = getComputedStyle(node);
    return {
      backgroundColor: style.backgroundColor,
      borderTopWidth: style.borderTopWidth,
      fontFamily: style.fontFamily,
    };
  }
  function read() {
    const ask = [...document.querySelectorAll("#log .row.ask")].pop();
    const title = ask.querySelector(".ask-title");
    const code = title.querySelector("code");
    return {
      command: code ? code.textContent : null,
      commandPaint: paintOf(code),
      titleWhiteSpace: getComputedStyle(title).whiteSpace,
      metadata: ask.querySelector(".ask-metadata")?.textContent ?? "",
    };
  }
  async function drive() {
    console.log("__PROBE_READY__");
    await ready("chat-permission-settled");
    await until(() => !document.getElementById("line").disabled);
    handlers.chat({
      payload: {
        said: ${JSON.stringify(REPLY)},
        busy: false,
        reacting_to: null,
        you: false,
        at: null,
        error: null,
        superseded_by: null,
      },
    });
    await until(() => document.querySelector("#log .row.them .said code"));
    handlers["chat-permission"]({ payload: ${JSON.stringify(ask("5", "`" + COMMAND + "`", [NOTE]))} });
    await until(() => document.querySelector('#log .row.ask [data-option="allow-once"]'));
    const first = read();
    handlers["chat-permission"]({ payload: ${JSON.stringify(ask("6", MARKED, []))} });
    await until(() => document.querySelectorAll("#log .row.ask").length === 2);
    const second = read();
    console.log("__PROBE_RESULT__" + JSON.stringify({
      backticksLeft: [...document.querySelectorAll("#log .row.ask")].some((row) =>
        row.textContent.includes("\`")),
      note: document.querySelector("#log .row.ask .ask-content")?.textContent ?? "",
      panel: getComputedStyle(document.body).backgroundColor,
      replyPaint: paintOf(document.querySelector("#log .row.them .said code")),
      ...first,
      marked: second.command,
      markedTitle: document.querySelectorAll("#log .row.ask")[1]
        .querySelector(".ask-title").textContent,
      markedWhiteSpace: second.titleWhiteSpace,
    }));
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "chat.html"), "utf8").replace(
    "<head>",
    `<head><base href="${pathToFileURL(SRC).href}">${stub}`,
  );
  return probePage(html, { args: ["--force-dark-mode"] });
}

test(
  "an ask's command is drawn as code, painted like reply code",
  { skip: chrome ? false : "headless Chromium is not installed", timeout: 90000 },
  async () => {
    for (const theme of ["minimal", "terminal"]) {
      const report = await surface(theme);
      assert.equal(report.backticksLeft, false, theme);
      assert.equal(report.command, COMMAND, theme);
      assert.equal(report.note.includes(NOTE), true, theme);
      assert.equal(report.metadata, "execute", theme);
      assert.deepEqual(
        report.commandPaint,
        report.replyPaint,
        `${theme} the ask's command does not paint like reply code`,
      );
      // Absolute, so a paint that stopped being code on both sides at once
      // cannot pass the comparison above.
      assert.equal(report.commandPaint.borderTopWidth, "1px", `${theme} the command has no edge`);
      assert.match(report.commandPaint.fontFamily, /mono/, theme);
      assert.notEqual(
        report.commandPaint.backgroundColor,
        report.panel,
        `${theme} the command's background is the panel's`,
      );
      // The mark, not the shape or the kind: a span inside a longer title is
      // code too, and the characters around it are the Harness's own.
      assert.equal(report.marked, "python3", theme);
      assert.equal(report.markedTitle, "Allow python3 for this turn\n(Read __init__.py first)", theme);
      assert.equal(report.markedWhiteSpace, "pre-wrap", `${theme} the title's lines would fold`);
      assert.equal(report.titleWhiteSpace, "pre-wrap", theme);
    }
  },
);
