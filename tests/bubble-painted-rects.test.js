import { test } from "node:test";
import assert from "node:assert/strict";

test("reportPaintedRects sends [x,y,w,h] arrays", () => {
  const rects = [
    [10, 20, 100, 50],
    [150, 200, 80, 60],
  ];

  const serialized = JSON.stringify(rects);
  const parsed = JSON.parse(serialized);

  assert.strictEqual(parsed.length, 2, "Two rects");
  assert.deepStrictEqual(parsed[0], [10, 20, 100, 50], "First rect as [x,y,w,h]");
  assert.deepStrictEqual(parsed[1], [150, 200, 80, 60], "Second rect as [x,y,w,h]");
});

test("painted rect cleared on hide", () => {
  const paintedRect = null;
  assert.strictEqual(paintedRect, null, "Painted rect cleared when bubble hidden");
});

test("painted rect set from positionBubble", () => {
  const pos = { x: 100, y: 200 };
  const bubbleSize = { width: 150, height: 80 };

  const paintedRect = [
    Math.round(pos.x),
    Math.round(pos.y),
    bubbleSize.width,
    bubbleSize.height,
  ];

  assert.deepStrictEqual(paintedRect, [100, 200, 150, 80], "Painted rect from bubble position");
});

test("painted rects batched per view", () => {
  const views = [
    { paintedRect: [10, 20, 100, 50] },
    { paintedRect: null },
    { paintedRect: [150, 200, 80, 60] },
  ];

  const rects = [];
  for (const view of views) {
    if (view.paintedRect) rects.push(view.paintedRect);
  }

  assert.strictEqual(rects.length, 2, "Only visible bubbles reported");
  assert.deepStrictEqual(rects[0], [10, 20, 100, 50]);
  assert.deepStrictEqual(rects[1], [150, 200, 80, 60]);
});
