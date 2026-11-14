// A branded connect leaves the landing status at the top of the log.
// Drives src/chat.html.

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { chromeSkip, chromePath, cleanupChrome, createPageTarget, injectBase, launchWithPipe } from "./chrome-cdp.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));

const unconfigured = {
  name: "BMO",
  character: "Buddy Bot",
  configured: false,
  enabled: true,
  harness: null,
  model: "",
  host: "",
  chat_ui: "minimal",
  login: null,
  harness_name: null,
  instructions: "",
  personality: "",
  instance_prompt: "",
  prompt_limit: 2000,
};

function opening(harness) {
  return {
    ...unconfigured,
    configured: true,
    harness_name: "grok",
    harness: { name: "grok", session: null, alive: false, login: null, ...harness },
  };
}

const initializing = opening({ initializing: true });
const ready = opening({ alive: true, session: "abcdef0123456789" });

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function withChat(run, { rejectHarness = false } = {}) {
  const stub = `
<script type="module">
  const handlers = {};
  window.__handlers = handlers;
  window.__invoked = [];
  window.__TAURI__ = {
    core: {
      invoke(name, args) {
        window.__invoked.push({ name, args });
        if (name === "chat_opening") return Promise.resolve(${JSON.stringify(unconfigured)});
        if (name === "select_harness") {
          return ${rejectHarness ? 'Promise.reject("unknown Harness preset: grok")' : 'Promise.resolve("ok")'};
        }
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
  window.__snap = () => {
    const log = document.getElementById("log");
    const title = document.getElementById("landing-title");
    const active = document.activeElement;
    const logBox = log.getBoundingClientRect();
    const titleBox = title.getBoundingClientRect();
    const note = log.querySelector(".note:last-of-type");
    const noteBox = note?.getBoundingClientRect();
    return {
      scrollTop: log.scrollTop,
      title: title.textContent,
      titleInView: titleBox.top >= logBox.top - 1 && titleBox.top < logBox.bottom,
      active: active?.dataset?.harness || active?.id || active?.tagName,
      invoked: window.__invoked.map((call) => call.name),
      note: note?.textContent ?? "",
      noteInView: Boolean(noteBox && noteBox.top >= logBox.top - 1 && noteBox.bottom <= logBox.bottom + 1),
    };
  };
  window.__reveal = (harness) => {
    const log = document.getElementById("log");
    const btn = document.querySelector(\`.connect-btn[data-harness="\${harness}"]\`);
    log.scrollTop = 0;
    const logBox = log.getBoundingClientRect();
    const btnBox = btn.getBoundingClientRect();
    log.scrollTop = Math.max(0, btnBox.top - logBox.top - 24);
    const placed = btn.getBoundingClientRect();
    return {
      x: placed.left + placed.width / 2,
      y: placed.top + placed.height / 2,
      scrollTop: log.scrollTop,
    };
  };
</script>`;
  const html = injectBase(readFileSync(join(SRC, "chat.html"), "utf8").replace(
    "<head>",
    `<head>${stub}`,
  ));
  const cdp = launchWithPipe(chromePath, [
    "--headless=new",
    "--disable-gpu",
    "--no-sandbox",
    "--disable-dev-shm-usage",
    "--allow-file-access-from-files",
    "--window-size=420,560",
  ]);
  try {
    const page = join(cdp.dir, "harness.html");
    writeFileSync(page, html);
    const { sessionId, navigate } = await createPageTarget(cdp);
    const send = (method, params = {}) => cdp.send(method, params, sessionId);
    const evalJs = async (expression) => {
      const msg = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
      assert.equal(msg.exceptionDetails, undefined, JSON.stringify(msg.exceptionDetails));
      return msg.result?.value;
    };
    await navigate(pathToFileURL(page).href);

    // A navigation swaps the document under a long-lived wait, so poll instead.
    const ready = `!!(window.__reveal && window.__handlers?.["chat-opening"] && document.getElementById("empty")?.hidden === false)`;
    let loaded = false;
    for (let i = 0; i < 100 && !loaded; i++) {
      // An evaluate that lands mid-navigation fails; the next one tries again.
      const msg = await send("Runtime.evaluate", { expression: ready, returnByValue: true }).catch(() => null);
      loaded = msg?.result?.value === true;
      if (!loaded) await sleep(50);
    }
    assert.ok(loaded, "chat.html never finished loading");

    await run({ send, evalJs });
  } finally {
    await cleanupChrome(cdp);
  }
}

test("a branded connect keeps the landing status at the top", { skip: chromeSkip, timeout: 60000 }, async () => {
  await withChat(async ({ send, evalJs }) => {
    const place = await evalJs(`window.__reveal("grok")`);
    assert.ok(place.scrollTop > 0, "the transcript could not scroll");

    await send("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x: place.x,
      y: place.y,
      button: "left",
      clickCount: 1,
    });
    await send("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: place.x,
      y: place.y,
      button: "left",
      clickCount: 1,
    });

    const clicked = await evalJs("window.__snap()");
    assert.ok(clicked.invoked.includes("select_harness"), "the click missed the branded button");
    assert.equal(clicked.scrollTop, 0);
    assert.equal(clicked.active, "BODY");
    assert.equal(clicked.titleInView, true);

    await evalJs(`window.__handlers["chat-opening"]({ payload: ${JSON.stringify(initializing)} })`);
    await evalJs(`window.__handlers["chat-session"]({ payload: "settings changed what answers" })`);
    const aftermath = await evalJs("window.__snap()");
    assert.equal(aftermath.title, "Initializing Grok…");
    assert.equal(aftermath.scrollTop, 0);
    assert.equal(aftermath.titleInView, true);
    assert.equal(aftermath.active, "BODY");

    await evalJs(`window.__handlers["chat-opening"]({ payload: ${JSON.stringify(ready)} })`);
    const long = `${"line\n".repeat(40)}end`;
    await evalJs(`window.__handlers.chat({ payload: { you: true, said: ${JSON.stringify(long)} } })`);
    const followed = await evalJs("window.__snap()");
    assert.ok(followed.scrollTop > 0, `a later line stayed pinned at ${followed.scrollTop}`);
  });
});

test("a branded connect the Shell refuses shows its error", { skip: chromeSkip, timeout: 60000 }, async () => {
  await withChat(
    async ({ send, evalJs }) => {
      const place = await evalJs(`window.__reveal("grok")`);
      await send("Input.dispatchMouseEvent", {
        type: "mousePressed",
        x: place.x,
        y: place.y,
        button: "left",
        clickCount: 1,
      });
      await send("Input.dispatchMouseEvent", {
        type: "mouseReleased",
        x: place.x,
        y: place.y,
        button: "left",
        clickCount: 1,
      });
      await evalJs("new Promise((resolve) => setTimeout(resolve, 0))");
      const failed = await evalJs("window.__snap()");
      assert.match(failed.note, /Could not connect to Grok: unknown Harness preset: grok/);
      assert.equal(failed.noteInView, true);
    },
    { rejectHarness: true },
  );
});
