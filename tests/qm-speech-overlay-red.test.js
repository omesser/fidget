// Red contract test for QM/speech/thinking overlay behavior at seam crossings.
// These tests MUST FAIL at 318b325d and PASS after the fix.

import { test } from "node:test";
import assert from "node:assert/strict";
import { createBubbleMachine } from "../src/bubble.js";
import { createQuickMessage } from "../src/quick-message.js";

test("RED: ownership flip drops speech immediately", () => {
  let speechShown = null;
  const machine = createBubbleMachine({
    showSpeech(text) {
      speechShown = text;
    },
    hideSpeech() {
      speechShown = null;
    },
    showThinking() {},
    hideThinking() {},
    schedule: (fn, ms) => setTimeout(fn, ms),
    cancel: (id) => clearTimeout(id),
  });

  machine.event({ dialogue: "Hello!" });
  machine.frame({ dialogue: "Hello!", visible: true, thinking: false });
  assert.equal(speechShown, "Hello!", "speech should show on bubble owner");

  // Ownership changes mid-speech (character crosses seam)
  machine.hideButKeepTurn();
  assert.equal(speechShown, null, "speech must hide immediately when ownership flips");
});

test("RED: ownership flicker cancels grace timer preventing ellipsis", async () => {
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

  // Backend sets thinking on bubble owner after QM send
  machine.frame({ thinking: true, visible: true });
  await new Promise((resolve) => setTimeout(resolve, 100)); // 100ms < 250ms grace

  // Ownership flickers (bubble_owner changes briefly)
  machine.hideButKeepTurn();
  assert.equal(thinkingShown, false, "thinking hides during ownership change");

  // Owner returns immediately (AI turn still in flight)
  // Should show thinking immediately with no grace since turn already started
  machine.frame({ thinking: true, visible: true });

  // At tip 318b325d, this re-arms grace timer instead of showing immediately
  assert.equal(
    thinkingShown,
    true,
    "RED: thinking should show immediately on re-entry (must fail at 318b325d)"
  );
});


test("PASS: thinking is only sent to bubble owner (validates backend behavior)", () => {
  let thinking1 = false;
  let thinking2 = false;

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

  // Backend sends thinking:true only to bubble owner (overlay 1)
  // Non-owner (overlay 0) receives thinking:false
  owner.frame({ thinking: true, visible: true, bubble: true });
  nonOwner.frame({ thinking: false, visible: true, bubble: false });

  assert.equal(thinking1, false, "grace timer not yet elapsed (250ms)");
  assert.equal(thinking2, false, "non-owner never receives thinking flag");
});

test("PASS: stable ownership shows thinking after grace", async () => {
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
  await new Promise((resolve) => setTimeout(resolve, 300)); // > 250ms grace

  assert.equal(thinkingShown, true, "thinking shows after grace with stable ownership");
});
