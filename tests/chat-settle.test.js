// An empty `said` after Dialogue has already been drawn is not a missing answer;
// treating it as one is the production change that fails this file. Nor is a
// caret the Shell cancelled, which owes the user the wake that cancelled it.

import assert from "node:assert/strict";
import { test } from "node:test";

import { MISSING_ANSWER, createChatTurns } from "../src/chat-settle.js";

function play(turns, payloads) {
  const log = [];
  for (const payload of payloads) {
    const outcome = turns.settle(payload);
    if (outcome.action === "dialogue") {
      log.push(outcome.said);
    } else if (outcome.action === "missing") {
      log.push(outcome.note);
    } else if (outcome.action === "error") {
      log.push(outcome.note);
    } else if (outcome.action === "notice") {
      log.push(outcome.note);
    } else if (outcome.action === "preempted") {
      log.push(outcome.note);
    }
  }
  return log;
}

test("Dialogue already drawn, then said None, does not report a missing answer", () => {
  const turns = createChatTurns();
  turns.typed();
  turns.typed();

  const log = play(turns, [
    {
      said: "You may! Tell me one thing that would make me a more useful little fidget.",
    },
    { said: null },
  ]);

  assert.deepEqual(log, [
    "You may! Tell me one thing that would make me a more useful little fidget.",
  ]);
  assert.equal(
    log.includes(MISSING_ANSWER),
    false,
    "an empty settle after Dialogue landed is not a missing answer",
  );
});

test("a turn that produced no Dialogue still reports a missing answer", () => {
  const turns = createChatTurns();
  turns.typed();

  const log = play(turns, [{ said: null }]);

  assert.deepEqual(log, [MISSING_ANSWER]);
});

test("a later turn that produced no Dialogue still reports a missing answer", () => {
  const turns = createChatTurns();
  turns.typed();
  play(turns, [{ said: "You may!" }]);
  turns.typed();

  const log = play(turns, [{ said: null }]);

  assert.deepEqual(log, [MISSING_ANSWER]);
});

test("a preempted caret says what preempted it, not that an answer is missing", () => {
  const turns = createChatTurns();
  turns.typed();

  const log = play(turns, [{ said: null, superseded_by: "poked" }]);

  assert.deepEqual(log, ["You poked me before that answer landed, so I dropped it."]);
  assert.equal(
    log.includes(MISSING_ANSWER),
    false,
    "the Shell already said why the caret was cancelled",
  );
});

test("the note names which wake took the slot", () => {
  const turns = createChatTurns();
  turns.typed();
  turns.typed();

  const log = play(turns, [
    { said: null, superseded_by: "poked" },
    { said: null, superseded_by: "spoken to" },
  ]);

  assert.deepEqual(log, [
    "You poked me before that answer landed, so I dropped it.",
    "You asked something else before that answer landed, so I dropped it.",
  ]);
});

test("a cause with no clause of its own still reads as a sentence", () => {
  const turns = createChatTurns();
  turns.typed();

  const log = play(turns, [{ said: null, superseded_by: "levitated" }]);

  assert.deepEqual(log, ["Something else started before that answer landed, so I dropped it."]);
});

test("a Harness error is the Shell's line as sent, under no second prefix", () => {
  const turns = createChatTurns();
  turns.typed();

  const log = play(turns, [
    { said: null, error: "harness not authenticated: run `claude /login`" },
  ]);

  assert.deepEqual(log, ["harness not authenticated: run `claude /login`"]);
});

test("an error with no waiting turn is orphan, not a note", () => {
  const turns = createChatTurns();
  const outcome = turns.settle({
    said: null,
    error: "harness not authenticated: run `claude /login`",
  });
  assert.equal(outcome.action, "orphan");
  assert.equal(outcome.turn, null);
});

test("an attach-time notice with no caret is still a note that outlives a later turn", () => {
  const turns = createChatTurns();
  const attach = turns.settle({
    notice: "Codex is on `gpt-6-sol`, which is not in its available models.",
  });
  assert.equal(attach.action, "notice");
  assert.equal(attach.turn, null);
  assert.match(attach.note, /gpt-6-sol/);

  turns.typed();
  const after = turns.settle({ said: "Hello" });
  assert.equal(after.action, "dialogue");
  assert.equal(after.said, "Hello");
});

// Attach opens inside the first turn, so typed() lands before the notice.
test("a notice after typed() leaves the waiting turn for the reply", () => {
  const turns = createChatTurns();
  turns.typed();

  const notice = turns.settle({
    notice: "Codex is on `gpt-6-sol`, which is not in its available models.",
  });
  assert.equal(notice.action, "notice");
  assert.equal(notice.turn, null);
  assert.match(notice.note, /gpt-6-sol/);

  const reply = turns.settle({ said: "Hello" });
  assert.equal(reply.action, "dialogue");
  assert.equal(reply.said, "Hello");
  assert.ok(reply.turn, "the typed turn must still be waiting for Hello");
});
