// The clock here is the test's, not the window's.

import assert from "node:assert/strict";
import { test } from "node:test";

import { STALL_MS, idlePhase, phaseLine, reducePhase } from "../src/turn-phase.js";

const stall = `No news for ${STALL_MS / 1000}s`;

function at(events, now) {
  let state = idlePhase();
  for (const event of events) {
    state = reducePhase(state, event);
  }
  return phaseLine(state, now);
}

test("a turn with no tool call is waiting", () => {
  assert.equal(at([{ type: "open", at: 0 }], 0), "Waiting");
});

test("a tool call in progress names its title", () => {
  assert.equal(
    at(
      [
        { type: "open", at: 0 },
        { type: "tool", at: 10, id: "t1", title: "Read file", status: "in_progress" },
      ],
      10,
    ),
    "Running: Read file",
  );
});

test("the latest call still running is the one the line names", () => {
  assert.equal(
    at(
      [
        { type: "open", at: 0 },
        { type: "tool", at: 1, id: "t1", title: "Read file", status: "completed" },
        { type: "tool", at: 2, id: "t2", title: "cargo test", status: "in_progress" },
      ],
      2,
    ),
    "Running: cargo test",
  );
});

test("a call that has not started leaves the turn waiting", () => {
  assert.equal(
    at(
      [
        { type: "open", at: 0 },
        { type: "tool", at: 1, id: "t1", title: "cargo test", status: "pending" },
      ],
      1,
    ),
    "Waiting",
  );
});

test("a finished call leaves the turn waiting", () => {
  assert.equal(
    at(
      [
        { type: "open", at: 0 },
        { type: "tool", at: 1, id: "t1", title: "Read file", status: "in_progress" },
        { type: "tool", at: 2, id: "t1", status: "completed" },
      ],
      2,
    ),
    "Waiting",
  );
});

test("silence for the stall threshold says so, and any update clears it", () => {
  const open = [
    { type: "open", at: 0 },
    { type: "tool", at: 0, id: "t1", title: "Read file", status: "in_progress" },
  ];
  assert.equal(at(open, STALL_MS), stall);
  assert.equal(at([...open, { type: "news", at: STALL_MS }], STALL_MS), "Running: Read file");
});

test("a typed turn drops calls that arrived while nothing was open", () => {
  assert.equal(
    at(
      [
        { type: "tool", at: 1, id: "t1", title: "Read file", status: "in_progress" },
        { type: "open", at: 2, fresh: true },
      ],
      2,
    ),
    "Waiting",
  );
});

test("a call that arrives before the turn opens is named once it does", () => {
  assert.equal(
    at(
      [
        { type: "tool", at: 1, id: "t1", title: "Read file", status: "in_progress" },
        { type: "open", at: 2 },
      ],
      2,
    ),
    "Running: Read file",
  );
});

test("a closed turn draws no line", () => {
  assert.equal(
    at(
      [
        { type: "open", at: 0 },
        { type: "tool", at: 1, id: "t1", title: "Read file", status: "in_progress" },
        { type: "close" },
      ],
      STALL_MS,
    ),
    "",
  );
});
