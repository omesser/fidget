import { test } from "node:test";
import assert from "node:assert/strict";
import { createBubbleMachine } from "../src/bubble.js";

test("aiTurnStarted shows thinking immediately", async () => {
  let thinkingShown = false;
  const machine = createBubbleMachine({
    showSpeech() {},
    hideSpeech() {},
    showThinking() {
      thinkingShown = true;
    },
    hideThinking() {
      thinkingShown = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.aiTurnStarted();

  await new Promise((resolve) => setTimeout(resolve, 250));

  assert.equal(thinkingShown, true, "thinking should show after aiTurnStarted grace period");
});

test("placement with thinking=true shows thinking indicator", async () => {
  let thinkingShown = false;
  const machine = createBubbleMachine({
    showSpeech() {},
    hideSpeech() {},
    showThinking() {
      thinkingShown = true;
    },
    hideThinking() {
      thinkingShown = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.frame({ thinking: true, visible: true });

  await new Promise((resolve) => setTimeout(resolve, 250));

  assert.equal(thinkingShown, true, "thinking should show from placement");
});

test("dialogue clears thinking and shows speech", async () => {
  let thinkingShown = false;
  let speechShown = null;
  const machine = createBubbleMachine({
    showSpeech(text) {
      speechShown = text;
    },
    hideSpeech() {
      speechShown = null;
    },
    showThinking() {
      thinkingShown = true;
    },
    hideThinking() {
      thinkingShown = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.aiTurnStarted();
  await new Promise((resolve) => setTimeout(resolve, 250));
  assert.equal(thinkingShown, true, "thinking should show first");

  machine.event({ dialogue: "Hello!" });
  machine.frame({ dialogue: "Hello!", thinking: false, visible: true });

  assert.equal(speechShown, "Hello!", "speech should show with dialogue");
  assert.equal(thinkingShown, false, "thinking should hide when speech shows");
});

test("asking placement shows speech pointing to Chat", async () => {
  let askShown = false;
  const machine = createBubbleMachine({
    showSpeech() {},
    hideSpeech() {},
    showAsk() {
      askShown = true;
    },
    showThinking() {},
    hideThinking() {},
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.event({ asking: true });
  machine.frame({ asking: true, visible: true });

  assert.equal(askShown, true, "ask pointer should show from placement");
});

test("invisible sprite suppresses thinking", async () => {
  let thinkingShown = false;
  const machine = createBubbleMachine({
    showSpeech() {},
    hideSpeech() {},
    showThinking() {
      thinkingShown = true;
    },
    hideThinking() {
      thinkingShown = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.frame({ thinking: true, visible: false });
  await new Promise((resolve) => setTimeout(resolve, 250));

  assert.equal(thinkingShown, false, "thinking should not show when sprite is invisible");
});

test("aiTurnAbandoned clears thinking", async () => {
  let thinkingShown = false;
  const machine = createBubbleMachine({
    showSpeech() {},
    hideSpeech() {},
    showThinking() {
      thinkingShown = true;
    },
    hideThinking() {
      thinkingShown = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.aiTurnStarted();
  await new Promise((resolve) => setTimeout(resolve, 250));
  assert.equal(thinkingShown, true);

  machine.aiTurnAbandoned();

  assert.equal(thinkingShown, false, "thinking should clear on aiTurnAbandoned");
});

test("hideButKeepTurn preserves aiTurnPending for ownership changes", async () => {
  let thinkingShown = false;
  let speechShown = null;
  const machine = createBubbleMachine({
    showSpeech(text) {
      speechShown = text;
    },
    hideSpeech() {
      speechShown = null;
    },
    showThinking() {
      thinkingShown = true;
    },
    hideThinking() {
      thinkingShown = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  // User submits QM message, AI turn starts
  machine.aiTurnStarted();
  await new Promise((resolve) => setTimeout(resolve, 250));
  assert.equal(thinkingShown, true, "thinking should show after aiTurnStarted");

  // Character crosses monitor seam, ownership changes, old owner hides bubbles
  machine.hideButKeepTurn();
  assert.equal(thinkingShown, false, "thinking should hide immediately");
  assert.equal(speechShown, null, "speech should hide immediately");

  // Reply arrives on new owner (which has same aiTurnPending state)
  machine.event({ dialogue: "Hello from QM!" });
  machine.frame({ dialogue: "Hello from QM!", thinking: false, visible: true });

  assert.equal(
    speechShown,
    "Hello from QM!",
    "speech should show despite ownership change mid-turn",
  );
  assert.equal(thinkingShown, false, "thinking should stay hidden when speech shows");
});
