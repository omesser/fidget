// An elicitation question and a plan step reach Chat with their lines and
// length. Drives src/chat.html as chat-elicit-link.test.js does and reads
// `innerText`, where a line break the CSS collapses reads as a space.

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

const opening = {
  name: "BMO",
  character: "Buddy Bot",
  configured: true,
  enabled: true,
  harness: { name: "codex", session: null, alive: true, login: "codex login" },
  model: "",
  host: "",
  chat_ui: "minimal",
  login: "codex login",
  sign_in: [],
  harness_name: "codex",
  instructions: "",
  personality: "",
  instance_prompt: "",
  prompt_limit: 2000,
};

const LONG = "Pick the branch the release goes out from. ".repeat(20).trim();

const form = {
  request: "8",
  message: `Which branch?\n- main\n-\u202e release\n${LONG}`,
  field: "branch",
  options: [{ value: "main", name: "main" }],
  url: null,
};

const steps = [
  { content: `write the\npatch\u2066\n${LONG}`, status: "in_progress", priority: "high" },
];

function paint() {
  const stub = `
<script type="module">
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
        return Promise.resolve(() => {});
      },
    },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "chat-test" }) },
  };
  const tick = () => new Promise((r) => setTimeout(r, 0));
  async function drive() {
    while (!handlers["chat-elicitation"] || !handlers["chat-plan"] || document.getElementById("landing").hidden) {
      await tick();
    }
    handlers["chat-elicitation"]({ payload: ${JSON.stringify(form)} });
    handlers["chat-plan"]({ payload: ${JSON.stringify(steps)} });
    await tick();
    const out = document.createElement("pre");
    out.id = "probe";
    out.textContent = JSON.stringify({
      question: document.querySelector(".row.ask .said").innerText,
      step: document.querySelector("#plan .step").innerText,
    });
    document.body.append(out);
  }
  window.addEventListener("load", drive);
</script>`;

  const dir = mkdtempSync(join(tmpdir(), "chat-elicit-plan-render-"));
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
      "--dump-dom",
      pathToFileURL(page).href,
    ],
    { encoding: "utf8", maxBuffer: 1 << 24, timeout: 40000, killSignal: "SIGKILL" },
  );
  const match = run.stdout.match(/<pre id="probe"[^>]*>(.*?)<\/pre>/s);
  assert.ok(match, `the window did not report. ${run.stderr?.slice(-500) ?? ""}`);
  const json = match[1]
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">");
  return JSON.parse(json);
}

const skip = chrome ? false : "headless Chromium is not installed";

test("a question and a plan step are drawn whole, line by line, without unsafe characters", { skip, timeout: 60000 }, () => {
  const report = paint();
  assert.equal(report.question, `Which branch?\n- main\n- release\n${LONG}\nmain\nDecline`);
  assert.equal(report.step, `write the\npatch\n${LONG}`);
});
