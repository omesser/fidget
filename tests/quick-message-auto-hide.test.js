import { test } from "node:test";
import assert from "node:assert/strict";
import { createQuickMessage, AUTO_HIDE_DELAY_MS } from "../src/quick-message.js";

test("quick-message does not auto-hide while focused", async () => {
  let timers = [];
  const machine = createQuickMessage({
    schedule: (fn, ms) => {
      const id = setTimeout(fn, ms);
      timers.push(id);
      return id;
    },
    clear: (id) => {
      clearTimeout(id);
      timers = timers.filter((t) => t !== id);
    },
    send: () => {},
    onChange: () => {},
    available: true,
  });

  machine.enterSprite();
  await new Promise((resolve) => setTimeout(resolve, 1600));

  assert.equal(machine.visible, true, "pill should show after hover delay");

  machine.focus();

  machine.leavePill();
  machine.leaveSprite();

  await new Promise((resolve) => setTimeout(resolve, AUTO_HIDE_DELAY_MS + 100));

  assert.equal(machine.visible, true, "pill should not auto-hide while focused");

  machine.blur();

  await new Promise((resolve) => setTimeout(resolve, AUTO_HIDE_DELAY_MS + 100));

  assert.equal(machine.visible, false, "pill should auto-hide after blur");

  timers.forEach((id) => clearTimeout(id));
});

test("quick-message does not auto-hide while typing", async () => {
  let timers = [];
  const machine = createQuickMessage({
    schedule: (fn, ms) => {
      const id = setTimeout(fn, ms);
      timers.push(id);
      return id;
    },
    clear: (id) => {
      clearTimeout(id);
      timers = timers.filter((t) => t !== id);
    },
    send: () => {},
    onChange: () => {},
    available: true,
  });

  machine.enterSprite();
  await new Promise((resolve) => setTimeout(resolve, 1600));

  assert.equal(machine.visible, true, "pill should show after hover delay");

  machine.focus();
  machine.setText("test");

  machine.leavePill();
  machine.leaveSprite();

  await new Promise((resolve) => setTimeout(resolve, AUTO_HIDE_DELAY_MS + 100));

  assert.equal(machine.visible, true, "pill should not auto-hide while text present");

  machine.setText("");

  assert.equal(machine.visible, true, "pill should remain visible immediately after clearing text");

  machine.blur();

  await new Promise((resolve) => setTimeout(resolve, AUTO_HIDE_DELAY_MS + 100));

  assert.equal(machine.visible, false, "pill should auto-hide after text cleared and blur");

  timers.forEach((id) => clearTimeout(id));
});

test("quick-message auto-hides when unfocused and empty", async () => {
  let timers = [];
  const machine = createQuickMessage({
    schedule: (fn, ms) => {
      const id = setTimeout(fn, ms);
      timers.push(id);
      return id;
    },
    clear: (id) => {
      clearTimeout(id);
      timers = timers.filter((t) => t !== id);
    },
    send: () => {},
    onChange: () => {},
    available: true,
  });

  machine.enterSprite();
  await new Promise((resolve) => setTimeout(resolve, 1600));

  assert.equal(machine.visible, true);

  machine.blur();
  machine.leaveSprite();

  await new Promise((resolve) => setTimeout(resolve, AUTO_HIDE_DELAY_MS + 100));

  assert.equal(machine.visible, false, "pill should auto-hide when pointer leaves and no text/focus");

  timers.forEach((id) => clearTimeout(id));
});
