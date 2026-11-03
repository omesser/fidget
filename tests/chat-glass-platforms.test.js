import assert from "node:assert/strict";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";

async function glassStyles() {
  const html = `<!doctype html>
<html class="chat-ui-glass">
  <head><link rel="stylesheet" href="chat-ui.css"></head>
  <body>
    <div class="prompt-actions"></div>
    <section class="prompt"><textarea></textarea></section>
    <script>
      const root = document.documentElement;
      const actions = document.querySelector(".prompt-actions");
      const textarea = document.querySelector(".prompt textarea");
      const alphaValues = (value) => [...value.matchAll(/rgba\\([^)]*,\\s*([\\d.]+)\\)/g)].map((match) => Number(match[1]));
      const alpha = (value) => Number(value.match(/rgba?\\([^)]*,\\s*([\\d.]+)\\)/)?.[1]);
      const out = {};
      for (const platform of ["windows", "linux", null]) {
        if (platform) root.dataset.platform = platform; else delete root.dataset.platform;
        for (const palette of ["dark", "light"]) {
          root.dataset.chatPalette = palette;
          const body = getComputedStyle(document.body);
          out[(platform || "other") + " " + palette] = {
            bodyTints: alphaValues(body.backgroundImage),
            panel: alpha(getComputedStyle(root).getPropertyValue("--chat-panel")),
            footer: alpha(getComputedStyle(actions).backgroundColor),
            promptRadius: getComputedStyle(textarea).borderRadius,
          };
        }
      }
      console.log("__PROBE_RESULT__" + JSON.stringify(out));
    </script>
  </body>
</html>`;
  return probePage(html);
}

test("Windows Glass paints the panel and footer at the body's heavier bottom tint", { skip: chromeSkip, timeout: 60000 }, async () => {
  const styles = await glassStyles();
  for (const palette of ["dark", "light"]) {
    const windows = styles["windows " + palette];
    const plain = styles["other " + palette];
    assert.equal(windows.panel, windows.bodyTints[1], palette);
    assert.equal(windows.footer, windows.bodyTints[1], palette);
    assert.ok(windows.bodyTints[1] > plain.bodyTints[1],
      `${palette}: Windows opaque ${windows.bodyTints[1]} must exceed plain Glass ${plain.bodyTints[1]}`);
    assert.equal(windows.promptRadius, "14px", palette);
  }
});

test("Linux Glass keeps its tint and uses a box-shaped Prompt editor", { skip: chromeSkip, timeout: 60000 }, async () => {
  const styles = await glassStyles();
  assert.deepEqual(styles["linux dark"], {
    bodyTints: [0.92, 0.93],
    panel: 0.62,
    footer: 0.62,
    promptRadius: "14px",
  });
  assert.deepEqual(styles["linux light"], {
    bodyTints: [0.93, 0.95],
    panel: 0.85,
    footer: 0.85,
    promptRadius: "14px",
  });
});
