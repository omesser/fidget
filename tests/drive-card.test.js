// The bindings card, the charge bar, and which keys possession listens to.
// Layout and pixels stay in the overlay. This is the model the page paints.

import assert from "node:assert/strict";
import { test } from "node:test";

import { chargeFill, possessionCard, possessionRects } from "../src/drive-card.js";
import { driveEdge } from "../src/drive-keys.js";

test("arrows, space and escape are the drive keys, and a repeat is ignored", () => {
  assert.deepEqual(driveEdge("ArrowLeft", false, true), { key: "left", down: true });
  assert.deepEqual(driveEdge("ArrowRight", false, false), { key: "right", down: false });
  assert.deepEqual(driveEdge(" ", false, true), { key: "space", down: true });
  assert.deepEqual(driveEdge("Escape", false, true), { key: "escape", down: true });
  assert.equal(driveEdge("ArrowLeft", true, true), null);
  assert.equal(driveEdge("ArrowUp", false, true), null);
  assert.equal(driveEdge("ArrowDown", false, true), null);
  assert.equal(driveEdge("1", false, true), null);
  assert.equal(driveEdge("a", false, true), null);
});

test("the card lists the bound keys under the explanation", () => {
  const card = possessionCard({ driving: true, paused: false });
  assert.equal(card.paused, false);
  assert.equal(card.heading, "Manually drive the fidget on your screen");
  assert.deepEqual(card.keys, [
    ["← →", "Walk"],
    ["Space", "Jump"],
    ["Esc", "Release"],
  ]);
});

test("a paused card dims and asks for a click", () => {
  const card = possessionCard({ driving: true, paused: true });
  assert.equal(card.paused, true);
  assert.equal(card.heading, "Click to resume");
  assert.equal(card.dimmed, true);
});

test("the card is absent until the sprite is possessed", () => {
  assert.equal(possessionCard({ driving: false, paused: false }), null);
  assert.equal(possessionCard({ driving: false, paused: true }), null);
});

test("the bar fills with the charge and hides when space is up", () => {
  assert.equal(chargeFill(null), null);
  assert.equal(chargeFill(undefined), null);
  assert.equal(chargeFill(0), 0);
  assert.equal(chargeFill(0.4), 0.4);
  assert.equal(chargeFill(1), 1);
  assert.equal(chargeFill(1.5), 1);
});

test("the card takes a click and the charge bar does not", () => {
  assert.deepEqual(possessionRects([10, 20, 220, 88], [10, 112, 220, 8]), [
    { rect: [10, 20, 220, 88], clickable: true },
    { rect: [10, 112, 220, 8], clickable: false },
  ]);
  assert.deepEqual(possessionRects(null, null), []);
});
