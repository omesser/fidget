// Streaming Speech updates grow a single Chat row; the turn stays open until
// the final reply lands. The Shell sends full answer-so-far, not incremental chunks.

import assert from "node:assert/strict";
import { test } from "node:test";

import { createChatTurns } from "../src/chat-settle.js";
import { replaceReply } from "../src/markdown.js";

class Node {
  constructor() {
    this.parentNode = null;
  }

  remove() {
    const siblings = this.parentNode?.childNodes;
    const at = siblings ? siblings.indexOf(this) : -1;
    assert.ok(at >= 0, "remove() was called on a node that is not in its parent");
    siblings.splice(at, 1);
    this.parentNode = null;
  }
}

class Text extends Node {
  constructor(text) {
    super();
    this.nodeType = 3;
    this.data = String(text);
  }

  get textContent() {
    return this.data;
  }
}

class Element extends Node {
  constructor(tag) {
    super();
    this.nodeType = 1;
    this.tagName = tag.toUpperCase();
    this.childNodes = [];
    this.className = "";
    this.dataset = {};
  }

  append(...nodes) {
    for (const node of nodes) {
      this.#adopt(node, this.childNodes.length);
    }
  }

  replaceChildren(...nodes) {
    for (const node of this.childNodes) {
      node.parentNode = null;
    }
    this.childNodes = [];
    this.append(...nodes);
  }

  #adopt(node, at) {
    if (node.parentNode) {
      node.remove();
    }
    node.parentNode = this;
    this.childNodes.splice(at, 0, node);
  }

  get children() {
    return this.childNodes.filter((node) => node.nodeType === 1);
  }

  get lastElementChild() {
    const kids = this.children;
    return kids.length > 0 ? kids[kids.length - 1] : null;
  }

  get textContent() {
    return this.childNodes.map((node) => node.textContent).join("");
  }

  set textContent(text) {
    this.replaceChildren(new Text(text));
  }

  querySelector(selector) {
    const wanted = selector.replace(/^\./, "");
    for (const kid of this.children) {
      if (kid.className.split(/\s+/).includes(wanted)) {
        return kid;
      }
      const deeper = kid.querySelector(selector);
      if (deeper) {
        return deeper;
      }
    }
    return null;
  }

  createElement(tag) {
    return new Element(tag);
  }

  createTextNode(text) {
    return new Text(text);
  }
}

const doc = {
  createElement: (tag) => new Element(tag),
  createTextNode: (text) => new Text(text),
};

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

test("drawn text: stream→final produces correct final string, not doubled", () => {
  const body = doc.createElement("div");
  body.className = "said md";

  replaceReply(body, "hey", doc);
  assert.equal(body.textContent, "hey");

  replaceReply(body, "hey there", doc);
  assert.equal(body.textContent, "hey there");

  replaceReply(body, "hey there friend", doc);
  const finalText = body.textContent;
  assert.equal(finalText, "hey there friend", "final text matches last streaming chunk");

  replaceReply(body, "hey there friend", doc);
  assert.equal(body.textContent, finalText, "final non-streaming replace does not double text");
  assert.ok(!body.textContent.includes("hey therehey there friend"), "text not appended, replaced");
});

test("drawn text: proactive streaming produces one growing row", () => {
  const turns = createChatTurns();
  const body = doc.createElement("div");
  body.className = "said md";

  const first = turns.settle({ said: "thinking", streaming: true });
  replaceReply(body, first.said, doc);
  const firstText = body.textContent;
  assert.equal(firstText, "thinking");

  const second = turns.settle({ said: "thinking about it", streaming: true });
  replaceReply(body, second.said, doc);
  const secondText = body.textContent;
  assert.equal(secondText, "thinking about it");

  const final = turns.settle({ said: "thinking about it more", reacting_to: "proactive" });
  replaceReply(body, final.said, doc);
  const finalText = body.textContent;
  assert.equal(finalText, "thinking about it more");

  assert.equal(first.turn, second.turn, "all updates target same turn");
  assert.equal(second.turn, final.turn, "final settles same turn");
  assert.notEqual(finalText, firstText + secondText, "text replaced not appended");
});

test("drawn text: caret stays during streaming, removed on final", () => {
  const body = doc.createElement("div");
  body.className = "said md";
  const caret = doc.createElement("span");
  caret.className = "caret";
  caret.textContent = "▍";
  body.append(caret);

  replaceReply(body, "hey", doc);
  assert.ok(body.querySelector(".caret"), "caret stays during first streaming chunk");
  assert.match(body.textContent, /hey/, "text drawn");

  replaceReply(body, "hey there", doc);
  assert.ok(body.querySelector(".caret"), "caret stays during second streaming chunk");

  replaceReply(body, "hey there friend", doc);
  assert.ok(body.querySelector(".caret"), "caret stays during third streaming chunk");

  body.querySelector(".caret").remove();
  assert.equal(body.querySelector(".caret"), null, "caret removed on final");
  assert.equal(body.textContent, "hey there friend", "final text without caret");
});

test("drawn text: failed turn after streaming keeps partial", () => {
  const turns = createChatTurns();
  const turn = turns.typed();
  const body = doc.createElement("div");
  body.className = "said md";
  const caret = doc.createElement("span");
  caret.className = "caret";
  body.append(caret);

  const speech = turns.settle({ said: "partial answer", streaming: true });
  replaceReply(body, speech.said, doc);
  assert.equal(body.textContent.trim(), "partial answer", "partial text drawn");
  assert.ok(turn.alreadyHasSpeechAhead, "turn marked with Speech");

  const failure = turns.settle({ failure: "connection lost" });
  assert.equal(failure.action, "failure");

  caret.remove();
  assert.equal(body.textContent, "partial answer", "partial text kept after failure");
});

test("QM pill: stream→final produces one row, no stuck caret", () => {
  const turns = createChatTurns();
  const body = doc.createElement("div");
  body.className = "said md";
  const caret = doc.createElement("span");
  caret.className = "caret";
  body.append(caret);

  const first = turns.settle({ said: "thinking", streaming: true });
  assert.equal(first.action, "speech", "orphan Speech creates proactivePending");
  replaceReply(body, first.said, doc);
  assert.equal(body.textContent.trim(), "thinking");

  const second = turns.settle({ said: "thinking about it", streaming: true });
  assert.equal(second.action, "speech", "subsequent Speech updates same turn");
  assert.equal(second.turn, first.turn, "same turn tracked");
  replaceReply(body, second.said, doc);
  assert.equal(body.textContent.trim(), "thinking about it");

  const final = turns.settle({ said: "thinking about it more", reacting_to: null });
  assert.equal(final.action, "speech", "final settles proactivePending even without reacting_to");
  assert.equal(final.turn, first.turn, "final settles same turn");
  replaceReply(body, final.said, doc);
  caret.remove();
  assert.equal(body.textContent, "thinking about it more", "one row with final text");
  assert.equal(turns.newest(), null, "turn settled, no stuck caret in waiting");
});

test("proactive stream superseded by typed turn: proactive row removed", () => {
  const turns = createChatTurns();

  const proactiveSpeech = turns.settle({ said: "thinking", streaming: true });
  assert.equal(proactiveSpeech.action, "speech", "proactive Speech creates pending turn");
  const proactiveTurn = proactiveSpeech.turn;

  const typedTurn = turns.typed();
  assert.notEqual(turns.newest(), proactiveTurn, "proactive turn removed when typed turn pushed");
  assert.equal(turns.newest(), typedTurn, "typed turn is now newest");

  const final = turns.settle({ said: "answer to typed question" });
  assert.equal(final.action, "speech", "typed turn gets the final");
  assert.equal(final.turn, typedTurn, "final targets typed turn");
  assert.equal(turns.newest(), null, "typed turn settled");
});
