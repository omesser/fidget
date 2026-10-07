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

test("streaming Speech with no waiting turn is orphan", () => {
  const turns = createChatTurns();

  const outcome = turns.settle({ said: "hey", streaming: true });

  assert.equal(outcome.action, "orphan");
  assert.equal(outcome.said, "hey");
});

test("proactive reply with streaming Speech (reacting_to set)", () => {
  const turns = createChatTurns();

  const first = turns.settle({ said: "hey", streaming: true, reacting_to: "proactive" });
  const second = turns.settle({ said: "hey there", streaming: true, reacting_to: "proactive" });
  const final = turns.settle({ said: "hey there friend", reacting_to: "proactive" });

  assert.equal(first.action, "orphan", "proactive has no typed turn");
  assert.equal(second.action, "orphan");
  assert.equal(final.action, "orphan");
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
