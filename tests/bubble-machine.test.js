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
    instance: "test-instance-1",
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

  // Setup global map in mock window
  global.window = { aiTurnsPending: new Map() };

  // User submits QM message, AI turn starts
  machine.aiTurnStarted();
  await new Promise((resolve) => setTimeout(resolve, 250));
  assert.equal(thinkingShown, true, "thinking should show after aiTurnStarted");
  assert.equal(global.window.aiTurnsPending.get("test-instance-1"), true, "global flag set");

  // Character crosses monitor seam, ownership changes, old owner hides bubbles
  machine.hideButKeepTurn();
  assert.equal(thinkingShown, false, "thinking should hide immediately");
  assert.equal(speechShown, null, "speech should hide immediately");
  assert.equal(
    global.window.aiTurnsPending.get("test-instance-1"),
    true,
    "global flag preserved during ownership change",
  );

  // Reply arrives on new owner (which has same aiTurnPending state)
  machine.event({ dialogue: "Hello from QM!" });
  machine.frame({ dialogue: "Hello from QM!", thinking: false, visible: true });

  assert.equal(
    speechShown,
    "Hello from QM!",
    "speech should show despite ownership change mid-turn",
  );
  assert.equal(thinkingShown, false, "thinking should stay hidden when speech shows");
  assert.equal(
    global.window.aiTurnsPending.has("test-instance-1"),
    false,
    "global flag cleared after dialogue",
  );

  delete global.window;
});

test("global AI turn state survives ownership flip", async () => {
  // Simulate two overlays with bubble machines for the same instance
  global.window = { aiTurnsPending: new Map() };

  let thinking1 = false;
  let speech1 = null;
  const overlay1 = createBubbleMachine({
    instance: "test-instance-2",
    showSpeech(text) {
      speech1 = text;
    },
    hideSpeech() {
      speech1 = null;
    },
    showThinking() {
      thinking1 = true;
    },
    hideThinking() {
      thinking1 = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  let thinking2 = false;
  let speech2 = null;
  const overlay2 = createBubbleMachine({
    instance: "test-instance-2",
    showSpeech(text) {
      speech2 = text;
    },
    hideSpeech() {
      speech2 = null;
    },
    showThinking() {
      thinking2 = true;
    },
    hideThinking() {
      thinking2 = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  // QM send happens on overlay1
  overlay1.aiTurnStarted();
  await new Promise((resolve) => setTimeout(resolve, 250));
  assert.equal(thinking1, true, "overlay1 shows thinking");
  assert.equal(thinking2, false, "overlay2 not started yet");

  // Character crosses seam, overlay1 loses ownership
  overlay1.hideButKeepTurn();
  assert.equal(thinking1, false, "overlay1 thinking hidden");

  // Overlay2 receives next frame with thinking flag from backend + global flag
  overlay2.frame({ thinking: false, visible: true });
  await new Promise((resolve) => setTimeout(resolve, 250));
  assert.equal(thinking2, true, "overlay2 shows thinking from global flag");

  // Reply arrives on overlay2
  overlay2.event({ dialogue: "Cross-seam reply" });
  overlay2.frame({ dialogue: "Cross-seam reply", thinking: false, visible: true });

  assert.equal(speech2, "Cross-seam reply", "overlay2 shows speech");
  assert.equal(
    global.window.aiTurnsPending.has("test-instance-2"),
    false,
    "global flag cleared",
  );

  delete global.window;
});
