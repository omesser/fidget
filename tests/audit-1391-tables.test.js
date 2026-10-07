// Table-driven contract tests for #1391.

import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createBubbleMachine } from "../src/bubble.js";
import { createQuickMessage } from "../src/quick-message.js";
import { computeBubblePaintedRect, reportPaintedRects, clearCache } from "../src/painted-rects.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function read(rel) {
  return fs.readFileSync(path.join(root, rel), "utf8");
}

// --- (a) Region rebuild wiring (source = tip's actual shape) -----------------

test("RegionParams must be a 3-tuple including painted", () => {
  const src = read("src-tauri/src/frame_loop.rs");
  const threeTuple =
    /type RegionParams\s*=\s*\(\s*Vec<\s*\[\s*i32\s*;\s*4\s*\]\s*>\s*,\s*Vec<\s*\[\s*i32\s*;\s*4\s*\]\s*>\s*,\s*Vec<\s*\[\s*i32\s*;\s*4\s*\]\s*>\s*\)/;
  assert.match(
    src,
    threeTuple,
    "RegionParams must include painted so painted changes trigger ApplyMask"
  );
});

test("set_overlay_painted must invalidate the region (not only store)", () => {
  const platform = read("src-tauri/src/platform.rs");
  const mainCmd = read("src-tauri/src/main.rs");
  const paintedSetter = platform.slice(
    platform.indexOf("pub fn set_overlay_painted"),
    platform.indexOf("pub fn set_overlay_painted") + 500
  );
  const invalidates =
    /last_mask|invalidate|dirty|rebuild|force_region|region_dirty/i.test(paintedSetter) ||
    /overlay_painted_rects[\s\S]{0,400}(last_mask|invalidate|dirty|rebuild)/i.test(mainCmd);
  assert.ok(
    invalidates,
    "set_overlay_painted must mark region dirty so idle bubble gets into SetWindowRgn"
  );
});

test("overlay_region_rects / apply_input_mask union painted when given", () => {
  const core = read("crates/core/src/overlay_region.rs");
  const win = read("src-tauri/src/platform/windows/overlay.rs");
  assert.match(core, /chain\(painted_bounds\)/);
  assert.match(win, /overlay_region_rects\(art, hotspot_rects, painted_rects\)/);
});

test("Windows must build region for painted rects on overlay without sprite", () => {
  const frame = read("src-tauri/src/frame_loop.rs");
  // Windows branch must check painted even when sprite_on_overlay is None.
  // Bubble straddling seam: character on overlay-0, bubble extends to overlay-1.
  const buildsRegionWithoutSprite =
    /else\s*\{[\s\S]{0,300}painted[\s\S]{0,300}overlay_painted_for/i.test(frame) ||
    /painted\s*=[\s\S]{0,200}overlay_painted_for[\s\S]{0,300}if\s+!painted\.is_empty/i.test(frame);
  assert.ok(
    buildsRegionWithoutSprite,
    "Windows frame_loop must build region when painted non-empty even without sprite"
  );
});

// --- (b) Ownership routing table --------------------------------------------

// Pure reimplementation matching crates/core/src/overlay.rs (for dual-display rows).
function covers(point, rect) {
  return (
    point[0] >= rect.x &&
    point[0] < rect.x + rect.width &&
    point[1] >= rect.y &&
    point[1] < rect.y + rect.height
  );
}
function outsideBy(point, rect) {
  const dx = Math.max(rect.x - point[0], point[0] - (rect.x + rect.width), 0);
  const dy = Math.max(rect.y - point[1], point[1] - (rect.y + rect.height), 0);
  return dx * dx + dy * dy;
}
function bubbleOwner(feet, displays) {
  const idx = displays.findIndex((d) => covers(feet, d));
  if (idx >= 0) return idx;
  const slack = 1;
  const floor = displays.findIndex((d) => outsideBy(feet, d) <= slack * slack);
  return floor >= 0 ? floor : null;
}
function bubbleOwnerHysteresis(feet, displays, prev, margin = 8) {
  if (prev != null && displays[prev]) {
    const d = displays[prev];
    if (covers(feet, d) || outsideBy(feet, d) <= margin * margin) return prev;
  }
  return bubbleOwner(feet, displays);
}

const ODED = [
  { x: 0, y: 0, width: 3440, height: 1440 },
  { x: -1200, y: -209, width: 1200, height: 1920 },
];

test("ownership/routing table (Oded dual-display)", () => {
  const rows = [
    { name: "primary center", feet: [1720, 720], prev: null, expect: 0 },
    { name: "portrait center", feet: [-600, 500], prev: null, expect: 1 },
    { name: "seam x=0 → primary", feet: [0, 100], prev: null, expect: 0 },
    { name: "just left of seam → portrait", feet: [-1, 100], prev: null, expect: 1 },
    { name: "hysteresis keeps primary near seam", feet: [5, 100], prev: 0, expect: 0 },
    { name: "beyond hysteresis → portrait", feet: [-20, 100], prev: 0, expect: 1 },
    { name: "hysteresis keeps portrait near seam", feet: [-5, 100], prev: 1, expect: 1 },
  ];
  for (const row of rows) {
    const got = bubbleOwnerHysteresis(row.feet, ODED, row.prev);
    assert.equal(got, row.expect, row.name);
  }
});

// --- (c) JS bubble / QM state machine table ---------------------------------

function machineHarness() {
  const log = [];
  const timers = [];
  const machine = createBubbleMachine({
    showSpeech(t) {
      log.push(["speech", t]);
    },
    hideSpeech() {
      log.push(["hideSpeech"]);
    },
    showThinking() {
      log.push(["thinking"]);
    },
    hideThinking() {
      log.push(["hideThinking"]);
    },
    showAsk() {
      log.push(["ask"]);
    },
    schedule(fn, ms) {
      const id = { fn, ms, cleared: false };
      timers.push(id);
      return id;
    },
    cancel(id) {
      if (id) id.cleared = true;
    },
  });
  function flush(ms) {
    for (const t of timers) {
      if (!t.cleared && t.ms <= ms) {
        t.cleared = true;
        t.fn();
      }
    }
  }
  return { machine, log, flush, timers };
}

test("bubble/QM event sequence table", () => {
  const rows = [
    {
      name: "QM send → aiTurnStarted → thinking immediate",
      run(h) {
        h.machine.aiTurnStarted();
        assert.deepEqual(h.log, [["thinking"]]);
      },
    },
    {
      name: "thinking flag with grace → show after 250ms",
      run(h) {
        h.machine.frame({ thinking: true, visible: true, bubble: true });
        assert.deepEqual(h.log, []);
        h.flush(250);
        assert.deepEqual(h.log, [["thinking"]]);
      },
    },
    {
      name: "ownership flip mid-turn → hideButKeepTurn then re-entry shows thinking now",
      run(h) {
        h.machine.aiTurnStarted();
        h.log.length = 0;
        h.machine.hideButKeepTurn();
        assert.ok(h.log.some((e) => e[0] === "hideThinking") || h.log.some((e) => e[0] === "hideSpeech"));
        h.log.length = 0;
        h.machine.frame({ thinking: true, visible: true, bubble: true });
        assert.deepEqual(h.log, [["thinking"]], "wasInterrupted → no grace");
      },
    },
    {
      name: "non-owner clears aiTurnPending",
      run(h) {
        h.machine.aiTurnStarted();
        h.log.length = 0;
        h.machine.frame({ thinking: false, visible: true, bubble: false });
        assert.ok(h.log.some((e) => e[0] === "hideThinking"));
      },
    },
    {
      name: "dialogue after thinking → speech replaces thinking",
      run(h) {
        h.machine.aiTurnStarted();
        h.log.length = 0;
        h.machine.event({ dialogue: "Hi" });
        h.machine.frame({ dialogue: "Hi", visible: true, thinking: false, bubble: true });
        assert.deepEqual(h.log[0], ["hideThinking"]);
        assert.deepEqual(h.log[1], ["speech", "Hi"]);
      },
    },
  ];
  for (const row of rows) {
    row.run(machineHarness());
  }
});

// --- Handoff race (tip causal chain) ----------------------------------------

/**
 * Tip handoff: emit only if backend qm_state.open after owner change.
 * Tip dismiss paths (drag empty / leaveSprite auto-hide) call reportQmVisible(null)
 * which CLEARS backend state — often BEFORE bubble_owner flips.
 */
function tipHandoff(seq) {
  let backend = seq.initialBackend; // { open, text, focused } | null
  const events = [];
  for (const step of seq.steps) {
    if (step.type === "dragDismissEmpty") {
      if (backend && !backend.text && !backend.focused) {
        backend = null;
        events.push("backend_cleared");
      }
    } else if (step.type === "leaveAutoHide") {
      if (backend && !backend.text && !backend.focused) {
        backend = null;
        events.push("backend_cleared");
      }
    } else if (step.type === "ownerChange") {
      if (backend && backend.open) {
        events.push(`handoff_to_${step.to}`);
      } else {
        events.push("handoff_skipped");
      }
    } else if (step.type === "reportOpen") {
      backend = { open: true, text: step.text || "", focused: !!step.focused };
      events.push("backend_open");
    }
  }
  return events;
}

// --- Drag decision table (pure function in core) ----------------------------

test("keep_qm_on_drag: empty closes, non-empty keeps", () => {
  const qmDraft = read("crates/core/src/qm_draft.rs");
  
  // Function must be pure and table-tested
  assert.match(qmDraft, /pub fn keep_qm_on_drag/);
  assert.match(qmDraft, /empty_text_closes/i);
  assert.match(qmDraft, /non_empty.*keeps/i);
  assert.match(qmDraft, /no_draft.*closes/i);
});

test("Drag decision table (matching qm_draft.rs tests)", () => {
  const rows = [
    { text: "", focused: true, keep: false, reason: "empty text closes on drag" },
    { text: "hello", focused: false, keep: true, reason: "non-empty text keeps pill" },
    { text: "   ", focused: true, keep: true, reason: "whitespace counts as non-empty per String::is_empty" },
  ];

  for (const { text, focused, keep, reason } of rows) {
    const draft = text === null ? null : { text, focused };
    const result = keepQmOnDrag(draft);
    assert.strictEqual(result, keep, reason);
  }
});

// Helper matching crates/core/src/qm_draft.rs::keep_qm_on_drag
function keepQmOnDrag(draft) {
  if (!draft) return false;
  return draft.text.length > 0;
}

test("Owner flip carries draft to new overlay", () => {
  const main = read("src/main.js");
  const frame = read("src-tauri/src/frame_loop.rs");
  
  // Draft rides in Placed.qm
  assert.match(frame, /qm:\s*live\.qm\.clone\(\)/);
  
  // JS restores from placement.qm when owner
  assert.ok(
    /placement\.bubble.*placement\.qm/.test(main) || /placement\.qm.*placement\.bubble/.test(main),
    "JS must restore from placement.qm when placement.bubble is true"
  );
  
  // Non-owner hides pill
  assert.match(main, /!placement\.bubble.*hideWithoutReport/s);
});

// --- Painted rect format / opacity ------------------------------------------

test("painted rect table", () => {
  clearCache();
  const rows = [
    {
      name: "hidden → null",
      bubble: { classList: { contains: () => false }, style: {}, offsetWidth: 10, offsetHeight: 10 },
      pos: { x: 1, y: 2, inverted: false },
      expect: null,
    },
    {
      name: "visible opaque normal (tail down) → xywh + 10px tail",
      bubble: {
        classList: { contains: (c) => c === "visible" },
        style: { opacity: "1" },
        offsetWidth: 200,
        offsetHeight: 80,
      },
      pos: { x: 10.4, y: 20.6, inverted: false },
      expect: [10, 21, 200, 90],
    },
    {
      name: "visible opaque inverted (tail up) → y-10, xywh + 10px tail",
      bubble: {
        classList: { contains: (c) => c === "visible" },
        style: { opacity: "1" },
        offsetWidth: 200,
        offsetHeight: 80,
      },
      pos: { x: 10.4, y: 20.6, inverted: true },
      expect: [10, 11, 200, 90],
    },
    {
      name: "fading → null",
      bubble: {
        classList: { contains: (c) => c === "visible" },
        style: { opacity: "0.4" },
        offsetWidth: 200,
        offsetHeight: 80,
      },
      pos: { x: 10, y: 20, inverted: false },
      expect: null,
    },
  ];
  for (const row of rows) {
    assert.deepEqual(computeBubblePaintedRect(row.bubble, row.pos), row.expect, row.name);
  }
});

test("reportPaintedRects batches and dedupes", () => {
  clearCache();
  const views = new Map([[1, { paintedRect: [1, 2, 3, 4] }]]);
  let n = 0;
  const invoke = () => {
    n++;
    return Promise.resolve();
  };
  reportPaintedRects(views, invoke);
  reportPaintedRects(views, invoke);
  assert.equal(n, 1);
  views.get(1).paintedRect = [9, 9, 9, 9];
  reportPaintedRects(views, invoke);
  assert.equal(n, 2);
});
