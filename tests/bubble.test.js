// Run with `node --test tests/`.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  bubbleDuration,
  wrapText,
  placeBubble,
  createBubbleMachine,
  THINKING_GRACE_MS,
  THINKING_MIN_HOLD_MS,
} from "../src/bubble.js";

const testMeasureFn = (text) => ({ width: text.length * 8 });

test("bubble duration is 900ms + 55ms per character, clamped to 2-8s", () => {
  assert.equal(bubbleDuration("hi"), 2000, "min clamp");
  assert.equal(bubbleDuration("hello"), 2000, "still at min");
  // "hello there" = 11 chars = 900 + 605 = 1505ms, clamped to 2000ms min
  assert.equal(bubbleDuration("hello there"), 2000, "short text clamped to min");
  const medium = "a".repeat(30);
  assert.equal(bubbleDuration(medium), 900 + 55 * 30, "base + per-char, no clamp");

  const long = "a".repeat(200);
  assert.equal(bubbleDuration(long), 8000, "max clamp");
});

// One Speech duration. `bubbleDuration` clamps how long the bubble may show a
// line; the Shell's `CARRY_WINDOW` is how long it may re-say one to a new bubble
// owner. A longer carry resurrects a line nobody could still be reading, and a
// shorter one drops one still on screen (#178).
test("the Shell carries a line for exactly as long as the bubble can show one", () => {
  const dir = dirname(fileURLToPath(import.meta.url));
  const shell = readFileSync(join(dir, "../src-tauri/src/main.rs"), "utf8");

  const carry = shell.match(/const CARRY_WINDOW: Duration = Duration::from_secs\((\d+)\)/);
  assert.ok(carry, "the Shell names a carry window");
  assert.equal(
    Number(carry[1]) * 1000,
    bubbleDuration("a".repeat(1000)),
    "the carry window is bubbleDuration's max clamp",
  );
});

test("wrap text at max width", () => {
  const short = "hi";
  const { lines, truncated } = wrapText(short, 260, testMeasureFn);
  assert.equal(lines.length, 1);
  assert.equal(lines[0], "hi");
  assert.equal(truncated, false, "a line that fits is not truncated");
});

test("long text wraps at word boundaries", () => {
  const text = "The quick brown fox jumps over the lazy dog";
  const { lines } = wrapText(text, 100, testMeasureFn);
  assert.ok(lines.length > 1, "text should wrap");
  assert.ok(lines.every(line => line.length > 0), "no empty lines");
});

test("text truncates with ellipsis past 6 lines", () => {
  const manyLines = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8";
  const { lines, truncated } = wrapText(manyLines, 260, testMeasureFn);
  assert.equal(lines.length, 6, "truncated to 6 lines");
  assert.ok(lines[5].endsWith("…"), "last line has ellipsis");
  assert.equal(truncated, true, "and says so");
});

// The flag is what puts the "Open chat" control in the bubble, so it has to be
// true for a turn that ran off the bottom by wrapping as well as one that
// arrived with too many paragraphs, and false for a line that fills the last one.
test("wrapping past the last line is truncation too", () => {
  const oneLongParagraph = "word ".repeat(200).trim();
  const { lines, truncated } = wrapText(oneLongParagraph, 100, testMeasureFn);
  assert.equal(lines.length, 6);
  assert.equal(truncated, true, "the paragraph outran the bubble");
});

test("exactly six lines is not truncation", () => {
  const sixLines = "line1\nline2\nline3\nline4\nline5\nline6";
  const { lines, truncated } = wrapText(sixLines, 260, testMeasureFn);
  assert.equal(lines.length, 6);
  assert.equal(truncated, false, "nothing was left out");
  assert.equal(lines[5], "line6", "so no ellipsis either");
});

// The bubble keeps a reply's breaks rather than collapsing them: a list read as
// one run-on line is worse than a clamped list, and the clamp has a hand-off.
test("a reply's paragraphs are the bubble's lines", () => {
  const reply = "Here's what I found:\n\n- the roster loads\n- the session resumed";
  const { lines, truncated } = wrapText(reply, 260, testMeasureFn);

  assert.deepEqual(lines, [
    "Here's what I found:",
    "- the roster loads",
    "- the session resumed",
  ], "a paragraph per line, and the blank one costs none of the six");
  assert.equal(truncated, false, "three of six fit");
});

// The cost of keeping them, asserted rather than discovered: a reply that fit
// while it was flattened can now run past the ceiling and hand off to Chat.
test("paragraphs reach the six-line ceiling sooner than one flowed line", () => {
  const items = ["one", "two", "three", "four", "five", "six"];
  const listed = `Here's what I found:\n${items.map((item) => `- ${item}`).join("\n")}`;

  assert.equal(wrapText(listed.replaceAll("\n", " "), 260, testMeasureFn).truncated, false,
    "flattened, the same reply fits inside the six");

  const { lines, truncated } = wrapText(listed, 260, testMeasureFn);
  assert.equal(lines.length, 6);
  assert.equal(truncated, true, "seven paragraphs do not fit in six lines");
  assert.ok(lines[5].endsWith("…"), "and the bubble says so");
});

test("bubble placement stays above sprite by default", () => {
  const spriteRect = { x: 100, y: 400, width: 64, height: 64 };
  const bubbleSize = { width: 200, height: 100 };
  const displayBounds = { x: 0, y: 0, width: 1000, height: 800 };

  const pos = placeBubble(spriteRect, bubbleSize, displayBounds);

  assert.ok(pos.y < spriteRect.y, "bubble is above sprite");
  assert.ok(pos.x >= displayBounds.x, "bubble is within display left");
  assert.ok(pos.x + bubbleSize.width <= displayBounds.x + displayBounds.width, "bubble is within display right");
  assert.equal(pos.tailOffset, 0, "tail centered when bubble not clamped");
  assert.equal(pos.inverted, false, "tail still points down");
});

// Near the ceiling the bubble inverts below the sprite at the same mirrored
// distance when above would cover the face.
test("bubble inverts below sprite at ceiling when above would cover face", () => {
  const spriteRect = { x: 100, y: 50, width: 64, height: 64 };
  const bubbleSize = { width: 200, height: 100 };
  const displayBounds = { x: 0, y: 0, width: 1000, height: 800 };

  const pos = placeBubble(spriteRect, bubbleSize, displayBounds);

  assert.ok(pos.y > spriteRect.y + spriteRect.height, "bubble is below sprite, not clamped above");
  assert.equal(pos.y, spriteRect.y + spriteRect.height + 10, "10px gap below, mirroring the normal above gap");
});

// #903: the overlay flips the tail from this flag. y is the known ceiling
// geometry (sprite at 50, 64 tall, bubble 100, 10px gap) — not recomputed.
test("inverted Speech bubble reports inverted so the tail can point up", () => {
  const spriteRect = { x: 100, y: 50, width: 64, height: 64 };
  const bubbleSize = { width: 200, height: 100 };
  const displayBounds = { x: 0, y: 0, width: 1000, height: 800 };

  const pos = placeBubble(spriteRect, bubbleSize, displayBounds);

  assert.equal(pos.y, 124);
  assert.equal(pos.inverted, true);
});

// The clamp is the only thing that moves the bubble off the head: with room
// above, the same sprite height gets the untouched 10px gap.
test("bubble keeps its 10px gap over the head when there is room", () => {
  const spriteRect = { x: 100, y: 400, width: 64, height: 64 };
  const bubbleSize = { width: 200, height: 100 };
  const displayBounds = { x: 0, y: 0, width: 1000, height: 800 };

  const pos = placeBubble(spriteRect, bubbleSize, displayBounds);

  assert.equal(pos.y + bubbleSize.height, spriteRect.y - 10, "10px of clear air under the tail");
});

test("bubble inverts below sprite when ceiling clamp would cover the face", () => {
  const spriteRect = { x: 100, y: 50, width: 64, height: 64 };
  const bubbleSize = { width: 200, height: 100 };
  const displayBounds = { x: 0, y: 0, width: 1000, height: 800 };

  const pos = placeBubble(spriteRect, bubbleSize, displayBounds);

  assert.ok(pos.y > spriteRect.y + spriteRect.height, "bubble is below sprite when ceiling would cover face");
  assert.equal(pos.y, spriteRect.y + spriteRect.height + 10, "mirrored 10px gap below sprite");
});

test("bubble inverts only when clamp would cover sprite, not when it clears", () => {
  const spriteRect = { x: 100, y: 150, width: 64, height: 64 };
  const bubbleSize = { width: 200, height: 60 };
  const displayBounds = { x: 0, y: 0, width: 1000, height: 800 };

  const pos = placeBubble(spriteRect, bubbleSize, displayBounds);

  assert.ok(pos.y < spriteRect.y, "bubble stays above when it fits without covering");
  assert.equal(pos.y, spriteRect.y - bubbleSize.height - 10, "normal above placement");
});

test("bubble slides horizontally at display edges", () => {
  const spriteNearLeftEdge = { x: 10, y: 400, width: 64, height: 64 };
  const bubbleSize = { width: 200, height: 100 };
  const displayBounds = { x: 0, y: 0, width: 1000, height: 800 };

  const pos = placeBubble(spriteNearLeftEdge, bubbleSize, displayBounds);

  assert.ok(pos.x >= displayBounds.x, "bubble clamped to left edge");
  assert.ok(pos.x + bubbleSize.width <= displayBounds.x + displayBounds.width, "bubble within right bound");
  const spriteCenterX = spriteNearLeftEdge.x + spriteNearLeftEdge.width / 2;
  const bubbleCenterX = pos.x + bubbleSize.width / 2;
  assert.equal(pos.tailOffset, spriteCenterX - bubbleCenterX, "tail offset points to sprite");
});

test("bubble stays whole across display seam", () => {
  const spriteAtSeam = { x: 995, y: 400, width: 64, height: 64 };
  const bubbleSize = { width: 200, height: 100 };
  const displayBounds = { x: 0, y: 0, width: 1000, height: 800 };

  const pos = placeBubble(spriteAtSeam, bubbleSize, displayBounds);

  assert.ok(pos.x >= displayBounds.x, "bubble not past left edge");
  assert.ok(pos.x + bubbleSize.width <= displayBounds.x + displayBounds.width, "bubble not past right edge");
});

// --- The bubble machine: what shows and hides, driven by fake timers. ---

function machineHarness() {
  let now = 0;
  let nextId = 1;
  const timers = new Map();
  const calls = [];
  // main.js draws both bubbles into one element in one of two modes, so the
  // harness models that element rather than two booleans: speech strictly
  // wins. A hide landing after the show that replaced it reads as a blank surface.
  let surface = null;
  const machine = createBubbleMachine({
    showSpeech(text, truncated) {
      surface = "speech";
      // The truncation mark rides in the remembered text, not in the bubble.
      calls.push(`showSpeech:${text}`);
    },
    hideSpeech() {
      surface = null;
      calls.push("hideSpeech");
    },
    showAsk() {
      surface = "speech";
      calls.push("showAsk");
    },
    showThinking() {
      surface = "thinking";
      calls.push("showThinking");
    },
    hideThinking() {
      surface = null;
      calls.push("hideThinking");
    },
    schedule(fn, ms) {
      const id = nextId++;
      timers.set(id, { fn, at: now + ms });
      return id;
    },
    cancel(id) {
      timers.delete(id);
    },
  });
  // Steps timer by timer so a callback that schedules (the grace arming the
  // min-hold) sees the clock at its own fire time, the way real timers do.
  const advance = (ms) => {
    const target = now + ms;
    for (;;) {
      const due = [...timers.entries()]
        .filter(([, timer]) => timer.at <= target)
        .sort((a, b) => a[1].at - b[1].at)[0];
      if (!due) break;
      const [id, timer] = due;
      timers.delete(id);
      now = timer.at;
      timer.fn();
    }
    now = target;
  };
  const placement = (overrides) => ({
    dialogue: null,
    thinking: false,
    visible: true,
    ...overrides,
  });
  return { machine, calls, advance, placement, surface: () => surface };
}

test("the bubble's one surface is never taken back off the wrong one", () => {
  const { machine, advance, placement, surface } = machineHarness();

  machine.frame(placement({ thinking: true }));
  advance(THINKING_GRACE_MS);
  assert.equal(surface(), "thinking", "grace elapsed, indicator up");

  const reply = placement({ dialogue: "hello" });
  machine.event(reply);
  machine.frame(reply);
  assert.equal(surface(), "speech", "the indicator's hide lands before the reply's show");

  // A second turn starts while the reply is still being read. It may not take
  // the surface, and the hide that ends it may not arrive early.
  machine.frame(placement({ thinking: true }));
  advance(bubbleDuration("hello") - 1);
  assert.equal(surface(), "speech", "the reply holds the surface for its reading time");

  advance(1);
  assert.equal(surface(), null, "reading time up");

  advance(THINKING_GRACE_MS);
  assert.equal(surface(), "thinking", "the turn still in flight gets the surface back");
});

test("a reply hides the thinking indicator the same frame its bubble shows", () => {
  const { machine, calls, advance, placement } = machineHarness();

  const thinkingTick = placement({ thinking: true });
  machine.event(thinkingTick);
  machine.frame(thinkingTick);
  advance(THINKING_GRACE_MS);
  assert.deepEqual(calls, ["showThinking"], "grace elapsed, indicator up");

  // The reply lands while the min-hold is still pending: the hold is for
  // silent endings and must not delay the answer.
  const reply = placement({ dialogue: "hello" });
  machine.event(reply);
  machine.frame(reply);
  assert.deepEqual(
    calls,
    ["showThinking", "hideThinking", "showSpeech:hello"],
    "indicator gone before the speech bubble shows, no timers involved",
  );
});

test("a dialogue pulse overwritten before the next drawn frame still shows", () => {
  const { machine, calls, placement } = machineHarness();

  // The Engine ticks faster than the display refreshes: the dialogue tick
  // and its successor both arrive before draw samples the newest placement.
  machine.event(placement({ dialogue: "missed me?" }));
  machine.event(placement({}));
  machine.frame(placement({}));

  assert.deepEqual(calls, ["showSpeech:missed me?"], "the pulse was latched");
});

test("a silent ending within the min-hold clears at the hold, not before", () => {
  const { machine, calls, advance, placement } = machineHarness();

  const thinkingTick = placement({ thinking: true });
  machine.frame(thinkingTick);
  advance(THINKING_GRACE_MS);
  assert.deepEqual(calls, ["showThinking"]);

  machine.frame(placement({}));
  assert.deepEqual(calls, ["showThinking"], "held: no flicker on a fast silent end");

  advance(THINKING_MIN_HOLD_MS);
  assert.deepEqual(calls, ["showThinking", "hideThinking"], "cleared at the hold");
});

test("a silent ending after the min-hold clears immediately", () => {
  const { machine, calls, advance, placement } = machineHarness();

  machine.frame(placement({ thinking: true }));
  advance(THINKING_GRACE_MS + THINKING_MIN_HOLD_MS);
  assert.deepEqual(calls, ["showThinking"], "still thinking at hold expiry");

  machine.frame(placement({}));
  assert.deepEqual(calls, ["showThinking", "hideThinking"]);
});

test("a reply faster than the grace never shows the indicator", () => {
  const { machine, calls, advance, placement } = machineHarness();

  machine.frame(placement({ thinking: true }));
  const reply = placement({ dialogue: "quick" });
  machine.event(reply);
  machine.frame(reply);
  advance(THINKING_GRACE_MS + THINKING_MIN_HOLD_MS);

  assert.deepEqual(calls, ["showSpeech:quick"], "no flash of the indicator");
});

test("the speech bubble hides itself after its reading time", () => {
  const { machine, calls, advance, placement } = machineHarness();

  const reply = placement({ dialogue: "hi" });
  machine.event(reply);
  machine.frame(reply);
  advance(bubbleDuration("hi"));

  assert.deepEqual(calls, ["showSpeech:hi", "hideSpeech"]);
});

test("a new turn waits behind a displayed reply, indicator only after it hides", () => {
  const { machine, calls, advance, placement } = machineHarness();

  const reply = placement({ dialogue: "hi" });
  machine.event(reply);
  machine.frame(reply);
  assert.deepEqual(calls, ["showSpeech:hi"]);

  // A second poke starts a new turn while the reply is still on screen: the
  // indicator must not appear over it, however long the turn runs.
  machine.frame(placement({ thinking: true }));
  advance(THINKING_GRACE_MS + THINKING_MIN_HOLD_MS);
  assert.deepEqual(calls, ["showSpeech:hi"], "nothing shows over the reply");

  // Reading time up: the reply hides, and the still-running turn starts its
  // grace from this moment.
  advance(bubbleDuration("hi") - THINKING_GRACE_MS - THINKING_MIN_HOLD_MS);
  assert.deepEqual(calls, ["showSpeech:hi", "hideSpeech"]);

  advance(THINKING_GRACE_MS);
  assert.deepEqual(calls, ["showSpeech:hi", "hideSpeech", "showThinking"]);
});

test("a reply landing in the post-speech grace never flashes the indicator", () => {
  const { machine, calls, advance, placement } = machineHarness();

  const first = placement({ dialogue: "hi" });
  machine.event(first);
  machine.frame(first);
  machine.frame(placement({ thinking: true }));
  advance(bubbleDuration("hi"));
  assert.deepEqual(calls, ["showSpeech:hi", "hideSpeech"], "grace just restarted");

  const second = placement({ dialogue: "again" });
  machine.event(second);
  machine.frame(second);
  advance(THINKING_GRACE_MS + THINKING_MIN_HOLD_MS);

  assert.deepEqual(
    calls,
    ["showSpeech:hi", "hideSpeech", "showSpeech:again"],
    "the indicator never appeared",
  );
});

// A Poke dropped because the user owes an answer (ADR-0016) points at Chat.
// It carries a control, so it stays up for the longest reading window.
test("an asking pulse points at Chat for the longest reading window", () => {
  const { machine, calls, advance, placement, surface } = machineHarness();

  machine.event(placement({ asking: true }));
  machine.event(placement({}));
  machine.frame(placement({ thinking: true }));
  assert.deepEqual(calls, ["showAsk"], "latched like a line, and speech wins");

  advance(bubbleDuration("a".repeat(1000)) - 1);
  assert.equal(surface(), "speech", "still up to be clicked");
  advance(1);
  assert.deepEqual(calls, ["showAsk", "hideSpeech"]);
});

test("a hidden sprite drops the asking pulse rather than queueing it", () => {
  const { machine, calls, placement } = machineHarness();

  machine.event(placement({ asking: true }));
  machine.frame(placement({ visible: false }));
  machine.frame(placement({}));
  assert.deepEqual(calls, []);
});


test("a quick-message send shows thinking immediately, with no grace", () => {
  const { machine, calls, advance, placement, surface } = machineHarness();

  machine.aiTurnStarted();
  assert.equal(surface(), "thinking", "indicator is up the moment the send is accepted");
  assert.deepEqual(calls, ["showThinking"]);
  advance(0);
  assert.equal(surface(), "thinking", "no grace timer to wait out");

  // Engine dialogue still clears it the same frame.
  const reply = placement({ dialogue: "on it" });
  machine.event(reply);
  machine.frame(reply);
  assert.equal(surface(), "speech");
});

test("abandoning a quick-message send clears the thinking it armed", () => {
  const { machine, surface } = machineHarness();

  machine.aiTurnStarted();
  assert.equal(surface(), "thinking");
  machine.aiTurnAbandoned();
  assert.equal(surface(), null);
});

test("quick-message thinking holds until reply even when Engine has not raised thinking", () => {
  const { machine, advance, placement, surface } = machineHarness();

  machine.aiTurnStarted();
  machine.frame(placement({ thinking: false }));
  advance(THINKING_MIN_HOLD_MS);
  assert.equal(surface(), "thinking", "min-hold expiry must not clear a pending AI turn");
  machine.frame(placement({ thinking: false }));
  assert.equal(surface(), "thinking", "held until dialogue or abandon");
});

// --- #178: one overlay owns the bubble; the rest draw the art only. ---
// The Shell nulls `dialogue`, `thinking` and `cue` on every overlay but the
// owner's, so the placements below are what a losing overlay is really handed.

test("a losing overlay is told no thinking, so it never arms the indicator", () => {
  const { machine, advance, placement, surface } = machineHarness();

  machine.frame(placement({ thinking: false, bubble: false }));
  advance(THINKING_GRACE_MS + THINKING_MIN_HOLD_MS);
  assert.equal(surface(), null, "grace never armed: this display is not the owner");

  machine.frame(placement({ thinking: true, bubble: true }));
  advance(THINKING_GRACE_MS);
  assert.equal(surface(), "thinking", "the same turn, owned, arms it");
});

test("an overlay that loses the bubble drops a turn it started", () => {
  const { machine, advance, placement, surface } = machineHarness();

  machine.aiTurnStarted();
  machine.frame(placement({ bubble: false }));
  advance(THINKING_GRACE_MS + THINKING_MIN_HOLD_MS);
  assert.equal(surface(), null, "the owner shows this turn's dots, not this display");
});

test("a turn the bubble carried away comes back with its dots at once, and its reply", () => {
  const { machine, advance, placement, surface } = machineHarness();

  machine.frame(placement({ thinking: true }));
  advance(THINKING_GRACE_MS);
  machine.hideButKeepTurn();
  assert.equal(surface(), null, "the display it left is blank");

  machine.frame(placement({ thinking: true }));
  assert.equal(surface(), "thinking", "no second grace: the dots were already up");

  const reply = placement({ dialogue: "back" });
  machine.event(reply);
  machine.frame(reply);
  assert.equal(surface(), "speech");
});

test("a line crossing the seam hides on the old display before it shows on the new", () => {
  // Two overlays, two machines: the shell hands the line to the owner, and on a
  // crossing says it again to the new one (`carry_line`), while the old one
  // hides on the tick it loses ownership — the way main.js does on `!bubble`.
  const a = machineHarness();
  const b = machineHarness();

  a.machine.event(a.placement({ dialogue: "hi", bubble: true }));
  a.machine.frame(a.placement({ bubble: true }));
  b.machine.event(b.placement({ dialogue: null, bubble: false }));
  assert.equal(a.surface(), "speech", "the owner shows the line");
  assert.equal(b.surface(), null, "the other display was never told the line");

  // Mid-reading, ownership flips: the shell re-pulses to b and stops naming a.
  a.machine.hideAllNow();
  b.machine.event(b.placement({ dialogue: "hi", bubble: true }));
  b.machine.frame(b.placement({ bubble: true }));
  assert.equal(a.surface(), null, "the old display is already clear");
  assert.equal(b.surface(), "speech", "and the new one shows the same line");
});

// A reply the token cap ended is still spoken. The mark rides in the
// remembered text, not in the bubble.
test("a truncated reply is spoken without the mark visible", () => {
  const { machine, calls, placement } = machineHarness();

  machine.event(placement({ dialogue: "Mine now, and the desk is", truncated: true }));
  machine.frame(placement({}));
  assert.deepEqual(calls, ["showSpeech:Mine now, and the desk is"]);

  machine.event(placement({ dialogue: "all mine" }));
  machine.frame(placement({}));
  assert.deepEqual(
    calls,
    ["showSpeech:Mine now, and the desk is", "showSpeech:all mine"],
    "the next whole line is not marked with the last one's mark",
  );
});
