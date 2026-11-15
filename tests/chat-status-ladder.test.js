// The open status is labelled fields, two to a row with Source across both.
// Collapsed, the sentence and Advanced share one line. Edges allow ±0.5px
// because a 10.5px font times 1.45 is 15.225px.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { chromeSkip, probePage } from "./chrome-cdp.js";
import { pageWait } from "./page-wait.js";

const SRC = fileURLToPath(new URL("../src/", import.meta.url));
const THEMES = ["minimal", "terminal", "glass"];
// Each field's name, in order, and the cell ids its value holds.
const FIELDS = [
  ["Source", ["s-source"]],
  ["Behavior", ["s-behavior"]],
  ["Animation", ["s-animation"]],
  ["Primitive", ["s-primitive"]],
  ["State", ["s-state", "s-facing"]],
  ["Director", ["s-director"]],
  ["Happened", ["s-happened"]],
];
const PLAIN_LINE = "Wander toward cursor for a very long time (idle) · next thought in 12s";
// Every Animation source key, and the label its field must show in full.
const SOURCE_LABELS = {
  idle_loop: "Idle Loop",
  engine: "Engine",
  local_director: "Local Director",
  ai_requested: "AI Requested",
  ai_mcp: "AI MCP",
  user_invoked: "User Invoked",
  ai_feedback: "AI Feedback",
  onboarding: "Onboarding",
};

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
  const THEMES = ${JSON.stringify(THEMES)};
  const SOURCE_KEYS = ${JSON.stringify(Object.keys(SOURCE_LABELS))};
  const round = (n) => Math.round(n * 100) / 100;
  function box(el) {
    const r = el.getBoundingClientRect();
    return { top: r.top, right: r.right, bottom: r.bottom, left: r.left, width: r.width, height: r.height };
  }
  function edges(bar) {
    const r = bar.getBoundingClientRect();
    const s = getComputedStyle(bar);
    return {
      left: r.left + parseFloat(s.paddingLeft),
      right: r.right - parseFloat(s.paddingRight),
      top: r.top + parseFloat(s.borderTopWidth) + parseFloat(s.paddingTop),
      bottom: r.bottom - parseFloat(s.borderBottomWidth) - parseFloat(s.paddingBottom),
      rect: r,
      pad: parseFloat(s.paddingTop) + parseFloat(s.paddingBottom),
      border: parseFloat(s.borderTopWidth) + parseFloat(s.borderBottomWidth),
    };
  }
  function push(details, payload, open) {
    handlers["chat-status"]({ payload });
    details.open = open;
  }
  function inside(r, edge) {
    return r.width > 0 && r.height > 0
      && r.left >= edge.left - 0.5 && r.right <= edge.right + 0.5
      && r.top >= edge.top - 0.5 && r.bottom <= edge.bottom + 0.5;
  }
  // Every field: its name, its value's text, whether both sit inside the bar,
  // and whether the value reads whole or ellipsizes inside its own field.
  function opened(bar, ladder, plain, summary) {
    const edge = edges(bar);
    const fields = [...ladder.querySelectorAll(".field")].map((field) => {
      const dt = field.querySelector("dt");
      const dd = field.querySelector("dd");
      return {
        name: dt.textContent,
        ids: [...dd.querySelectorAll("[id]")].map((el) => el.id).concat(dd.id ? [dd.id] : []),
        value: dd.textContent,
        nameInside: inside(dt.getBoundingClientRect(), edge),
        valueInside: inside(dd.getBoundingClientRect(), edge),
        whole: dd.scrollWidth <= dd.clientWidth + 0.5,
        nameLeft: Math.round(dt.getBoundingClientRect().left),
        valueLeft: Math.round(dd.getBoundingClientRect().left),
        top: Math.round(field.getBoundingClientRect().top),
        width: round(field.getBoundingClientRect().width),
        nameColor: getComputedStyle(dt).color,
        valueColor: getComputedStyle(dd).color,
      };
    });
    const l = ladder.getBoundingClientRect();
    const p = plain.getBoundingClientRect();
    const s = summary.getBoundingClientRect();
    return {
      fields,
      rows: new Set(fields.map((f) => f.top)).size,
      below: l.top + 0.5 >= p.bottom && l.top + 0.5 >= s.bottom,
      noHScroll: bar.scrollWidth <= bar.clientWidth + 0.5,
      ladderWidth: round(l.width),
      barWidth: round(edge.right - edge.left),
    };
  }
  function collapsed(bar, ladder, plain, summary) {
    const p = plain.getBoundingClientRect();
    const s = summary.getBoundingClientRect();
    const l = ladder.getBoundingClientRect();
    const edge = edges(bar);
    const row = Math.max(p.bottom, s.bottom) - Math.min(p.top, s.top);
    return {
      sameTop: Math.abs(Math.round(p.top) - Math.round(s.top)) <= 1,
      noBox: getComputedStyle(ladder).display === "none" || (l.width === 0 && l.height === 0),
      oneLine: Math.abs(edge.rect.height - (row + edge.pad + edge.border)) <= 0.5,
      barHeight: round(edge.rect.height),
    };
  }
  const short = { behavior: "walk", primitive: null, animation: "idle", state: null, facing: 1, asking: false, happened: null, wake_ms: 12000 };
  const long = { behavior: "play-with-cursor", primitive: "Walk", animation: "idle-breathe", state: "Climbing", facing: -1, asking: false, happened: "spoken to", wake_ms: 720000 };
  const over = { behavior: "x".repeat(80), primitive: null, animation: null, state: null, facing: 1, asking: false, happened: null, wake_ms: 12000 };
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
    const reports = {};
    for (const theme of THEMES) {
      applyChatUiClass(document.documentElement, theme);
      document.documentElement.style.height = "";
      document.body.style.height = "";
      const scenes = {};
      for (const width of [420, 320]) {
        document.body.style.width = width + "px";
        push(details, short, false);
        scenes["collapsed" + width] = collapsed(bar, ladder, plain, summary);
        for (const [name, payload] of [["short", short], ["long", long], ["over", over]]) {
          push(details, payload, true);
          scenes[name + width] = opened(bar, ladder, plain, summary);
        }
        scenes["sources" + width] = SOURCE_KEYS.map((key) => {
          handlers["chat-status"]({ payload: { ...long, source: key } });
          const shown = opened(bar, ladder, plain, summary);
          return { key, field: shown.fields[0], noHScroll: shown.noHScroll };
        });
      }
      push(details, wandering, false);
      const prose = {
        text: plain.textContent,
        lines: new Set([...plain.getClientRects()].map((r) => Math.round(r.top))).size,
        ellipsis: plain.scrollWidth > plain.clientWidth,
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
        ...scenes,
        plain: prose,
        column: {
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
  "the open status names every value and keeps it inside the bar",
  { skip: chromeSkip, timeout: 90000 },
  async () => {
    const reports = await measure();
    for (const theme of THEMES) {
      const scene = reports[theme];
      for (const width of [420, 320]) {
        const closed = scene["collapsed" + width];
        assert.equal(closed.sameTop, true, why(theme, `collapsed ${width}`, closed));
        assert.equal(closed.noBox, true, why(theme, `collapsed ${width}`, closed));
        assert.equal(closed.oneLine, true, why(theme, `collapsed ${width}`, closed));

        for (const name of ["short", "long", "over"]) {
          const open = scene[name + width];
          const label = why(theme, `${name} ${width}`, open);
          assert.deepEqual(
            open.fields.map((f) => [f.name, f.ids]),
            FIELDS,
            label,
          );
          // Source across both columns, then three rows of two.
          assert.equal(open.rows, 4, label);
          assert.equal(open.fields[0].width, open.barWidth, label);
          assert.equal(open.below, true, label);
          assert.equal(open.noHScroll, true, label);
          for (const field of open.fields) {
            assert.equal(field.nameInside, true, label);
            assert.equal(field.valueInside, true, label);
            assert.notEqual(field.nameColor, field.valueColor, label);
          }
          // Names line up in two columns, so each value sits under its own.
          assert.equal(new Set(open.fields.slice(1).map((f) => f.nameLeft)).size, 2, label);
          assert.equal(new Set(open.fields.map((f) => f.valueLeft)).size, 2, label);
        }

        const short = scene["short" + width];
        assert.deepEqual(
          short.fields.map((f) => f.value),
          ["—", "walk", "idle", "—", "— →", "wake 12s", "—"],
          why(theme, `short ${width}`, short),
        );
        assert.equal(short.fields.every((f) => f.whole), true, why(theme, `short ${width}`, short));

        const over = scene["over" + width];
        assert.equal(over.fields[1].value, "x".repeat(80), why(theme, `over ${width}`, over));
        assert.equal(over.fields[1].whole, false, why(theme, `over ${width}`, over));

        for (const shown of scene["sources" + width]) {
          const label = `${theme} ${width} source ${JSON.stringify(shown)}`;
          assert.equal(shown.field.value, SOURCE_LABELS[shown.key], label);
          assert.equal(shown.field.whole, true, label);
          assert.equal(shown.field.valueInside, true, label);
          assert.equal(shown.noHScroll, true, label);
        }
      }

      assert.equal(scene.plain.text, PLAIN_LINE, why(theme, "plain", scene.plain));
      assert.equal(scene.plain.lines, 1, why(theme, "plain", scene.plain));
      assert.equal(scene.plain.ellipsis, true, why(theme, "plain", scene.plain));

      assert.equal(scene.column.composerStable, true, why(theme, "column", scene.column));
      assert.equal(scene.column.logTracks, true, why(theme, "column", scene.column));
    }
  },
);
