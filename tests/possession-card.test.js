// The bindings card, the charge bar, and which keys possession listens to.
// Layout and pixels stay in the overlay. This is the model the page paints.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  chargeFill,
  paintPossession,
  placePossession,
  possessionBounds,
  possessionCard,
  possessionRects,
} from "../src/possession-card.js";
import { possessionEdge } from "../src/possession-keys.js";

test("arrows, space, escape and digits are the possession keys, and a repeat is ignored", () => {
  assert.deepEqual(possessionEdge("ArrowLeft", false, true), { key: "left", down: true });
  assert.deepEqual(possessionEdge("ArrowRight", false, false), { key: "right", down: false });
  assert.deepEqual(possessionEdge(" ", false, true), { key: "space", down: true });
  assert.deepEqual(possessionEdge("Escape", false, true), { key: "escape", down: true });
  assert.equal(possessionEdge("ArrowLeft", true, true), null);
  assert.deepEqual(possessionEdge("ArrowUp", false, true), { key: "up", down: true });
  assert.deepEqual(possessionEdge("ArrowDown", false, false), { key: "down", down: false });
  assert.deepEqual(possessionEdge("&", false, true, "Digit1"), { key: "1", down: true });
  assert.deepEqual(possessionEdge("1", false, false, "Numpad9"), { key: "9", down: false });
  assert.equal(possessionEdge("&", true, true, "Digit1"), null);
  assert.equal(possessionEdge("0", false, true, "Digit0"), null);
  assert.equal(possessionEdge("a", false, true, "KeyA"), null);
  assert.equal(possessionEdge("d", false, true, "KeyD"), null);
});

test("the pill's field keeps its keydowns, but every keyup still lets go", () => {
  assert.equal(possessionEdge("ArrowLeft", false, true, "ArrowLeft", true), null);
  assert.equal(possessionEdge("1", false, true, "Digit1", true), null);
  assert.deepEqual(possessionEdge("ArrowLeft", false, false, "ArrowLeft", true), { key: "left", down: false });
  assert.deepEqual(possessionEdge(" ", false, false, "Space", true), { key: "space", down: false });
  assert.deepEqual(possessionEdge("1", false, false, "Digit1", true), { key: "1", down: false });
});

test("the card lists the bound keys under the explanation", () => {
  const card = possessionCard({ possessed: true, paused: false });
  assert.equal(card.paused, false);
  assert.equal(card.heading, "Manually drive the fidget on your screen");
  assert.deepEqual(card.keys, [
    ["← →", "Walk"],
    ["↑", "Climb"],
    ["↓", "Drop"],
    ["Space", "Jump"],
    ["Esc", "Release Possession"],
  ]);
});

test("the card numbers the shell's list and writes each name as a label", () => {
  const card = possessionCard({
    possessed: true,
    paused: false,
    behaviors: ["wave", "nod"],
  });
  assert.deepEqual(card.keys, [
    ["← →", "Walk"],
    ["↑", "Climb"],
    ["↓", "Drop"],
    ["Space", "Jump"],
    ["Esc", "Release Possession"],
    ["1", "Wave"],
    ["2", "Nod"],
  ]);
});

test("the card does not reorder or drop a name the shell sent", () => {
  const names = ["k", "j", "walk"];
  const card = possessionCard({ possessed: true, paused: false, behaviors: names });
  assert.deepEqual(card.keys.slice(5), [
    ["1", "K"],
    ["2", "J"],
    ["3", "Walk"],
  ]);
});

test("a paused card dims and asks for a click", () => {
  const card = possessionCard({ possessed: true, paused: true });
  assert.equal(card.paused, true);
  assert.equal(card.heading, "Click to resume");
  assert.equal(card.dimmed, true);
});

test("the bindings card uses the shared sans stack", () => {
  const shared = readFileSync(new URL("../src/chat-shared.css", import.meta.url), "utf8");
  const css = readFileSync(new URL("../src/main.css", import.meta.url), "utf8");
  const sans = shared.match(/--shared-sans:\s*([^;]+);/);
  assert.ok(sans, "chat-shared.css names the sans stack once");
  assert.equal(sans[1].trim(), "system-ui, -apple-system, sans-serif");
  const block = (selector) => {
    const found = css.match(new RegExp(`${selector}\\s*\\{([^}]+)\\}`));
    assert.ok(found, `${selector} has a block`);
    return found[1];
  };
  assert.match(block("\\.possession"), /font-family:\s*var\(--shared-sans\)/);
  assert.match(block("\\.bubble"), /font-family:\s*var\(--shared-sans\)/);
});

test("the card is absent until the sprite is possessed", () => {
  assert.equal(possessionCard({ possessed: false, paused: false }), null);
  assert.equal(possessionCard({ possessed: false, paused: true }), null);
});

test("the bar fills with the charge and hides when space is up", () => {
  assert.equal(chargeFill(null), null);
  assert.equal(chargeFill(undefined), null);
  assert.equal(chargeFill(0), 0);
  assert.equal(chargeFill(0.4), 0.4);
  assert.equal(chargeFill(1), 1);
  assert.equal(chargeFill(1.5), 1);
});

function onScreen(rect, bounds) {
  return (
    rect.x >= bounds.x &&
    rect.y >= bounds.y &&
    rect.x + rect.width <= bounds.x + bounds.width &&
    rect.y + rect.height <= bounds.y + bounds.height
  );
}

test("the card sits in the top-left inset and does not follow the sprite", () => {
  const bounds = { x: 0, y: 0, width: 800, height: 600 };
  const card = { width: 220, height: 88 };
  const placed = placePossession(card, 8, 8, bounds);
  assert.equal(placed.card.x, 16);
  assert.equal(placed.card.y, 16);
  assert.equal(placed.bar.x, 16);
  assert.equal(placed.bar.y, 16 + card.height + 8);
  assert.equal(onScreen({ ...placed.card, width: card.width, height: card.height }, bounds), true);
  assert.equal(onScreen(placed.bar, bounds), true);
});

test("the card is seated in the usable area, below a top menu bar", () => {
  const frame = { x: 0, y: 0, width: 1440, height: 900 };
  const menuBar = 24;
  const work = { x: 0, y: menuBar, width: frame.width, height: frame.height - menuBar };
  const card = { width: 220, height: 88 };
  const placed = placePossession(card, 8, 8, possessionBounds(work, frame));
  assert.equal(placed.card.x, 16);
  assert.equal(placed.card.y, menuBar + 16);
  assert.equal(placed.bar.y, placed.card.y + card.height + 8);
  const cardRect = { ...placed.card, width: card.width, height: card.height };
  assert.equal(onScreen(cardRect, work), true);
  assert.equal(onScreen(placed.bar, work), true);
  assert.equal(possessionBounds(null, frame).y, 0);
});

test("the card moves to the display the sprite crosses onto", () => {
  const card = { width: 220, height: 88 };
  const here = { x: 0, y: 0, width: 1920, height: 1080 };
  const next = { x: 1920, y: 0, width: 1440, height: 900 };
  const onHere = placePossession(card, 8, 8, here);
  const onNext = placePossession(card, 8, 8, next);
  assert.equal(onHere.card.x, 16);
  assert.equal(onHere.card.y, 16);
  assert.equal(onNext.card.x, 1920 + 16);
  assert.equal(onNext.card.y, 16);
  assert.equal(onNext.bar.x, onNext.card.x);
  assert.equal(onNext.bar.y, onNext.card.y + card.height + 8);
});

test("the card stays inside a display whose origin is not zero", () => {
  const card = { width: 220, height: 88 };
  const left = { x: -1920, y: 0, width: 1920, height: 1080 };
  const above = { x: 0, y: -800, width: 1920, height: 800 };
  for (const bounds of [left, above]) {
    const placed = placePossession(card, 8, 8, bounds);
    const cardRect = { ...placed.card, width: card.width, height: card.height };
    assert.equal(placed.card.x, bounds.x + 16);
    assert.equal(placed.card.y, bounds.y + 16);
    assert.equal(onScreen(cardRect, bounds), true);
    assert.equal(onScreen(placed.bar, bounds), true);
  }
});

function element() {
  const node = {
    className: "",
    textContent: "",
    style: {},
    children: [],
    classList: { toggle() {} },
    append(...kids) {
      node.children.push(...kids);
    },
    replaceChildren(...kids) {
      node.children = kids;
    },
    querySelector(selector) {
      const name = selector.slice(1);
      const walk = (current) => {
        if ((current.className || "").split(" ").includes(name)) return current;
        for (const child of current.children) {
          const found = walk(child);
          if (found) return found;
        }
        return null;
      };
      return walk(node);
    },
  };
  return node;
}

test("a charge-only change keeps the same card nodes", () => {
  const previous = globalThis.document;
  globalThis.document = { createElement: () => element() };
  try {
    const card = element();
    const bar = element();
    const fill = element();
    fill.className = "possession-charge-fill";
    bar.append(fill);
    const model = possessionCard({ possessed: true, paused: false });

    paintPossession(card, bar, model, 0.2);
    const heading = card.children[0];
    const keys = card.children[1];
    paintPossession(card, bar, model, 0.8);

    assert.equal(card.children[0], heading);
    assert.equal(card.children[1], keys);
    assert.equal(fill.style.width, "80%");
  } finally {
    if (previous === undefined) delete globalThis.document;
    else globalThis.document = previous;
  }
});

test("the card takes a click only while paused, and the charge bar never does", () => {
  const card = [10, 20, 220, 88];
  const bar = [10, 112, 220, 8];
  assert.deepEqual(possessionRects(card, bar, true), [
    { rect: card, clickable: true },
    { rect: bar, clickable: false },
  ]);
  assert.deepEqual(possessionRects(card, bar, false), [
    { rect: card, clickable: false },
    { rect: bar, clickable: false },
  ]);
  assert.deepEqual(possessionRects(null, null, false), []);
});
