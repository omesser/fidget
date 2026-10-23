import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { cupModel } from "../src/caffeinate-cup.js";

const html = readFileSync(new URL("../src/chat.html", import.meta.url), "utf8");

test("a frame without the field draws no cup", () => {
  assert.equal(cupModel(undefined), null);
  assert.equal(cupModel(null), null);
});

test("the cup is one SVG drawing whose body is what fills", () => {
  const button = html.match(/<button[^>]*id="caffeinate"[^>]*>([\s\S]*?)<\/button>/)[1];
  assert.equal(button.match(/<svg /g).length, 1);
  assert.equal(button.match(/class="cup-body"/g).length, 1);
});

test("off with flags stays a button, and an empty set is disabled with its reason", () => {
  const off = cupModel({ on: false, reason: null });
  assert.deepEqual(off, { disabled: false, on: false, title: "keep my computer awake", label: null });

  const on = cupModel({ on: true, reason: null });
  assert.deepEqual(on, { disabled: false, on: true, title: "keep my computer awake", label: "caffeinated" });

  const reason = "At least one caffeinate flag has to stay on.";
  assert.deepEqual(cupModel({ on: false, reason }), { disabled: true, on: false, title: reason, label: null });
  assert.deepEqual(cupModel({ on: true, reason }), { disabled: true, on: false, title: reason, label: null });
});
