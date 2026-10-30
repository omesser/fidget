// Character Imports drawn on the real Settings page: its button is the same
// size as the other Settings buttons, the trust warning is a neutral notice
// under it, not a red error strip, and an empty status draws nothing.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
const read = (name) => JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), "utf8"));

test("Import from zip matches the New button, warns below it, and an empty status has no height", { skip: chromeSkip, timeout: 60000 }, async () => {
  const snapshot = {
    form: read("settings-snapshot-harnessDriving.json"),
    view: read("settings-values-harnessDriving.json"),
    reveal: null,
  };
  const stub = `
<script>
  ${pageWait()}
  window.__TAURI__ = {
    core: { invoke: (name) => Promise.resolve(name === "settings_snapshot" ? ${JSON.stringify(snapshot)} : undefined) },
    event: { listen(name) { listeningNow(name); return Promise.resolve(() => {}); } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: "settings-test" }) },
  };
  window.__TAURI_INTERNALS__ = { invoke: () => Promise.resolve([]) };
  const box = (node) => {
    const r = node.getBoundingClientRect();
    return { width: Math.round(r.width), height: Math.round(r.height) };
  };
  async function run() {
    console.log("__PROBE_READY__");
    await ready("settings-refresh");
    [...document.querySelectorAll('[role="tab"]')].find((t) => t.textContent === "Character").click();
    await settle();
    const imports = document.querySelector(".set-character-imports");
    const importButton = imports.querySelector("button");
    const newButton = document.querySelector('[data-row="new_instance"] button');
    console.log("__PROBE_RESULT__" + JSON.stringify({
      heading: imports.querySelector("h3").textContent,
      label: importButton.textContent,
      importHeight: box(importButton).height,
      newHeight: box(newButton).height,
      status: box(imports.querySelector(".set-status")),
      order: [...imports.children].map((node) => node.className || node.tagName),
      warning: imports.querySelector(".set-caution").textContent,
      cautionBar: getComputedStyle(imports.querySelector(".set-caution")).borderLeftColor,
      errorBar: (() => {
        const error = imports.appendChild(Object.assign(document.createElement("p"), { className: "set-notice" }));
        const color = getComputedStyle(error).borderLeftColor;
        error.remove();
        return color;
      })(),
    }));
  }
  window.addEventListener("load", () => run().catch((why) => {
    console.log("__PROBE_RESULT__" + JSON.stringify({ error: String(why) }));
  }));
</script>`;
  const html = readFileSync(join(SRC, "settings.html"), "utf8").replace("<head>", `<head>${stub}`);
  const seen = await probePage(html, { args: ["--window-size=760,900"] });
  assert.equal(seen.error, undefined, seen.error);
  assert.equal(seen.heading, "Character Imports");
  assert.equal(seen.label, "Import from zip");
  assert.equal(seen.importHeight, seen.newHeight, "same height as the New button");
  assert.deepEqual(seen.order, ["H3", "set-help", "set-controls", "set-notice set-caution", "set-status", "set-list"], "the trust warning sits under the button");
  assert.match(seen.warning, /Only import ones from people you trust/);
  assert.notEqual(seen.cautionBar, seen.errorBar, "the trust warning is not drawn as an error");
  assert.deepEqual(seen.status, { width: seen.status.width, height: 0 }, "empty status draws no strip");
});
