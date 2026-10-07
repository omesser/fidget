// The quick-message composer above a Character. Timing and dismiss live here
// so a hover can be driven without a webview.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  AUTO_HIDE_DELAY_MS,
  BUBBLE_YIELD_MS,
  HOVER_DELAY_MS,
  DRAG_DISMISS_PX,
  applyQuickMessageGate,
  createQuickMessage,
  createDraftReporter,
  crossedDrag,
  keepOnDrag,
  placeQuickMessage,
  quickMessageConnects,
  quickMessageMirror,
  quickMessagePrompt,
} from "../src/quick-message.js";

function harness(options = {}) {
  let time = 0;
  /** @type {{ id: number, fn: () => void, at: number }[]} */
  const tasks = [];
  let nextId = 1;
  const sent = [];
  const changes = [];
  const qm = createQuickMessage({
    schedule(fn, ms) {
      const id = nextId;
      nextId += 1;
      tasks.push({ id, fn, at: time + ms });
      return id;
    },
    clear(id) {
      const index = tasks.findIndex((task) => task.id === id);
      if (index >= 0) tasks.splice(index, 1);
    },
    send(text) {
      sent.push(text);
    },
    onChange() {
      changes.push(qm.typing);
    },
    available: options.available,
  });

  function advance(ms) {
    time += ms;
    for (let guard = 0; guard < 20; guard += 1) {
      const due = tasks
        .filter((task) => task.at <= time)
        .sort((a, b) => a.at - b.at || a.id - b.id);
      if (due.length === 0) return;
      const task = due[0];
      const index = tasks.indexOf(task);
      tasks.splice(index, 1);
      task.fn();
    }
  }

  return { qm, sent, advance, changes };
}

function shown() {
  const harnessed = harness();
  harnessed.qm.enterSprite();
  harnessed.advance(HOVER_DELAY_MS);
  return harnessed;
}

test("the composer appears after a 1.5s hover, focused, and not before", () => {
  const { qm, advance } = harness();

  qm.enterSprite();
  advance(HOVER_DELAY_MS - 1);
  assert.equal(qm.visible, false, "a glance is not a hover");
  assert.equal(qm.typing, false);

  advance(1);
  assert.equal(qm.visible, true);
  assert.equal(qm.typing, true, "the caret is claimed as the pill appears");
  assert.equal(qm.takeFocus(), true);
  assert.equal(qm.takeFocus(), false, "the overlay focuses the field once");
});

test("leaving before the hover completes does not show the composer", () => {
  const { qm, advance } = harness();

  qm.enterSprite();
  advance(HOVER_DELAY_MS - 1);
  qm.leaveSprite();
  advance(HOVER_DELAY_MS);
  assert.equal(qm.visible, false);
});

test("a second enter does not postpone the dwell", () => {
  const { qm, advance } = harness();

  qm.enterSprite();
  advance(1000);
  qm.enterSprite();
  advance(HOVER_DELAY_MS - 1000);
  assert.equal(qm.visible, true);
});

test("clicking away during the dwell does not show the composer later", () => {
  const { qm, advance } = harness();

  qm.enterSprite();
  advance(1000);
  qm.outside();
  advance(HOVER_DELAY_MS);
  assert.equal(qm.visible, false);
});

test("a Summon keeps the composer down until the pointer leaves", () => {
  const { qm, advance } = harness();

  qm.enterSprite();
  advance(500);
  qm.summon();
  for (let tick = 0; tick < 10; tick += 1) {
    qm.enterSprite();
    advance(HOVER_DELAY_MS);
  }
  assert.equal(qm.visible, false, "Chat is up, so the pill stays down");
  assert.equal(qm.takeFocus(), false, "the field never claims the caret");

  qm.leaveSprite();
  qm.enterSprite();
  advance(HOVER_DELAY_MS);
  assert.equal(qm.visible, true, "a fresh hover opens it again");
});

test("leaving after the pill is up does not dismiss it immediately", () => {
  const { qm, advance } = shown();

  qm.setText("hey");
  qm.leaveSprite();
  advance(500);
  assert.equal(qm.visible, true);
  assert.equal(qm.text, "hey");
  assert.equal(qm.typing, true);
});

test("pill auto-hides after 3s if empty and pointer leaves both sprite and pill", () => {
  const { qm, advance } = shown();

  qm.blur();
  qm.leaveSprite();
  qm.leavePill();

  advance(2900);
  assert.equal(qm.visible, true, "pill stays visible before 3s");

  advance(100);
  assert.equal(qm.visible, false, "pill auto-hides after 3s continuous away");
});

test("auto-hide cancels if pointer re-enters sprite before 3s", () => {
  const { qm, advance } = shown();

  qm.leaveSprite();
  qm.leavePill();
  advance(2000);

  // Re-enter sprite before 3s
  qm.enterSprite();
  advance(2000);

  assert.equal(qm.visible, true, "pill stays visible when sprite re-entered");
});

test("re-entering sprite resets auto-hide timer to fresh 3s on next leave", () => {
  const { qm, advance } = shown();

  qm.blur();
  qm.leaveSprite();
  qm.leavePill();
  advance(2000);

  qm.enterSprite();

  qm.leaveSprite();
  advance(2900);
  assert.equal(qm.visible, true, "pill still visible at 2.9s of fresh timer");

  advance(100);
  assert.equal(qm.visible, false, "pill hides after fresh 3s from second leave");
});

test("auto-hide cancels if pointer re-enters pill before 3s", () => {
  const { qm, advance } = shown();

  qm.leaveSprite();
  qm.leavePill();
  advance(2000);

  // Re-enter pill before 3s
  qm.enterPill();
  advance(2000);

  assert.equal(qm.visible, true, "pill stays visible when pill re-entered");
});

test("auto-hide does not trigger if user has typed non-empty text", () => {
  const { qm, advance } = shown();

  qm.setText("hey");
  qm.leaveSprite();
  qm.leavePill();

  // Even after 3s, should not auto-hide because text is present
  advance(3000);
  assert.equal(qm.visible, true, "pill with text does not auto-hide");
  assert.equal(qm.text, "hey");
});

test("auto-hide treats whitespace-only as empty", () => {
  const { qm, advance } = shown();

  qm.setText("   ");
  qm.blur();
  qm.leaveSprite();
  qm.leavePill();

  advance(3000);
  assert.equal(qm.visible, false, "pill with only whitespace auto-hides");
});

test("auto-hide does not trigger if only sprite is left but pill is hovered", () => {
  const { qm, advance } = shown();

  // Simulate entering the pill (user moved from sprite to pill)
  qm.enterPill();
  qm.leaveSprite();

  advance(3000);
  assert.equal(qm.visible, true, "pill stays visible while hovered");
});

test("auto-hide does not trigger if only pill is left but sprite is hovered", () => {
  const { qm, advance } = shown();

  // Leave pill but not sprite (shouldn't happen in practice but test the logic)
  qm.leavePill();
  // Still on sprite

  advance(3000);
  assert.equal(qm.visible, true, "pill stays visible while sprite hovered");
});

test("existing dismiss paths still work with auto-hide feature", () => {
  const { qm } = shown();

  // outside dismiss
  qm.outside();
  assert.equal(qm.visible, false);

  // Show again and test drag dismiss
  const { qm: qm2 } = shown();
  qm2.drag();
  assert.equal(qm2.visible, false);

  // Show again and test summon dismiss
  const { qm: qm3 } = shown();
  qm3.summon();
  assert.equal(qm3.visible, false);
});

test("blurring the field keeps the pill and releases the typing hold", () => {
  const { qm } = shown();

  qm.blur();
  assert.equal(qm.visible, true);
  assert.equal(qm.typing, false);

  qm.focus();
  assert.equal(qm.typing, true);
});

test("click outside and a double-click dismiss, draft included", () => {
  for (const dismiss of ["outside", "summon"]) {
    const { qm } = shown();
    qm.setText("hey");
    qm[dismiss]();
    assert.equal(qm.visible, false, dismiss);
    assert.equal(qm.text, "", dismiss);
    assert.equal(qm.typing, false, dismiss);
  }

});

test("a pet drag closes an empty pill, caret or not, and keeps a typed one", () => {
  const empty = shown().qm;
  assert.equal(empty.typing, true, "a fresh pill holds the caret");
  empty.drag();
  assert.equal(empty.visible, false);
  assert.equal(empty.draft, null, "a closed pill reports no draft");

  const typed = shown().qm;
  typed.setText("hey");
  typed.drag();
  assert.equal(typed.visible, true);
  assert.deepEqual(typed.draft, { text: "hey", focused: true }, "the draft rides on");
});

test("a poke still reaches the pet and does not dismiss the composer", () => {
  const { qm } = shown();
  let pokes = 0;
  qm.press("character", () => {
    pokes += 1;
  });
  qm.press("composer", () => {
    pokes += 1;
  });
  assert.equal(pokes, 1);
  assert.equal(qm.visible, true);
});

test("a drag is a few pixels of movement, not the click itself", () => {
  assert.equal(DRAG_DISMISS_PX, 4);
  assert.equal(crossedDrag(DRAG_DISMISS_PX - 1, 0), false);
  assert.equal(crossedDrag(DRAG_DISMISS_PX, 0), true);
  assert.equal(crossedDrag(0, DRAG_DISMISS_PX), true);
});

test("a drag keeps the pill only for text worth keeping", () => {
  const rows = [
    ["", false, "an empty pill closes"],
    ["   ", false, "whitespace is empty, the same as Send sees it"],
    ["hey", true, "typed text follows the Character"],
  ];
  for (const [text, keep, why] of rows) assert.equal(keepOnDrag(text), keep, why);
});

test("drag cancels hover timer and dismisses pill", () => {
  const { qm, advance } = harness();
  qm.enterSprite();
  advance(HOVER_DELAY_MS - 100);
  assert.equal(qm.visible, false, "not yet visible before timer completes");

  qm.drag();
  assert.equal(qm.visible, false, "drag dismisses any pending hover");

  advance(200);
  assert.equal(qm.visible, false, "timer does not fire after drag dismisses");
});

test("dispose clears the composer so composing cannot stick", () => {
  const { qm, changes, advance } = shown();
  qm.setText("hey");
  const before = changes.length;

  qm.dispose();
  assert.equal(qm.visible, false);
  assert.equal(qm.typing, false);
  assert.equal(qm.text, "");
  assert.equal(changes.at(-1), false);
  assert.ok(changes.length > before, "the overlay is told the hold is gone");

  qm.enterSprite();
  advance(HOVER_DELAY_MS);
  qm.restore("hey");
  assert.equal(qm.visible, false, "dispose is final");
  assert.equal(qm.typing, false);
});

test("Escape does not dismiss", () => {
  const { qm } = shown();
  assert.equal(qm.keydown("Escape"), false);
  assert.equal(qm.visible, true);
});

test("Send delivers the trimmed line and puts the pill away", () => {
  const { qm, sent } = shown();
  qm.enterPill();
  qm.setText("  hey, status?  ");

  assert.equal(qm.submit(), true);
  assert.deepEqual(sent, ["hey, status?"]);
  assert.equal(qm.visible, false, "the pointer still on the pill does not keep it up");
  assert.equal(qm.text, "");
  assert.equal(qm.typing, false, "no caret left to hold the pet");
});

test("no pill opens while Chat is up, and a fresh hover opens it once Chat is gone", () => {
  const { qm, advance } = harness();

  qm.setChatOpen(true);
  for (let tick = 0; tick < 10; tick += 1) {
    qm.enterSprite();
    advance(HOVER_DELAY_MS);
  }
  assert.equal(qm.visible, false, "a hover over the pet with Chat open is not a quick message");
  assert.equal(qm.takeFocus(), false, "the pill never claims the caret from Chat");

  qm.setChatOpen(false);
  qm.leaveSprite();
  qm.enterSprite();
  advance(HOVER_DELAY_MS);
  assert.equal(qm.visible, true);
});

test("a dwell under way when Chat opens does not open the pill", () => {
  const { qm, advance } = harness();

  qm.enterSprite();
  advance(HOVER_DELAY_MS - 100);
  qm.setChatOpen(true);
  advance(100);
  assert.equal(qm.visible, false);
});

test("Chat opening keeps a draft in an open pill", () => {
  const { qm, advance } = shown();
  qm.setText("half a thought");

  qm.setChatOpen(true);
  qm.leaveSprite();
  qm.leavePill();
  advance(10_000);
  assert.equal(qm.visible, true, "typing in progress is not thrown away");
  assert.equal(qm.text, "half a thought");
});

test("a bubble takes an idle pill the pointer has left", () => {
  const { qm, advance } = shown();
  qm.blur();
  qm.leaveSprite();
  assert.equal(qm.visible, true, "auto-hide has not run yet");

  qm.setBubble(true);
  advance(BUBBLE_YIELD_MS);
  assert.equal(qm.visible, false, "the Character is talking, so the pill waits");
  assert.ok(BUBBLE_YIELD_MS < AUTO_HIDE_DELAY_MS);
});

test("a bubble waits for the pointer to leave the sprite and the pill", () => {
  const { qm, advance } = shown();
  qm.setBubble(true);
  advance(10_000);
  assert.equal(qm.visible, true, "a hovered pill stays over the bubble");

  qm.blur();
  qm.leaveSprite();
  advance(BUBBLE_YIELD_MS - 1);
  qm.enterPill();
  advance(10_000);
  assert.equal(qm.visible, true, "the pointer crossed onto the pill in time");

  qm.leavePill();
  advance(BUBBLE_YIELD_MS);
  assert.equal(qm.visible, false);
});

test("a bubble keeps a pill that holds a draft", () => {
  const { qm, advance } = shown();
  qm.setText("half a thought");
  qm.leaveSprite();

  qm.setBubble(true);
  advance(10_000);
  assert.equal(qm.visible, true);
  assert.equal(qm.text, "half a thought");
});

test("a hover still opens the pill while a bubble is up", () => {
  const { qm, advance } = harness();
  qm.setBubble(true);

  qm.enterSprite();
  advance(HOVER_DELAY_MS);
  assert.equal(qm.visible, true, "hovering is how the user talks over the Character");
});

test("an empty line is not sent", () => {
  const { qm, sent } = shown();
  qm.setText("   ");

  assert.equal(qm.submit(), false);
  assert.deepEqual(sent, []);
  assert.equal(qm.visible, true);
});

test("an unavailable pill appears but refuses text, focus, and send", () => {
  const { qm, sent, advance } = harness({ available: false });

  qm.enterSprite();
  advance(HOVER_DELAY_MS);
  assert.equal(qm.visible, true, "the status still has to be readable");
  assert.equal(qm.available, false);
  assert.equal(qm.typing, false, "a field that takes nothing is not a typing hold");
  assert.equal(qm.takeFocus(), false);

  qm.setText("hey");
  qm.focus();
  assert.equal(qm.text, "");
  assert.equal(qm.typing, false);
  assert.equal(qm.keydown("Enter"), false);
  assert.equal(qm.submit(), false);
  assert.deepEqual(sent, []);
  assert.equal(qm.visible, true);
});

test("a refused send brings the frozen pill back without the line", () => {
  const { qm, sent } = shown();
  qm.setText("hey");
  qm.dismiss();
  qm.setAvailable(false);

  qm.restore("hey");
  assert.equal(qm.visible, true);
  assert.equal(qm.text, "");
  assert.equal(qm.typing, false);
  assert.deepEqual(sent, []);
});

test("freezing clears a draft and thawing claims the caret", () => {
  const { qm, sent } = shown();
  qm.setText("hey");

  qm.setAvailable(false);
  assert.equal(qm.visible, true);
  assert.equal(qm.text, "", "a draft would hide the unavailable sentence");
  assert.equal(qm.typing, false);
  assert.equal(qm.submit(), false);
  assert.deepEqual(sent, []);

  qm.setAvailable(true);
  assert.equal(qm.available, true);
  assert.equal(qm.typing, true);
  assert.equal(qm.takeFocus(), true);
  qm.setText("hey");
  assert.equal(qm.submit(), true);
  assert.deepEqual(sent, ["hey"]);
});

test("the unavailable pill points at Chat instead of echoing its composer", () => {
  const off = { name: "bmo", configured: true, enabled: false };
  assert.equal(quickMessagePrompt(off), "Connect an AI to talk to me");
  assert.equal(quickMessagePrompt(null), "Connect an AI to talk to me");
  assert.equal(quickMessageConnects(off), true);
  assert.equal(quickMessageConnects(null), true);
  const login = { name: "bmo", configured: true, enabled: true, login: "claude /login" };
  assert.equal(quickMessagePrompt(login), "Connect an AI to talk to me");
  assert.equal(quickMessageConnects(login), true);

  const starting = {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "hermes",
    harness: { name: "hermes", alive: false, initializing: true },
  };
  assert.equal(quickMessagePrompt(starting), "Starting Hermes…");
  assert.equal(quickMessageConnects(starting), false, "a wait that ends on its own is not a link");

  const down = {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "hermes",
    harness: { name: "hermes", alive: false, session: null },
  };
  assert.equal(quickMessagePrompt(down), "Connect an AI to talk to me");
  assert.equal(quickMessageConnects(down), true);

  const ready = { name: "bmo", configured: true, enabled: true };
  assert.equal(quickMessagePrompt(ready), "talk to me");
  assert.equal(quickMessageConnects(ready), false);
  assert.equal(
    quickMessageMirror("", "Connect an AI to talk to me", false),
    "Connect an AI to talk to me\u200b",
    "the frozen pill reserves the sentence, or overflow clips it",
  );
  assert.equal(quickMessageMirror("", "talk to me", true), "\u200b");
  assert.equal(quickMessageMirror("hey", "talk to me", true), "hey\u200b");
});

function gateDouble() {
  return {
    field: {
      disabled: false,
      placeholder: "",
      labels: {},
      setAttribute(name, value) {
        this.labels[name] = value;
      },
    },
    send: { disabled: false, hidden: false },
    link: { hidden: true },
    machine: {
      ready: true,
      setAvailable(value) {
        this.ready = value;
      },
    },
  };
}

test("a draft does not hide the unavailable sentence when the pill freezes", () => {
  const { qm } = shown();
  qm.setText("still drafting");
  const gate = gateDouble();
  gate.machine = qm;

  applyQuickMessageGate(gate, { name: "bmo", configured: true, enabled: false });

  assert.equal(qm.text, "");
  assert.equal(gate.field.placeholder, "Connect an AI to talk to me");
  assert.equal(
    quickMessageMirror(qm.text, gate.field.placeholder, qm.available),
    "Connect an AI to talk to me\u200b",
    "leftover draft text would hide the sentence the placeholder is showing",
  );
});

test("the gate disables the field and send from the same opening chat uses", () => {
  const frozen = gateDouble();
  applyQuickMessageGate(frozen, { name: "bmo", configured: true, enabled: false });
  assert.equal(frozen.machine.ready, false);
  assert.equal(frozen.field.disabled, true);
  assert.equal(frozen.send.disabled, true);
  assert.equal(frozen.field.placeholder, "Connect an AI to talk to me");
  assert.equal(frozen.field.labels["aria-label"], "Connect an AI to talk to me");
  assert.equal(frozen.link.hidden, false, "the hint carries a link that opens Chat");
  assert.equal(frozen.send.hidden, true, "Send leaves so the link is the pill's one control");

  const starting = gateDouble();
  applyQuickMessageGate(starting, {
    name: "bmo",
    configured: true,
    enabled: true,
    harness_name: "cursor-agent",
    harness: { name: "cursor-agent", alive: false, initializing: true },
  });
  assert.equal(starting.field.disabled, true);
  assert.equal(starting.send.disabled, true);
  assert.equal(starting.field.placeholder, "Starting Cursor…");
  assert.equal(starting.link.hidden, true);
  assert.equal(starting.send.hidden, false);

  const ready = gateDouble();
  applyQuickMessageGate(ready, { name: "bmo", configured: true, enabled: true });
  assert.equal(ready.machine.ready, true);
  assert.equal(ready.field.disabled, false);
  assert.equal(ready.send.disabled, false);
  assert.equal(ready.field.placeholder, "talk to me");
  assert.equal(ready.field.labels["aria-label"], "Quick message");
  assert.equal(ready.link.hidden, true);
  assert.equal(ready.send.hidden, false);
});

test("Enter sends and Shift+Enter does not", () => {
  const { qm, sent } = shown();
  qm.setText("hey");

  assert.equal(qm.keydown("Enter", { shiftKey: true }), false);
  assert.deepEqual(sent, []);
  assert.equal(qm.visible, true);

  assert.equal(qm.keydown("Enter"), true);
  assert.deepEqual(sent, ["hey"]);
  assert.equal(qm.visible, false, "Enter is a Send, so the pill goes too");
});

test("the composer sits above Speech when the two would share a box", () => {
  const sprite = { x: 100, y: 400, width: 64, height: 64 };
  const size = { width: 200, height: 40 };
  const bounds = { x: 0, y: 0, width: 1000, height: 800 };
  const speech = { x: 32, y: 350, width: 200, height: 80 };

  const clear = placeQuickMessage(sprite, size, bounds, null);
  assert.equal(clear.y, 350, "same 10px gap placeBubble leaves over the head");

  const stacked = placeQuickMessage(sprite, size, bounds, speech);
  assert.equal(stacked.y, 300, "one gap above the Speech bubble, not on top of it");
});

test("inverted flag matches final vertical position relative to sprite", () => {
  const sprite = { x: 100, y: 50, width: 64, height: 64 };
  const size = { width: 200, height: 40 };
  const bounds = { x: 0, y: 0, width: 1000, height: 800 };

  const above = placeQuickMessage(sprite, size, bounds, null);
  assert.equal(above.y, 0, "pill sits 10px above sprite top");
  assert.equal(above.inverted, false, "inverted is false when pill is above sprite center");

  const nearTop = { x: 100, y: 30, width: 64, height: 64 };
  const flipped = placeQuickMessage(nearTop, size, bounds, null);
  assert.equal(flipped.y, 104, "pill flips below sprite when it would cover it");
  assert.equal(flipped.inverted, true, "inverted is true when pill is below sprite center");

  const speech = { x: 32, y: 104, width: 200, height: 80 };
  const pushed = placeQuickMessage(nearTop, size, bounds, speech);
  assert.ok(pushed.y < 104, "pill moves above speech to avoid collision");
  assert.equal(pushed.inverted, false, "inverted reflects final position above sprite");
});

test("the pill stays whole on its display, like Speech, when the sprite reaches an edge", () => {
  const size = { width: 200, height: 40 };
  const bounds = { x: 0, y: 0, width: 1920, height: 1080 };

  const pastLeft = placeQuickMessage({ x: -50, y: 400, width: 64, height: 64 }, size, bounds, null);
  assert.equal(pastLeft.x, 0, "slides in from the seam rather than hanging off it");

  const pastRight = placeQuickMessage({ x: 1900, y: 400, width: 64, height: 64 }, size, bounds, null);
  assert.equal(pastRight.x, 1720, "and in from the right edge");
});

test("without a clickable link the pill still names the fix as text", () => {
  const gate = gateDouble();
  gate.link = null;
  applyQuickMessageGate(gate, { name: "bmo", configured: false, enabled: false });
  assert.equal(gate.field.placeholder, "Connect an AI to talk to me");
  assert.equal(gate.field.disabled, true);
  assert.equal(gate.send.hidden, false, "with no link to stand in, Send keeps its place");
});

function shellDouble() {
  const told = [];
  const invoke = (command, args) => {
    told.push([command, args]);
    return Promise.resolve();
  };
  return { told, invoke };
}

test("each Instance's closed pill reaches the Shell, even when two close together", () => {
  const { told, invoke } = shellDouble();
  const drafts = createDraftReporter(invoke);

  drafts.report("a", { text: "hi", focused: true });
  drafts.report("b", { text: "yo", focused: false });
  drafts.report("a", null);
  drafts.report("b", null);
  drafts.report("b", null);

  assert.deepEqual(
    told.slice(-2),
    [
      ["overlay_report_qm_draft", { instance: "a", payload: null }],
      ["overlay_report_qm_draft", { instance: "b", payload: null }],
    ],
    "both closes are told, so neither Character is left holding its walk",
  );
  assert.equal(told.length, 4, "an unchanged draft is not told again");
});

test("a removed Instance's draft is cleared, so its walk is not held", () => {
  const { told, invoke } = shellDouble();
  const drafts = createDraftReporter(invoke);

  drafts.report("gone", { text: "half", focused: true });
  drafts.forget("gone");

  assert.deepEqual(told.at(-1), ["overlay_report_qm_draft", { instance: "gone", payload: null }]);
});

// `setOwner` is what each placement says: whether this overlay owns the
// Instance's bubble, and the draft the Shell carries to that owner.
const DRAFT = { text: "half a thought", focused: true };

test("the overlay that gains the bubble opens the pill with the carried draft", () => {
  const { qm } = harness();
  qm.setOwner(false, null);
  qm.setOwner(true, DRAFT);
  assert.equal(qm.visible, true);
  assert.equal(qm.text, "half a thought", "the typed text is not lost on the crossing");
  assert.equal(qm.typing, true);
  assert.equal(qm.takeFocus(), true, "it had the caret, so it takes it back");
});

test("a fresh overlay's first frame as owner counts as gaining it", () => {
  const { qm } = harness();
  qm.setOwner(true, { text: "half", focused: false });
  assert.equal(qm.text, "half");
  assert.equal(qm.typing, false, "no caret it did not have");
  assert.equal(qm.takeFocus(), false, "so it does not take focus");
});

test("a frame that keeps the owner leaves what is being typed alone", () => {
  const { qm } = harness();
  qm.setOwner(true, DRAFT);
  qm.takeFocus();
  qm.setText("half a thought, and more");

  qm.setOwner(true, DRAFT);
  assert.equal(qm.text, "half a thought, and more", "a draft one report behind is not news");
  assert.equal(qm.takeFocus(), false, "nor a reason to move the caret");
});

test("a frame still carrying a sent, dismissed, or dragged-away draft does not reopen it", () => {
  const close = {
    send(qm) {
      qm.keydown("Enter");
    },
    dismiss(qm) {
      qm.dismiss();
    },
    drag(qm) {
      qm.setText("");
      qm.drag();
    },
  };
  for (const [how, act] of Object.entries(close)) {
    const { qm } = harness();
    qm.setOwner(true, DRAFT);
    act(qm);
    assert.equal(qm.visible, false, how);
    qm.setOwner(true, DRAFT);
    assert.equal(qm.visible, false, `${how}: the Shell has not heard the close yet`);
  }
});

test("the overlay that loses the bubble hides its pill and lets the draft go", () => {
  const { qm, changes } = harness();
  qm.setOwner(true, DRAFT);
  const before = changes.length;

  qm.setOwner(false, null);
  assert.equal(qm.visible, false);
  assert.equal(qm.text, "", "the Shell holds the draft now, not this overlay");
  assert.ok(changes.length > before, "the overlay hears the pill go");
  assert.equal(qm.owner, false, "and knows not to report it closed");
});
