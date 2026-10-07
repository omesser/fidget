import { test } from "node:test";
import assert from "node:assert/strict";
import { computeBubblePaintedRect, reportPaintedRects, clearCache } from "../src/painted-rects.js";

test("computeBubblePaintedRect returns null when bubble is hidden", () => {
  const bubble = {
    classList: {
      contains: () => false,
    },
  };
  const pos = { x: 100, y: 200 };
  assert.equal(computeBubblePaintedRect(bubble, pos), null);
});

test("computeBubblePaintedRect returns null during fade out", () => {
  const bubble = {
    classList: {
      contains: (cls) => cls === "visible",
    },
    style: {
      opacity: "0.5",
    },
    offsetWidth: 300,
    offsetHeight: 150,
  };
  const pos = { x: 100, y: 200 };
  assert.equal(computeBubblePaintedRect(bubble, pos), null);
});

test("computeBubblePaintedRect returns rect when fully visible", () => {
  const bubble = {
    classList: {
      contains: (cls) => cls === "visible",
    },
    style: {
      opacity: "1",
    },
    offsetWidth: 300,
    offsetHeight: 150,
  };
  const pos = { x: 100, y: 200 };
  const rect = computeBubblePaintedRect(bubble, pos);
  assert.deepEqual(rect, [100, 200, 300, 150]);
});

test("computeBubblePaintedRect rounds position", () => {
  const bubble = {
    classList: {
      contains: (cls) => cls === "visible",
    },
    style: {},
    offsetWidth: 300,
    offsetHeight: 150,
  };
  const pos = { x: 100.7, y: 200.3 };
  const rect = computeBubblePaintedRect(bubble, pos);
  assert.deepEqual(rect, [101, 200, 300, 150]);
});

test("reportPaintedRects sends empty array when no views have painted rects", () => {
  clearCache();
  const views = new Map();
  views.set(1, { paintedRect: null });
  views.set(2, { paintedRect: null });
  
  let invokedWith = null;
  const invoke = (cmd, args) => {
    invokedWith = { cmd, args };
    return Promise.resolve();
  };

  reportPaintedRects(views, invoke);
  assert.equal(invokedWith.cmd, "overlay_painted_rects");
  assert.deepEqual(invokedWith.args.rects, []);
});

test("reportPaintedRects batches multiple view rects", () => {
  clearCache();
  const views = new Map();
  views.set(1, { paintedRect: [10, 20, 100, 50] });
  views.set(2, { paintedRect: [200, 300, 150, 75] });
  views.set(3, { paintedRect: null });
  
  let invokedWith = null;
  const invoke = (cmd, args) => {
    invokedWith = { cmd, args };
    return Promise.resolve();
  };

  reportPaintedRects(views, invoke);
  assert.equal(invokedWith.cmd, "overlay_painted_rects");
  assert.deepEqual(invokedWith.args.rects, [
    [10, 20, 100, 50],
    [200, 300, 150, 75],
  ]);
});

test("reportPaintedRects skips duplicate sends", () => {
  clearCache();
  const views = new Map();
  views.set(1, { paintedRect: [10, 20, 100, 50] });
  
  let invokeCount = 0;
  const invoke = () => {
    invokeCount++;
    return Promise.resolve();
  };

  reportPaintedRects(views, invoke);
  assert.equal(invokeCount, 1);

  reportPaintedRects(views, invoke);
  assert.equal(invokeCount, 1, "second call with same rects should not invoke");
});

test("reportPaintedRects sends update when rects change", () => {
  clearCache();
  const views = new Map();
  views.set(1, { paintedRect: [10, 20, 100, 50] });
  
  let invokeCount = 0;
  const invoke = () => {
    invokeCount++;
    return Promise.resolve();
  };

  reportPaintedRects(views, invoke);
  assert.equal(invokeCount, 1);

  views.get(1).paintedRect = [10, 20, 200, 100];
  reportPaintedRects(views, invoke);
  assert.equal(invokeCount, 2, "changed rects should trigger another invoke");
});

test("reportPaintedRects handles invoke errors", () => {
  clearCache();
  const views = new Map();
  views.set(1, { paintedRect: [10, 20, 100, 50] });
  
  const invoke = () => Promise.reject(new Error("test error"));

  // Should not throw
  reportPaintedRects(views, invoke);
});
