import assert from "node:assert/strict";
import { test } from "node:test";

import { cupModel } from "../src/caffeinate-cup.js";

test("a frame without the field draws no cup", () => {
  assert.equal(cupModel(undefined), null);
  assert.equal(cupModel(null), null);
});

test("off with flags stays a button, and an empty set is disabled with its reason", () => {
  const off = cupModel({ on: false, flags: "-dims", reason: null });
  assert.equal(off.disabled, false);
  assert.equal(off.on, false);
  assert.match(off.svg, /xmlns="http:\/\/www\.w3\.org\/2000\/svg"/);
  assert.match(off.svg, /class="cup-body"/);

  const on = cupModel({ on: true, flags: "-dims", reason: null });
  assert.equal(on.svg, off.svg, "on and off are one drawing");
  assert.equal(on.on, true);
  assert.equal(on.disabled, false);
  assert.equal(off.title, "keep my computer awake");
  assert.equal(on.title, "keep my computer awake");

  const empty = cupModel({
    on: false,
    flags: null,
    reason: "At least one caffeinate flag has to stay on.",
  });
  assert.equal(empty.disabled, true);
  assert.equal(empty.on, false);
  assert.equal(empty.title, "At least one caffeinate flag has to stay on.");
});
