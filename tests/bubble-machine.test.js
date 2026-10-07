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

  // User submits QM message, AI turn starts (simulated via backend's thinking flag)
  machine.frame({ thinking: true, visible: true });
  await new Promise((resolve) => setTimeout(resolve, 250));
  assert.equal(thinkingShown, true, "thinking should show from backend flag");

  // Character crosses monitor seam, ownership changes, old owner hides bubbles
  machine.hideButKeepTurn();
  assert.equal(thinkingShown, false, "thinking should hide immediately");
  assert.equal(speechShown, null, "speech should hide immediately");

  // New owner receives frame with thinking flag still true (backend tracks it)
  machine.frame({ thinking: true, visible: true });
  await new Promise((resolve) => setTimeout(resolve, 250));
  assert.equal(thinkingShown, true, "thinking reappears on new owner from backend flag");

  // Reply arrives
  machine.event({ dialogue: "Hello from QM!" });
  machine.frame({ dialogue: "Hello from QM!", thinking: false, visible: true });

  assert.equal(
    speechShown,
    "Hello from QM!",
    "speech should show despite ownership change mid-turn",
  );
  assert.equal(thinkingShown, false, "thinking should stay hidden when speech shows");
});

test("backend thinking flag only reaches bubble owner", async () => {
  // Backend filters thinking by bubble owner: only owner receives thinking=true
  let thinking1 = false;
  const owner = createBubbleMachine({
    showSpeech() {},
    hideSpeech() {},
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
  const nonOwner = createBubbleMachine({
    showSpeech() {},
    hideSpeech() {},
    showThinking() {
      thinking2 = true;
    },
    hideThinking() {
      thinking2 = false;
    },
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  // Backend sends thinking=true only to bubble owner (owner), not to non-owner
  owner.frame({ thinking: true, visible: true, bubble: true });
  nonOwner.frame({ thinking: false, visible: true, bubble: false });

  // On first rise, grace timer applies (250ms)
  await new Promise((resolve) => setTimeout(resolve, 300));
  assert.equal(thinking1, true, "bubble owner shows thinking after grace");
  assert.equal(thinking2, false, "non-owner never receives thinking flag");
});
