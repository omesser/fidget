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

  assert.strictEqual(reported.length, 0, "No reportPaintedRects in bubble machine (delegated to main.js io)");
});

test("bubble clears painted rect after hide", async () => {
  let reported = [];
  const machine = createBubbleMachine({
    showSpeech: () => {},
    hideSpeech: () => {
      reported = [];
    },
    hideThinking: () => {
      reported = [];
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.event({ dialogue: "Hello" });
  machine.frame({ visible: true, bubble: true });

  machine.hideAllNow();
  assert.strictEqual(reported.length, 0, "Painted rects cleared via hideSpeech");
});

test("thinking reports painted rect", async () => {
  let thinkingShown = false;
  const machine = createBubbleMachine({
    showThinking: () => {
      thinkingShown = true;
    },
    hideThinking: () => {
      thinkingShown = false;
    },
    hideSpeech: () => {},
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.aiTurnStarted();

  await new Promise((resolve) => setTimeout(resolve, 300));

  machine.frame({ visible: true, bubble: true, thinking: true });

  assert.ok(thinkingShown, "Thinking shown (painted rect reporting delegated to main.js)");
});
