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
    <div class="prompt-actions"></div>
    <script>
      const root = document.documentElement;
      const out = { backdrops: {}, tints: {}, promptActionTints: {} };
      for (const platform of ["macos", null]) {
        if (platform) root.dataset.platform = platform; else delete root.dataset.platform;
        out.backdrops[platform || "other"] = getComputedStyle(document.body).backdropFilter;
      }
      const promptActions = document.querySelector(".prompt-actions");
      root.dataset.platform = "macos";
      for (const palette of ["dark", "light"]) {
        root.dataset.chatPalette = palette;
        out.tints[palette] = [...getComputedStyle(document.body).backgroundImage.matchAll(/rgba\\([^)]*,\\s*([\\d.]+)\\)/g)].map((m) => Number(m[1]));
        out.promptActionTints[palette] = Number(getComputedStyle(promptActions).backgroundColor.match(/rgba?\\([^)]*,\\s*([\\d.]+)\\)/)?.[1]);
      }
      console.log("__PROBE_RESULT__" + JSON.stringify(out));
    </script>
  </body>
</html>`;
  return probePage(html);
}

test("Glass drops its CSS blur under the macOS flag and keeps it elsewhere", { skip: chromeSkip, timeout: 60000 }, async () => {
  assert.deepEqual((await bodyStyles()).backdrops, {
    macos: "none",
    other: "blur(22px) saturate(1.5)",
  });
});

test("Glass macOS body tints are lighter in both palettes", { skip: chromeSkip, timeout: 60000 }, async () => {
  const styles = await bodyStyles();
  assert.deepEqual(styles.tints, {
    dark: [0.22, 0.3],
    light: [0.55, 0.65],
  });
  assert.deepEqual(styles.promptActionTints, {
    dark: 0.3,
    light: 0.65,
  });
});
