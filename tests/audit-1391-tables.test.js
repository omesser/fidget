// Table-driven audit for #1391 tip 89fb28d7.
// Rows marked RED must fail at tip; GREEN rows lock correct pure contracts.

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

test("RED: RegionParams must be a 3-tuple including painted", () => {
  const src = read("src-tauri/src/frame_loop.rs");
  // Tip today:
  //   type RegionParams = (Vec<[i32; 4]>, Vec<[i32; 4]>);
  // Required:
  //   type RegionParams = (Vec<[i32; 4]>, Vec<[i32; 4]>, Vec<[i32; 4]>);
  const threeTuple =
    /type RegionParams\s*=\s*\(\s*Vec<\s*\[\s*i32\s*;\s*4\s*\]\s*>\s*,\s*Vec<\s*\[\s*i32\s*;\s*4\s*\]\s*>\s*,\s*Vec<\s*\[\s*i32\s*;\s*4\s*\]\s*>\s*\)/;
  assert.match(
    src,
    threeTuple,
    "RED tip 89fb28d7: RegionParams is (art, hotspots) only — painted changes never trigger ApplyMask"
  );
});

test("RED: set_overlay_painted must invalidate the region (not only store)", () => {
  const platform = read("src-tauri/src/platform.rs");
  const mainCmd = read("src-tauri/src/main.rs");
  const paintedSetter = platform.slice(
    platform.indexOf("pub fn set_overlay_painted"),
    platform.indexOf("pub fn set_overlay_painted") + 500
  );
  // Tip only retain+extend. Required: mark dirty / clear last_mask / force rebuild.
  const invalidates =
    /last_mask|invalidate|dirty|rebuild|force_region|region_dirty/i.test(paintedSetter) ||
    /overlay_painted_rects[\s\S]{0,400}(last_mask|invalidate|dirty|rebuild)/i.test(mainCmd);
  assert.ok(
    invalidates,
    "RED tip 89fb28d7: set_overlay_painted only stores rects; idle bubble never gets into SetWindowRgn"
  );
});

test("GREEN: overlay_region_rects / apply_input_mask already union painted when given", () => {
  const core = read("crates/core/src/overlay_region.rs");
  const win = read("src-tauri/src/platform/windows/overlay.rs");
  assert.match(core, /chain\(painted_bounds\)/);
  assert.match(win, /overlay_region_rects\(art, hotspot_rects, painted_rects\)/);
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

test("RED: empty pill drag-clear must not skip handoff", () => {
  const frame = read("src-tauri/src/frame_loop.rs");
  const main = read("src-tauri/src/main.rs");
  // Required: latch qm_was_open across drag/dismiss, use plan_qm_handoff
  const hasLatch = /qm_was_open/.test(main);
  const usesPlanQmHandoff = /plan_qm_handoff/.test(frame);
  assert.ok(
    hasLatch && usesPlanQmHandoff,
    "RED tip handoff race: must latch qm_was_open and use plan_qm_handoff to survive drag-clear"
  );
});

test("GREEN: focused draft survives drag and handoffs", () => {
  const events = tipHandoff({
    initialBackend: { open: true, text: "hello", focused: true },
    steps: [
      { type: "dragDismissEmpty" },
      { type: "ownerChange", to: 1 },
    ],
  });
  assert.deepEqual(events, ["handoff_to_1"]);
});

test("RED: old owner must be told to dismiss on handoff", () => {
  const main = read("src/main.js");
  const frame = read("src-tauri/src/frame_loop.rs");
  // Tip emit_to(new_owner only). Required: also dismiss old owner (event or flag).
  const emitsOnlyNew = /emit_to\(\s*new_owner_label\s*,\s*"qm-handoff"/.test(frame);
  const oldDismiss =
    /qm-handoff[\s\S]{0,800}dismiss|from_overlay[\s\S]{0,400}dismiss|handoff.*old/i.test(main);
  assert.ok(
    !emitsOnlyNew || oldDismiss,
    "RED tip: qm-handoff goes only to new owner; old overlay keeps/orphans its pill DOM"
  );
});

// --- Painted rect format / opacity ------------------------------------------

test("painted rect table", () => {
  clearCache();
  const rows = [
    {
      name: "hidden → null",
      bubble: { classList: { contains: () => false }, style: {}, offsetWidth: 10, offsetHeight: 10 },
      pos: { x: 1, y: 2 },
      expect: null,
    },
    {
      name: "visible opaque → xywh",
      bubble: {
        classList: { contains: (c) => c === "visible" },
        style: { opacity: "1" },
        offsetWidth: 200,
        offsetHeight: 80,
      },
      pos: { x: 10.4, y: 20.6 },
      expect: [10, 21, 200, 80],
    },
    {
      name: "fading → null",
      bubble: {
        classList: { contains: (c) => c === "visible" },
        style: { opacity: "0.4" },
        offsetWidth: 200,
        offsetHeight: 80,
      },
      pos: { x: 10, y: 20 },
      expect: null,
    },
  ];
  for (const row of rows) {
    assert.deepEqual(computeBubblePaintedRect(row.bubble, row.pos), row.expect, row.name);
  }
});

test("GREEN: reportPaintedRects batches and dedupes", () => {
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
