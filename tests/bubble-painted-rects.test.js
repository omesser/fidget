import { test } from "node:test";
import assert from "node:assert/strict";
import { computeBubblePaintedRect, createPaintedReporter } from "../src/painted-rects.js";

const bubble = (visible, opacity) => ({
  classList: { contains: (cls) => visible && cls === "visible" },
  style: opacity === undefined ? {} : { opacity },
  offsetWidth: 200,
  offsetHeight: 80,
});

// What Windows must keep in the overlay's region for the bubble: its body and
// the 10px tail, rounded to pixels. A bubble that cannot be read keeps nothing.
test("the painted rect is the readable bubble plus its tail", () => {
  const rows = [
    ["hidden", bubble(false), { x: 1, y: 2 }, null],
    ["fading out", bubble(true, "0.4"), { x: 10, y: 20 }, null],
    ["opaque, tail down", bubble(true, "1"), { x: 10.4, y: 20.6 }, [10, 21, 200, 90]],
    ["no opacity set yet", bubble(true), { x: 100.7, y: 200.3 }, [101, 200, 200, 90]],
    [
      "inverted, tail up",
      bubble(true, "1"),
      { x: 10.4, y: 20.6, inverted: true },
      [10, 11, 200, 90],
    ],
  ];
  for (const [name, element, pos, expected] of rows) {
    assert.deepEqual(computeBubblePaintedRect(element, pos), expected, name);
  }
});

test("every view's painted rect reaches the Shell in one call, once per change", () => {
  const told = [];
  const report = createPaintedReporter((command, args) => {
    told.push([command, args]);
    return Promise.resolve();
  });
  const views = new Map([
    [1, { paintedRect: [10, 20, 100, 50] }],
    [2, { paintedRect: null }],
    [3, { paintedRect: [200, 300, 150, 75] }],
  ]);

  report(views);
  report(views);
  assert.deepEqual(told, [
    [
      "overlay_painted_rects",
      {
        rects: [
          [10, 20, 100, 50],
          [200, 300, 150, 75],
        ],
      },
    ],
  ]);

  views.get(1).paintedRect = null;
  views.get(3).paintedRect = null;
  report(views);
  assert.deepEqual(told.at(-1), ["overlay_painted_rects", { rects: [] }], "an empty list clears");
});
