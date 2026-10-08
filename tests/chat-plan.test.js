// The plan above the composer is the agent's steps and which one is current
// (#697). The marking is the wire's `status`, not anything decided here.

import assert from "node:assert/strict";
import { test } from "node:test";

import { planSteps } from "../src/chat-plan.js";

test("the in-progress step is the one marked current", () => {
  const steps = planSteps([
    { content: "read the roster", priority: "high", status: "completed" },
    { content: "write the patch", priority: "medium", status: "in_progress" },
    { content: "run the tests", priority: "low", status: "pending" },
  ]);

  assert.deepEqual(
    steps.map((step) => step.status),
    ["completed", "in_progress", "pending"],
  );
  assert.equal(steps[1].text, "write the patch");
  // Carried rather than drawn: no CSS rule reads it yet, and dropping it here
  // would be a fresh gap of the kind ADR-0028 is about.
  assert.deepEqual(
    steps.map((step) => step.priority),
    ["high", "medium", "low"],
  );
});

test("an empty plan draws nothing", () => {
  assert.deepEqual(planSteps([]), []);
  assert.deepEqual(planSteps(undefined), []);
});

// A step as the Harness wrote it, the rule the ask card follows: its lines and
// length stay, and only the characters that could hide or reverse text go.
test("a step keeps its lines and length and loses only unsafe characters", () => {
  const long = "Move the roster parser behind the settings boundary. ".repeat(4).trim();
  assert.equal(long.length, 211);
  const [step] = planSteps([
    { content: `write the\n\u202epatch\u2066\n  - then ${long}`, status: "pending" },
  ]);
  assert.equal(step.text, `write the\npatch\n  - then ${long}`);
});
