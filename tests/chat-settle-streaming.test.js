// Streaming Speech updates grow a single Chat row; the turn stays open until
// the final reply lands. The Shell sends full answer-so-far, not incremental chunks.

import assert from "node:assert/strict";
import { test } from "node:test";

import { createChatTurns } from "../src/chat-settle.js";

test("streaming Speech keeps the turn open for subsequent updates", () => {
  const turns = createChatTurns();
  const turn = turns.typed();

  const outcomes = [
    turns.settle({ said: "hey", streaming: true }),
    turns.settle({ said: "hey there", streaming: true }),
    turns.settle({ said: "hey there friend", streaming: true }),
  ];

  assert.equal(outcomes[0].action, "speech");
  assert.equal(outcomes[0].turn, turn);
  assert.equal(outcomes[1].action, "speech");
  assert.equal(outcomes[1].turn, turn, "second Speech updates the same turn");
  assert.equal(outcomes[2].action, "speech");
  assert.equal(outcomes[2].turn, turn, "third Speech updates the same turn");
  assert.equal(turns.newest(), turn, "turn remains in waiting queue");
});

test("final reply settles the turn after streaming Speech", () => {
  const turns = createChatTurns();
  const turn = turns.typed();

  turns.settle({ said: "hey", streaming: true });
  turns.settle({ said: "hey there", streaming: true });
  const final = turns.settle({ said: "hey there friend", streaming: false });

  assert.equal(final.action, "speech");
  assert.equal(final.turn, turn);
  assert.equal(turns.newest(), null, "turn is removed from waiting queue");
});

test("streaming Speech on typed turn grows one row", () => {
  const turns = createChatTurns();
  const turn = turns.typed();

  const first = turns.settle({ said: "hey", streaming: true });
  const second = turns.settle({ said: "hey there", streaming: true });
  const final = turns.settle({ said: "hey there friend" });

  assert.equal(first.said, "hey");
  assert.equal(second.said, "hey there");
  assert.equal(final.said, "hey there friend");
  assert.equal(first.turn, turn);
  assert.equal(second.turn, turn);
  assert.equal(final.turn, turn);
});

test("final reply with no said after streaming Speech is silent, not missing", () => {
  const turns = createChatTurns();
  turns.typed();

  turns.settle({ said: "hey", streaming: true });
  const final = turns.settle({ said: null });

  assert.equal(final.action, "silent", "Speech was already drawn");
});

test("streaming Speech with no waiting turn creates a pending proactive turn", () => {
  const turns = createChatTurns();

  const outcome = turns.settle({ said: "hey", streaming: true });

  assert.equal(outcome.action, "speech", "pending proactive turn created for orphan streaming Speech");
  assert.equal(outcome.said, "hey");
  assert.ok(outcome.turn, "turn is tracked");
});

test("proactive reply with streaming Speech (reacting_to set)", () => {
  const turns = createChatTurns();

  const first = turns.settle({ said: "hey", streaming: true });
  const second = turns.settle({ said: "hey there", streaming: true });
  const final = turns.settle({ said: "hey there friend", reacting_to: "proactive" });

  assert.equal(first.action, "speech", "proactive creates one pending turn");
  assert.equal(second.action, "speech", "subsequent Speeches update same turn");
  assert.equal(final.action, "speech", "final settles the proactive turn");
  assert.equal(first.turn, second.turn, "all Speeches grow the same turn");
  assert.equal(second.turn, final.turn, "final settles same turn");
  assert.equal(first.said, "hey");
  assert.equal(second.said, "hey there");
  assert.equal(final.said, "hey there friend");
});

test("multiple typed turns: only the most recent receives streaming Speech", () => {
  const turns = createChatTurns();
  const first = turns.typed();
  const second = turns.typed();

  const outcome = turns.settle({ said: "answer", streaming: true });

  assert.equal(outcome.action, "speech");
  assert.equal(outcome.turn, second, "streaming updates the most recent turn");
  assert.equal(turns.newest(), second, "second turn still waiting");
  assert.notEqual(outcome.turn, first);
});

test("streaming Speech marks turn so final said:null is silent", () => {
  const turns = createChatTurns();
  const turn = turns.typed();

  turns.settle({ said: "partial", streaming: true });
  const final = turns.settle({ said: null });

  assert.equal(final.action, "silent");
  assert.equal(turn.alreadyHasSpeechAhead, true, "turn marked after streaming Speech");
});
