// The bindings card, the charge bar, and which keys possession listens to.
// Layout and pixels stay in the overlay. This is the model the page paints.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  chargeFill,
  paintPossession,
  placePossession,
  possessionCard,
  possessionRects,
} from "../src/possession-card.js";
import { possessionEdge } from "../src/possession-keys.js";

test("arrows, space and escape are the possession keys, and a repeat is ignored", () => {
  assert.deepEqual(possessionEdge("ArrowLeft", false, true), { key: "left", down: true });
  assert.deepEqual(possessionEdge("ArrowRight", false, false), { key: "right", down: false });
  assert.deepEqual(possessionEdge(" ", false, true), { key: "space", down: true });
  assert.deepEqual(possessionEdge("Escape", false, true), { key: "escape", down: true });
  assert.equal(possessionEdge("ArrowLeft", true, true), null);
  assert.equal(possessionEdge("ArrowUp", false, true), null);
  assert.equal(possessionEdge("ArrowDown", false, true), null);
  assert.equal(possessionEdge("1", false, true), null);
  assert.equal(possessionEdge("a", false, true), null);
});

test("the card lists the bound keys under the explanation", () => {
  const card = possessionCard({ possessed: true, paused: false });
  assert.equal(card.paused, false);
  assert.equal(card.heading, "Manually drive the fidget on your screen");
  assert.deepEqual(card.keys, [
    ["← →", "Walk"],
    ["Space", "Jump"],
    ["Esc", "Release Possession"],
  ]);
});

test("a paused card dims and asks for a click", () => {
  const card = possessionCard({ possessed: true, paused: true });
  assert.equal(card.paused, true);
  assert.equal(card.heading, "Click to resume");
  assert.equal(card.dimmed, true);
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

function covers(a, b) {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}

function onScreen(rect, bounds) {
  return (
    rect.x >= bounds.x &&
    rect.y >= bounds.y &&
    rect.x + rect.width <= bounds.x + bounds.width &&
    rect.y + rect.height <= bounds.y + bounds.height
  );
}

test("a card at the top corner of the display stays on screen", () => {
  const bounds = { x: 0, y: 0, width: 800, height: 600 };
  const card = { width: 220, height: 88 };
  const sprite = { x: 0, y: 12, width: 128, height: 128 };
  const placed = placePossession(sprite, card, 8, 8, bounds, null);
  assert.equal(onScreen({ ...placed.card, width: card.width, height: card.height }, bounds), true);
  assert.equal(onScreen(placed.bar, bounds), true);
});

test("a card steps clear of the quick-message pill", () => {
  const bounds = { x: 0, y: 0, width: 800, height: 600 };
  const card = { width: 220, height: 88 };
  const sprite = { x: 290, y: 320, width: 128, height: 128 };
  const pill = { x: 244, y: 200, width: 220, height: 70 };
  const placed = placePossession(sprite, card, 8, 8, bounds, null, pill);
  const top = Math.min(placed.card.y, placed.bar.y);
  const bottom = Math.max(placed.card.y + card.height, placed.bar.y + placed.bar.height);
  const stack = { x: placed.card.x, y: top, width: card.width, height: bottom - top };
  assert.equal(covers(stack, pill), false);
  assert.equal(onScreen({ ...placed.card, width: card.width, height: card.height }, bounds), true);
  assert.equal(onScreen(placed.bar, bounds), true);
});

test("a card that steps off the pill does not land back on the bubble", () => {
  const bounds = { x: 0, y: 0, width: 800, height: 600 };
  const card = { width: 220, height: 88 };
  const sprite = { x: 290, y: 200, width: 128, height: 128 };
  const bubble = { x: 0, y: 130, width: 800, height: 80 };
  const pill = { x: 0, y: 80, width: 800, height: 40 };
  const placed = placePossession(sprite, card, 8, 8, bounds, bubble, pill);
  const top = Math.min(placed.card.y, placed.bar.y);
  const bottom = Math.max(placed.card.y + card.height, placed.bar.y + placed.bar.height);
  const stack = { x: placed.card.x, y: top, width: card.width, height: bottom - top };
  assert.equal(covers(stack, bubble), false);
  assert.equal(covers(stack, pill), false);
  assert.equal(onScreen({ ...placed.card, width: card.width, height: card.height }, bounds), true);
  assert.equal(onScreen(placed.bar, bounds), true);
});

test("a card near the top stays clear of the bubble and the pill", () => {
  const bounds = { x: 0, y: 0, width: 800, height: 600 };
  const card = { width: 220, height: 88 };
  const sprite = { x: 290, y: 16, width: 128, height: 128 };
  const bubble = { x: 0, y: 154, width: 800, height: 50 };
  const pill = { x: 0, y: 40, width: 800, height: 50 };
  const placed = placePossession(sprite, card, 8, 8, bounds, bubble, pill);
  const top = Math.min(placed.card.y, placed.bar.y);
  const bottom = Math.max(placed.card.y + card.height, placed.bar.y + placed.bar.height);
  const stack = { x: placed.card.x, y: top, width: card.width, height: bottom - top };
  assert.equal(covers(stack, bubble), false);
  assert.equal(covers(stack, pill), false);
  assert.equal(onScreen({ ...placed.card, width: card.width, height: card.height }, bounds), true);
  assert.equal(onScreen(placed.bar, bounds), true);
});

test("a card steps clear of the speech bubble", () => {
  const bounds = { x: 0, y: 0, width: 800, height: 600 };
  const card = { width: 220, height: 88 };
  const sprite = { x: 290, y: 320, width: 128, height: 128 };
  const speech = { x: 244, y: 190, width: 220, height: 90 };
  const placed = placePossession(sprite, card, 8, 8, bounds, speech);
  const cardRect = { ...placed.card, width: card.width, height: card.height };
  assert.equal(covers(cardRect, speech), false);
  assert.equal(covers(placed.bar, speech), false);
  assert.equal(onScreen(cardRect, bounds), true);
  assert.equal(onScreen(placed.bar, bounds), true);
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

test("the card takes a click and the charge bar does not", () => {
  assert.deepEqual(possessionRects([10, 20, 220, 88], [10, 112, 220, 8]), [
    { rect: [10, 20, 220, 88], clickable: true },
    { rect: [10, 112, 220, 8], clickable: false },
  ]);
  assert.deepEqual(possessionRects(null, null), []);
});
