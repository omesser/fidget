import { test } from "node:test";
import assert from "node:assert/strict";
import { createBubbleMachine } from "../src/bubble.js";

test("aiTurnPending cleared when placement.bubble becomes false", async () => {
  let thinkingShown = false;
  const machine = createBubbleMachine({
    showSpeech: () => {},
    hideSpeech: () => {},
    showThinking: () => {
      thinkingShown = true;
    },
    hideThinking: () => {
      thinkingShown = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.aiTurnStarted();

  await new Promise((resolve) => setTimeout(resolve, 300));

  assert.ok(thinkingShown, "Thinking should show after aiTurnStarted");

  machine.frame({ bubble: false, visible: true });

  await new Promise((resolve) => setTimeout(resolve, 100));

  assert.ok(!thinkingShown, "Thinking should be cleared when bubble ownership lost");

  machine.frame({ bubble: true, visible: true });

  await new Promise((resolve) => setTimeout(resolve, 100));

  assert.ok(!thinkingShown, "Thinking should stay cleared after reacquiring ownership (aiTurnPending was cleared)");
});

test("unconditional aiTurnStarted at QM send", () => {
  let aiTurnStartedCalled = false;
  const mockBubbles = {
    aiTurnStarted: () => {
      aiTurnStartedCalled = true;
    },
  };

  const mockView = {
    latest: { bubble: false },
    bubbles: mockBubbles,
  };

  mockView.bubbles.aiTurnStarted();

  assert.ok(aiTurnStartedCalled, "aiTurnStarted should be called unconditionally, not gated by view.latest?.bubble");
});
