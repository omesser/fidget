import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
const CSS = readFileSync(new URL("../src/ui-tokens.css", import.meta.url), "utf8");
const DESIGNS = ["minimal", "terminal", "glass"];

function declaredToken(selector, name) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const block = CSS.match(new RegExp(`${escaped}\\s*\\{([^}]+)\\}`));
  assert.ok(block, `no block for ${selector}`);
  const decl = block[1].match(new RegExp(`${name}\\s*:\\s*([^;]+);`));
  assert.ok(decl, `${selector} has no ${name}`);
  return resolveToken(decl[1].trim());
}

function resolveToken(value) {
  const compact = value.replace(/\s+/g, "");
  const ref = compact.match(/^var\((--[A-Za-z0-9-]+)\)$/);
  if (!ref) {
    return compact;
  }
  const snippet = readFileSync(new URL("../src/bubble-tokens.css", import.meta.url), "utf8");
  const decl = snippet.match(new RegExp(`${ref[1]}\\s*:\\s*([^;]+);`));
  assert.ok(decl, `${ref[1]} is not in bubble-tokens.css`);
  return decl[1].trim().replace(/\s+/g, "");
}

// One Chrome launch per test: `body` runs once per design on the same page and
// returns that design's readings. A launch costs seconds, the loop costs nothing.
async function probe(body, forceDark, preferredColorScheme) {
  const appearance = pathToFileURL(join(SRC, "chat-appearance.js")).href;
  const html = `<!doctype html>
<html>
  <head>
    <link rel="stylesheet" href="chat-ui.css">
  </head>
  <body>
    <script type="module">
      import { mountChatAppearance } from ${JSON.stringify(appearance)};
      const html = document.documentElement;
      const panel = () => getComputedStyle(html).getPropertyValue("--ui-panel").trim();
      const read = (extra) => ({
        panel: panel(),
        colorScheme: getComputedStyle(html).colorScheme,
        palette: html.dataset.chatPalette,
        ...extra,
      });
      const out = {};
      for (const design of ${JSON.stringify(DESIGNS)}) {
        html.className = "chat-ui-" + design;
        out[design] = (() => {
          ${body}
        })();
      }
      console.log("__PROBE_READY__");
      console.log("__PROBE_RESULT__" + JSON.stringify(out));
    </script>
  </body>
</html>`;
  const args = [];
  if (forceDark) {
    args.push("--force-dark-mode");
  }
  if (preferredColorScheme !== undefined) {
    args.push(`--blink-settings=preferredColorScheme=${preferredColorScheme}`);
  }
  return probePage(html, { args, timeout: 3100 });
}

const flat = (value) => value.replace(/\s+/g, "");
const browser = { skip: chromeSkip, timeout: 60000 };

test("forced light paints each design's light panel and pins color-scheme", browser, async () => {
  const reports = await probe(
    `
    mountChatAppearance(html)("light");
    return read();
  `,
    true,
  );
  for (const design of DESIGNS) {
    const expected = declaredToken(`[data-chat-palette="light"].chat-ui-${design}`, "--ui-panel");
    assert.equal(reports[design].colorScheme, "light", design);
    assert.equal(flat(reports[design].panel), expected, design);
  }
});

test("system and dark under force-dark-mode keep the dark panel", browser, async () => {
  const reports = await probe(
    `
    mountChatAppearance(html)("dark");
    const dark = read();
    mountChatAppearance(html)("system");
    return { dark, system: read() };
  `,
    true,
  );
  for (const design of DESIGNS) {
    const expected = declaredToken(`.chat-ui-${design}`, "--ui-panel");
    const { dark, system } = reports[design];
    assert.equal(dark.colorScheme, "dark", design);
    assert.equal(flat(dark.panel), expected, `${design} dark`);
    assert.equal(system.palette, "dark", `${design} system`);
    assert.equal(flat(system.panel), expected, `${design} system panel`);
  }
});

test("changing data-chat-palette from dark to light repaints the panel", browser, async () => {
  const reports = await probe(
    `
    const apply = mountChatAppearance(html);
    apply("dark");
    const before = panel();
    apply("light");
    return read({ before });
  `,
    true,
  );
  for (const design of DESIGNS) {
    const report = reports[design];
    assert.equal(flat(report.before), declaredToken(`.chat-ui-${design}`, "--ui-panel"), design);
    assert.equal(
      flat(report.panel),
      declaredToken(`[data-chat-palette="light"].chat-ui-${design}`, "--ui-panel"),
      design,
    );
    assert.equal(report.colorScheme, "light", design);
  }
});

test("system follows browser preference and dark forces dark panel", browser, async () => {
  for (const [schemeName, schemeValue] of [["light", 1], ["dark", 0]]) {
    const reports = await probe(
      `
      const apply = mountChatAppearance(html);
      apply("system");
      const systemPalette = html.dataset.chatPalette;
      const systemPanel = panel();
      const systemScheme = getComputedStyle(html).colorScheme;
      apply("dark");
      const darkPanel = panel();
      const darkScheme = getComputedStyle(html).colorScheme;
      apply("light");
      return read({ systemPalette, systemPanel, systemScheme, darkPanel, darkScheme });
    `,
      false,
      schemeValue,
    );
    for (const design of DESIGNS) {
      const darkPanel = declaredToken(`.chat-ui-${design}`, "--ui-panel");
      const lightPanel = declaredToken(`[data-chat-palette="light"].chat-ui-${design}`, "--ui-panel");
      const systemPanel = schemeName === "light" ? lightPanel : darkPanel;
      const report = reports[design];
      assert.equal(report.systemPalette, schemeName, `${schemeName} ${design} system palette`);
      assert.equal(flat(report.systemPanel), systemPanel, `${schemeName} ${design} system panel`);
      assert.equal(report.systemScheme, schemeName, `${schemeName} ${design} system scheme`);
      assert.equal(flat(report.darkPanel), darkPanel, `${schemeName} ${design} dark panel`);
      assert.equal(report.darkScheme, "dark", `${schemeName} ${design} dark scheme`);
      assert.equal(flat(report.panel), lightPanel, `${schemeName} ${design} light panel`);
      assert.equal(report.colorScheme, "light", `${schemeName} ${design} light scheme`);
    }
  }
});
