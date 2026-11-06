// A flex item's computed display is flex wherever it sits, so the parent
// and the boxes prove the row. Collapsed, the sentence and Advanced share
// one line. sameTop allows ±1px because a 10.5px font times 1.45 is 15.225px.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
const THEMES = ["minimal", "terminal", "glass"];
const CELL_IDS = [
  "s-behavior",
  "s-animation",
  "s-primitive",
  "s-state",
  "s-facing",
  "s-director",
  "s-happened",
];
const SHORT_CELLS = ["walk", "idle", "—", "—", "→", "wake 12s", "—"];
const LONG_CELLS = [
  "play-with-cursor",
  "idle-breathe",
  "Walk",
  "Climbing",
  "←",
  "wake 12m",
  "spoken to",
];
const OVER_CELLS = ["x".repeat(80), "—", "—", "—", "→", "wake 12s", "—"];
const CAP_CELLS = ["m".repeat(24), "m".repeat(24), "m".repeat(24), "m".repeat(24), "→", "wake 2h", "m".repeat(24)];
const PLAIN_LINE = "Wander toward cursor for a very long time · next thought in 12s";

function measure() {
  const opening = {
    name: "Buddy Bot",
    character: "Buddy Bot",
    configured: true,
    enabled: true,
    harness: {
      name: "cursor-agent",
      session: "fd4be1a2-497f-4899-a827-a4e42fbdc1f2",
      alive: true,
      missing: false,
      initializing: false,
      login: null,
    },
    model: "",
    host: "",
    chat_ui: "minimal",
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
  const IDS = ${JSON.stringify(CELL_IDS)};
  const THEMES = ${JSON.stringify(THEMES)};
  const round = (n) => Math.round(n * 100) / 100;
  function box(el) {
    const r = el.getBoundingClientRect();
    return { top: r.top, right: r.right, bottom: r.bottom, left: r.left, width: r.width, height: r.height };
  }
  function edges(bar) {
    const r = bar.getBoundingClientRect();
    const s = getComputedStyle(bar);
    const padLeft = parseFloat(s.paddingLeft);
    const padRight = parseFloat(s.paddingRight);
    return {
      left: r.left + padLeft,
      right: r.right - padRight,
      padLeft,
      padRight,
      top: r.top + parseFloat(s.borderTopWidth) + parseFloat(s.paddingTop),
      bottom: r.bottom - parseFloat(s.borderBottomWidth) - parseFloat(s.paddingBottom),
      rect: r,
      pad: parseFloat(s.paddingTop) + parseFloat(s.paddingBottom),
      border: parseFloat(s.borderTopWidth) + parseFloat(s.borderBottomWidth),
    };
  }
  function nodes() {
    return IDS.map((id) => document.getElementById(id));
  }
  function tops(list) {
    return new Set(list.map((el) => Math.round(el.getBoundingClientRect().top))).size;
  }
  function push(details, payload, open) {
    handlers["chat-status"]({ payload });
    details.open = open;
  }
  function contained(el, bar, ladder) {
    const r = el.getBoundingClientRect();
    const edge = edges(bar);
    const l = ladder.getBoundingClientRect();
    return r.width > 0 && r.height > 0
      && r.left >= edge.left - 0.5
      && r.right <= edge.right + 0.5
      && r.top >= edge.top - 0.5
      && r.bottom <= edge.bottom + 0.5
      && r.top >= l.top - 0.5
      && r.bottom <= l.bottom + 0.5;
  }
  function layout(bar, ladder, plain, summary) {
    const edge = edges(bar);
    const list = nodes();
    const rects = list.map((el) => el.getBoundingClientRect());
    const l = ladder.getBoundingClientRect();
    const p = plain.getBoundingClientRect();
    const s = summary.getBoundingClientRect();
    return {
      cells: list.map((el) => el.textContent),
      padLeft: edge.padLeft,
      padRight: edge.padRight,
      childIds: [...ladder.children].map((el) => el.id),
      cellsInside: rects.every((r) => r.right <= edge.right + 0.5),
      maxRightOverflow: round(Math.max(...rects.map((r) => r.right - edge.right))),
      ladderLeftDelta: round(l.left - edge.left),
      ladderRightDelta: round(l.right - edge.right),
      ladderMatches: Math.abs(l.left - edge.left) <= 1 && Math.abs(l.right - edge.right) <= 1,
      ladderBelow: l.top + 0.5 >= p.bottom && l.top + 0.5 >= s.bottom,
      distinctTops: tops(list),
      tops: rects.map((r) => Math.round(r.top)),
      noHScroll: bar.scrollWidth <= bar.clientWidth + 0.5,
      scrollWidth: bar.scrollWidth,
      clientWidth: bar.clientWidth,
      parent: ladder.parentElement.tagName.toLowerCase() + "." + [...ladder.parentElement.classList].join("."),
      display: getComputedStyle(ladder).display,
      ladderWidth: round(l.width),
      ladderHeight: round(l.height),
    };
  }
  function collapsed(bar, ladder, plain, summary) {
    const p = plain.getBoundingClientRect();
    const s = summary.getBoundingClientRect();
    const l = ladder.getBoundingClientRect();
    const edge = edges(bar);
    const row = Math.max(p.bottom, s.bottom) - Math.min(p.top, s.top);
    const display = getComputedStyle(ladder).display;
    return {
      sameTop: Math.abs(Math.round(p.top) - Math.round(s.top)) <= 1,
      plainTop: Math.round(p.top),
      summaryTop: Math.round(s.top),
      noBox: display === "none" || (l.width === 0 && l.height === 0),
      display,
      ladderWidth: round(l.width),
      ladderHeight: round(l.height),
      oneLine: Math.abs(edge.rect.height - (row + edge.pad + edge.border)) <= 0.5,
      barHeight: round(edge.rect.height),
      rowPlusPad: round(row + edge.pad + edge.border),
    };
  }
  function visibleTops(ladder) {
    const box = ladder.getBoundingClientRect();
    const seen = [];
    for (const el of nodes()) {
      const r = el.getBoundingClientRect();
      const hits = r.width > 0 && r.height > 0
        && r.right > box.left && r.left < box.right
        && r.bottom > box.top && r.top < box.bottom;
      if (hits) seen.push(Math.round(r.top));
    }
    return new Set(seen).size;
  }
  const short = { behavior: "walk", primitive: null, animation: "idle", state: null, facing: 1, asking: false, happened: null, wake_ms: 12000 };
  const long = { behavior: "play-with-cursor", primitive: "Walk", animation: "idle-breathe", state: "Climbing", facing: -1, asking: false, happened: "spoken to", wake_ms: 720000 };
  const over = { behavior: "x".repeat(80), primitive: null, animation: null, state: null, facing: 1, asking: false, happened: null, wake_ms: 12000 };
  const cap = { behavior: "m".repeat(24), primitive: "m".repeat(24), animation: "m".repeat(24), state: "m".repeat(24), facing: 1, asking: false, happened: "m".repeat(24), wake_ms: 7200000 };
  const wandering = { behavior: "wander_toward_cursor_for_a_very_long_time", primitive: null, animation: "idle", state: null, facing: 1, asking: false, happened: null, wake_ms: 12000 };
  async function drive() {
    console.log("__PROBE_READY__");
    const { applyChatUiClass } = await import("./chat-ui-class.js");
    await ready("chat-status");
    const bar = document.querySelector("footer.bar");
    const details = document.querySelector("details.advanced");
    const ladder = document.querySelector(".ladder");
    const plain = document.getElementById("s-plain");
    const summary = details.querySelector("summary");
    const composer = document.getElementById("composer");
    const log = document.getElementById("log");
    const reports = { childIds: [...ladder.children].map((el) => el.id) };
    for (const theme of THEMES) {
      applyChatUiClass(document.documentElement, theme);
      document.documentElement.style.height = "";
      document.body.style.height = "";
      document.body.style.width = "420px";
      push(details, short, false);
      const closed = collapsed(bar, ladder, plain, summary);
      closed.open = details.open;
      push(details, short, true);
      const opened = layout(bar, ladder, plain, summary);
      document.body.style.width = "320px";
      push(details, long, true);
      const wide = layout(bar, ladder, plain, summary);
      const director = document.getElementById("s-director");
      const happened = document.getElementById("s-happened");
      wide.directorInside = contained(director, bar, ladder);
      wide.happenedInside = contained(happened, bar, ladder);
      push(details, over, true);
      const behavior = document.getElementById("s-behavior");
      const huge = layout(bar, ladder, plain, summary);
      const behaviorRect = behavior.getBoundingClientRect();
      huge.ellipsis = behavior.scrollWidth > behavior.clientWidth;
      huge.behaviorScroll = behavior.scrollWidth;
      huge.behaviorClient = behavior.clientWidth;
      huge.behaviorInside = behaviorRect.right <= edges(bar).right + 0.5;
      push(details, cap, true);
      const capped = {
        cells: nodes().map((el) => el.textContent),
        capped: ladder.scrollHeight > ladder.clientHeight,
        scrollHeight: ladder.scrollHeight,
        clientHeight: ladder.clientHeight,
        visibleTops: visibleTops(ladder),
        distinctTops: tops(nodes()),
      };
      push(details, wandering, true);
      const lineCount = new Set([...plain.getClientRects()].map((r) => Math.round(r.top))).size;
      const prose = {
        text: plain.textContent,
        lines: lineCount,
        ellipsis: plain.scrollWidth > plain.clientWidth,
        scrollWidth: plain.scrollWidth,
        clientWidth: plain.clientWidth,
      };
      document.documentElement.style.height = "320px";
      document.body.style.height = "320px";
      document.body.style.width = "320px";
      push(details, short, false);
      const shut = { composer: box(composer), log: box(log), bar: box(bar) };
      details.open = true;
      const grew = { composer: box(composer), log: box(log), bar: box(bar) };
      const logDrop = shut.log.height - grew.log.height;
      const footerGrowth = grew.bar.height - shut.bar.height;
      reports[theme] = {
        collapsed: closed,
        short: opened,
        long: wide,
        oversized: huge,
        cap: capped,
        plain: prose,
        column: {
          composerLeftDelta: round(grew.composer.left - shut.composer.left),
          composerWidthDelta: round(grew.composer.width - shut.composer.width),
          logDrop: round(logDrop),
          footerGrowth: round(footerGrowth),
          composerStable: Math.abs(grew.composer.left - shut.composer.left) <= 0.5
            && Math.abs(grew.composer.width - shut.composer.width) <= 0.5,
          logTracks: Math.abs(logDrop - footerGrowth) <= 1,
        },
      };
    }
    console.log("__PROBE_RESULT__" + JSON.stringify(reports));
  }
  window.addEventListener("load", drive);
</script>`;

  const html = readFileSync(join(SRC, "chat.html"), "utf8").replace(
    "<head>",
    `<head>${stub}`,
  );
  return probePage(html, { args: ["--window-size=420,560"] });
}

function why(theme, name, scene) {
  return `${theme} ${name} ${JSON.stringify(scene)}`;
}

test(
  "the open ladder is its own full-width wrapping row",
  { skip: chromeSkip, timeout: 90000 },
  async () => {
    const reports = await measure();
    assert.deepEqual(reports.childIds, CELL_IDS);
    for (const theme of THEMES) {
      const scene = reports[theme];
      assert.equal(scene.collapsed.sameTop, true, why(theme, "collapsed", scene.collapsed));
      assert.equal(scene.collapsed.noBox, true, why(theme, "collapsed", scene.collapsed));
      assert.equal(scene.collapsed.oneLine, true, why(theme, "collapsed", scene.collapsed));

      assert.deepEqual(scene.short.cells, SHORT_CELLS, why(theme, "short", scene.short));
      assert.equal(scene.short.padLeft, 16, why(theme, "short", scene.short));
      assert.equal(scene.short.padRight, 16, why(theme, "short", scene.short));
      assert.equal(scene.short.cellsInside, true, why(theme, "short", scene.short));
      assert.equal(scene.short.ladderMatches, true, why(theme, "short", scene.short));
      assert.equal(scene.short.ladderBelow, true, why(theme, "short", scene.short));
      assert.equal(scene.short.distinctTops, 1, why(theme, "short", scene.short));
      assert.equal(scene.short.noHScroll, true, why(theme, "short", scene.short));
      assert.equal(scene.short.parent, "footer.bar", why(theme, "short", scene.short));

      assert.deepEqual(scene.long.cells, LONG_CELLS, why(theme, "long", scene.long));
      assert.equal(scene.long.distinctTops, 2, why(theme, "long", scene.long));
      assert.equal(scene.long.directorInside, true, why(theme, "long", scene.long));
      assert.equal(scene.long.happenedInside, true, why(theme, "long", scene.long));
      assert.equal(scene.long.cellsInside, true, why(theme, "long", scene.long));
      assert.equal(scene.long.noHScroll, true, why(theme, "long", scene.long));

      assert.deepEqual(scene.oversized.cells, OVER_CELLS, why(theme, "oversized", scene.oversized));
      assert.equal(scene.oversized.behaviorInside, true, why(theme, "oversized", scene.oversized));
      assert.equal(scene.oversized.ellipsis, true, why(theme, "oversized", scene.oversized));
      // The 80-x cell clamps to the row and ellipsizes. Shrink is 0, so the
      // other six wrap under it.
      assert.equal(scene.oversized.distinctTops, 2, why(theme, "oversized", scene.oversized));

      assert.deepEqual(scene.cap.cells, CAP_CELLS, why(theme, "cap", scene.cap));
      assert.equal(scene.cap.capped, true, why(theme, "cap", scene.cap));
      assert.equal(scene.cap.visibleTops, 2, why(theme, "cap", scene.cap));

      assert.equal(scene.plain.text, PLAIN_LINE, why(theme, "plain", scene.plain));
      assert.equal(scene.plain.lines, 1, why(theme, "plain", scene.plain));
      assert.equal(scene.plain.ellipsis, true, why(theme, "plain", scene.plain));

      assert.equal(scene.column.composerStable, true, why(theme, "column", scene.column));
      assert.equal(scene.column.logTracks, true, why(theme, "column", scene.column));
    }
  },
);
