import { test } from "node:test";
import assert from "node:assert/strict";
import { createBubbleMachine } from "../src/bubble.js";

test("bubble reports painted rect after showSpeech", async () => {
  let reported = [];
  const machine = createBubbleMachine({
    showSpeech: () => {},
    hideSpeech: () => {},
    showThinking: () => {},
    hideThinking: () => {},
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
    reportPaintedRects: (rects) => {
      reported = rects;
    },
  });

  machine.event({ dialogue: "Hello" });
  machine.frame({ visible: true, bubble: true });

  assert.ok(reported.length > 0, "Painted rects should include bubble after showSpeech");
  const [rect] = reported;
  assert.ok(rect.x !== undefined && rect.y !== undefined, "Rect should have position");
  assert.ok(rect.width > 0 && rect.height > 0, "Rect should have size");
});

test("bubble clears painted rect after hide", async () => {
  let reported = [];
  const machine = createBubbleMachine({
    showSpeech: () => {},
    hideSpeech: () => {},
    hideThinking: () => {},
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
    reportPaintedRects: (rects) => {
      reported = rects;
    },
  });

  machine.event({ dialogue: "Hello" });
  machine.frame({ visible: true, bubble: true });
  assert.ok(reported.length > 0, "Bubble should be painted");

  machine.hideAllNow();
  assert.strictEqual(reported.length, 0, "Painted rects should be cleared after hide");
});

test("thinking reports painted rect", async () => {
  let reported = [];
  const machine = createBubbleMachine({
    showThinking: () => {},
    hideThinking: () => {},
    hideSpeech: () => {},
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
    reportPaintedRects: (rects) => {
      reported = rects;
    },
  });

  machine.aiTurnStarted();

  await new Promise((resolve) => setTimeout(resolve, 300));

  machine.frame({ visible: true, bubble: true, thinking: true });

  assert.ok(reported.length > 0, "Painted rects should include thinking bubble");
});
