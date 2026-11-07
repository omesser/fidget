// The landing's commands read as code, its URLs open in the browser, and all
// of it copies as plain text. Drives src/chat.html the way
// chat-reply-code.test.js does; a real drag and paste still need a person.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
// The fixture launcher of scripts/scenarios/ci/launcher-dies-at-startup.sh: one
// unbroken path, the shape that pushed the landing sideways.
const LAUNCHER = "/private/tmp/claude-501/scratchpad/pr1088/wt/target/debug/deps/fidget-b9e5365f61326ae2";
const FAILED = {
  command: `${LAUNCHER} harness::tests::fake_acp_agent --exact --nocapture script=abort-first`,
  reason: "exited before initialize, signal: 6 (SIGABRT)",
  output: "running 1 test\ndyld[0]: Library not loaded: /opt/homebrew/opt/llhttp/lib/libllhttp.9.3.dylib",
  node_check: null,
};

function opening(harness) {
  return {
    name: "BMO",
    character: "Buddy Bot",
    configured: true,
    enabled: true,
    harness: { name: "codex", session: null, alive: false, login: null, ...harness },
    model: "",
    host: "",
    chat_ui: "minimal",
    login: null,
    harness_name: "codex",
    instructions: "",
    personality: "",
    instance_prompt: "",
    prompt_limit: 2000,
  };
}

function paint(open, width = 420) {
  const stub = `
<script type="module">
  ${pageWait()}
  const handlers = {};
  window.__invoked = [];
  window.__TAURI__ = {
    core: {
      invoke(name, args) {
        window.__invoked.push({ name, args });
        if (name === "chat_opening") return Promise.resolve(${JSON.stringify(open)});
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
    const title = document.getElementById("landing-title");
    const lede = document.getElementById("landing-lede");
    await ready("chat");
    await until(() => !document.getElementById("empty").hidden);
    const range = document.createRange();
    range.setStartBefore(title);
    range.setEndAfter(lede);
    getSelection().removeAllRanges();
    getSelection().addRange(range);
    const selected = getSelection().toString();
    const link = lede.querySelector(".md-link");
    const code = lede.querySelector("code");
    let clicked = null;
    if (link) {
      const before = window.__invoked.length;
      link.click();
      clicked = window.__invoked.slice(before).find((call) => call.name === "open_link") ?? null;
    }
    const style = (node) => {
      if (!node) return null;
      const s = getComputedStyle(node);
      return { font: s.fontFamily, color: s.color, line: s.textDecorationLine, select: s.userSelect };
    };
    const mind = document.getElementById("mind-text");
    const log = document.getElementById("log");
    const box = (part) => {
      const section = document.getElementById(\`failure-\${part}-part\`);
      return section.hidden || section.closest("[hidden]")
        ? null
        : { label: section.querySelector("h3").textContent, text: section.querySelector("pre").textContent };
    };
    const report = {
      title: title.textContent,
      titleCode: [...title.querySelectorAll("code")].map((n) => n.textContent),
      ledeCode: [...lede.querySelectorAll("code")].map((n) => n.textContent),
      mindCode: [...mind.querySelectorAll("code")].map((n) => n.textContent),
      backticks: (title.textContent + lede.textContent + mind.textContent).includes("\`"),
      links: [...lede.querySelectorAll(".md-link")].map((n) => n.dataset.href),
      anchors: document.querySelectorAll("#landing a").length,
      selected,
      clicked,
      lede: style(lede),
      code: style(code),
      link: style(link),
      accent: getComputedStyle(document.documentElement).getPropertyValue("--ui-accent").trim(),
      kicker: document.getElementById("landing-kicker").hidden ? null : document.getElementById("landing-kicker").textContent,
      boxes: { output: box("output"), command: box("command"), check: box("check") },
      next: document.getElementById("landing-next").hidden ? null : document.getElementById("landing-next").textContent,
      // The composer covers the log's foot, so its top is the fold.
      outputTop: Math.round(document.getElementById("failure-output").getBoundingClientRect().top),
      fold: Math.round(document.getElementById("line").getBoundingClientRect().top),
      sideways: Math.max(
        document.documentElement.scrollWidth - document.documentElement.clientWidth,
        log.scrollWidth - log.clientWidth,
      ),
    };
    console.log("__PROBE_RESULT__" + JSON.stringify(report));
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "chat.html"), "utf8").replace(
    "<head>",
    `<head>${stub}`,
  );
  return probePage(html, { args: [`--window-size=${width},560`] });
}

for (const width of [420, 360, 320]) {
  test(`a launcher that failed to start boxes what it ran and printed at ${width}`, { skip: chromeSkip, timeout: 60000 }, async () => {
    const report = await paint(opening({ failed: FAILED }), width);
    assert.equal(report.kicker, "Harness error");
    assert.equal(report.title, "Codex couldn't start");
    assert.deepEqual(report.boxes, {
      output: { label: "Error output", text: FAILED.output },
      command: { label: "Command", text: FAILED.command },
      check: null,
    });
    assert.equal(report.next, "Fix the error above, then pick it again, or pick a different Harness below.");
    assert.equal(report.sideways, 0, "the landing scrolls sideways");
    assert.equal(report.backticks, false);
    assert.notEqual(report.lede.select, "none");
  });
}

test("a Harness named by its launcher path keeps the path out of the title (#1186)", { skip: chromeSkip, timeout: 60000 }, async () => {
  const named = opening({ name: LAUNCHER, failed: FAILED });
  named.harness_name = LAUNCHER;
  const report = await paint(named, 320);
  assert.equal(report.title, "Harness couldn't start");
  assert.ok(report.outputTop < report.fold, `Error output starts at ${report.outputTop}, under the composer at ${report.fold}`);
  assert.equal(report.boxes.command.text, FAILED.command);
  assert.equal(report.sideways, 0, "the title pushed the landing sideways");
});

test("an npx launcher that failed shows the Node.js check in its own box", { skip: chromeSkip, timeout: 60000 }, async () => {
  const report = await paint(
    opening({
      failed: { command: "npx -y @agentclientprotocol/codex-acp@latest", reason: "exited before initialize", output: "", node_check: "node --version" },
    }),
  );
  assert.equal(report.boxes.output, null);
  assert.deepEqual(report.boxes.check, { label: "Check that Node.js starts", text: "node --version" });
  assert.match(report.selected, /It exited before initialize, and printed nothing\./);
});

test("a launcher that failed preflight boxes the probe it ran and what it printed", { skip: chromeSkip, timeout: 60000 }, async () => {
  const unhealthy = {
    command: "npx --version",
    reason: "exited with exit status: 1",
    output: "npm ERR! code ENOENT",
    node_check: "node --version",
  };
  const report = await paint(opening({ unhealthy }), 320);
  assert.equal(report.kicker, "Harness error");
  assert.equal(report.title, "Codex couldn't start");
  assert.deepEqual(report.boxes, {
    output: { label: "Error output", text: "npm ERR! code ENOENT" },
    command: { label: "Command", text: "npx --version" },
    check: { label: "Check that Node.js starts", text: "node --version" },
  });
  assert.match(report.selected, /It exited with exit status: 1\. This is what it printed:/);
  assert.equal(report.sideways, 0, "the landing scrolls sideways");
});

test("a missing launcher links its install page to the browser", { skip: chromeSkip, timeout: 60000 }, async () => {
  const report = await paint(opening({ missing: "npx", install: "https://nodejs.org/" }));
  assert.equal(report.title, "Codex needs npx");
  assert.deepEqual(report.titleCode, ["npx"]);
  assert.deepEqual(report.mindCode, ["npx"]);
  assert.equal(report.backticks, false);
  assert.deepEqual(report.links, ["https://nodejs.org/"]);
  assert.equal(report.anchors, 0, "an <a href> would navigate the Chat webview");
  assert.equal(report.link.line, "underline");
  assert.notEqual(report.link.color, report.lede.color, "a link is not coloured apart from its sentence");
  assert.deepEqual(report.clicked, { name: "open_link", args: { url: "https://nodejs.org/" } });
  assert.match(report.selected, /Install from https:\/\/nodejs\.org\/\./);
  assert.match(report.selected, /npx is not installed/);
});
