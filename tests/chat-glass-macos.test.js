// Glass on macOS: the native window effect behind the webview blurs the
// desktop, so under data-platform="macos" the panel drops its CSS blur rather
// than blur that blur again. Elsewhere the CSS blur stays.
import assert from "node:assert/strict";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";

async function bodyStyles() {
  const html = `<!doctype html>
<html class="chat-ui-glass">
  <head><link rel="stylesheet" href="chat-ui.css"></head>
  <body>
    <section class="prompt"><div class="frozen" id="personality"></div><textarea></textarea></section>
    <script>
      const root = document.documentElement;
      const out = { backdrops: {}, tints: {}, promptTextareaRadii: {}, promptFrozenRadii: {} };
      for (const platform of ["macos", null]) {
        if (platform) root.dataset.platform = platform; else delete root.dataset.platform;
        out.backdrops[platform || "other"] = getComputedStyle(document.body).backdropFilter;
      }
      const promptTextarea = document.querySelector(".prompt textarea");
      const promptFrozen = document.querySelector(".prompt .frozen");
      root.dataset.platform = "macos";
      for (const palette of ["dark", "light"]) {
        root.dataset.chatPalette = palette;
        out.tints[palette] = [...getComputedStyle(document.body).backgroundImage.matchAll(/rgba\\([^)]*,\\s*([\\d.]+)\\)/g)].map((m) => Number(m[1]));
        out.promptTextareaRadii[palette] = getComputedStyle(promptTextarea).borderRadius;
        out.promptFrozenRadii[palette] = getComputedStyle(promptFrozen).borderRadius;
      }
      console.log("__PROBE_RESULT__" + JSON.stringify(out));
    </script>
  </body>
</html>`;
  return probePage(html);
}

// One probe serves every assertion here; splitting tests must not multiply
// Chrome launches over the same page.
let stylesPromise;
function sharedStyles() {
  stylesPromise ||= bodyStyles();
  return stylesPromise;
}

test("Glass drops its CSS blur under the macOS flag and keeps it elsewhere", { skip: chromeSkip, timeout: 60000 }, async () => {
  assert.deepEqual((await sharedStyles()).backdrops, {
    macos: "none",
    other: "blur(22px) saturate(1.5)",
  });
});

test("Glass macOS body tints are lighter in both palettes", { skip: chromeSkip, timeout: 60000 }, async () => {
  assert.deepEqual((await sharedStyles()).tints, {
    dark: [0.22, 0.3],
    light: [0.55, 0.65],
  });
});

test("Glass macOS Prompt textarea keeps a box-shaped radius in both palettes", { skip: chromeSkip, timeout: 60000 }, async () => {
  assert.deepEqual((await sharedStyles()).promptTextareaRadii, {
    dark: "14px",
    light: "14px",
  });
});

test("Glass macOS frozen Prompt layers share the textarea's box-shaped radius", { skip: chromeSkip, timeout: 60000 }, async () => {
  const styles = await sharedStyles();
  assert.deepEqual(styles.promptFrozenRadii, styles.promptTextareaRadii,
    "App-level instructions and Personality read as prose blocks, not control capsules");
});
