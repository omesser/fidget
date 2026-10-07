import { test } from "node:test";
import assert from "node:assert/strict";
import { createQuickMessage, HOVER_DELAY_MS } from "../src/quick-message.js";

test("drag does not dismiss pill when it has text", async () => {
  let visible = false;
  const machine = createQuickMessage({
    schedule: (fn, ms) => setTimeout(fn, ms),
    clear: (id) => clearTimeout(id),
    send: () => {},
    onChange: () => {
      visible = machine.visible;
    },
    available: true,
  });

  machine.enterSprite();
  await new Promise((resolve) => setTimeout(resolve, HOVER_DELAY_MS + 100));
  machine.setText("hello");
  assert.equal(machine.visible, true);
  assert.equal(machine.text, "hello");

  machine.drag();

  assert.equal(machine.visible, true, "pill with text should not dismiss on drag");
  assert.equal(machine.text, "hello", "text should be preserved");
});

test("drag does not dismiss pill when it is focused", async () => {
  let visible = false;
  const machine = createQuickMessage({
    schedule: (fn, ms) => setTimeout(fn, ms),
    clear: (id) => clearTimeout(id),
    send: () => {},
    onChange: () => {
      visible = machine.visible;
    },
    available: true,
  });

  machine.enterSprite();
  await new Promise((resolve) => setTimeout(resolve, HOVER_DELAY_MS + 100));
  machine.focus();
  assert.equal(machine.visible, true);

  machine.drag();

  assert.equal(machine.visible, true, "focused pill should not dismiss on drag");
});

test("drag dismisses pill when empty and unfocused", async () => {
  let visible = false;
  const machine = createQuickMessage({
    schedule: (fn, ms) => setTimeout(fn, ms),
    clear: (id) => clearTimeout(id),
    send: () => {},
    onChange: () => {
      visible = machine.visible;
    },
    available: true,
  });

  machine.enterSprite();
  await new Promise((resolve) => setTimeout(resolve, HOVER_DELAY_MS + 100));
  assert.equal(machine.visible, true);
  
  machine.blur();

  machine.drag();

  assert.equal(machine.visible, false, "empty unfocused pill should dismiss on drag");
});
