import assert from "node:assert/strict";
import { test } from "node:test";

import { createComposerRecall } from "../src/chat-recall.js";

function field(value, start = value.length, end = start) {
  return {
    value,
    selectionStart: start,
    selectionEnd: end,
    clientWidth: 0,
    ownerDocument: null,
    setSelectionRange(a, b) {
      this.selectionStart = a;
      this.selectionEnd = b;
    },
  };
}

function row(text, classes = ["row", "you"]) {
  const said = { textContent: text };
  const names = new Set(classes);
  return {
    classList: {
      contains(name) {
        return names.has(name);
      },
    },
    querySelector(selector) {
      return selector === ".said" ? said : null;
    },
  };
}

function arrow(key, extra = {}) {
  return {
    key,
    shiftKey: false,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    isComposing: false,
    defaultPrevented: false,
    ...extra,
    preventDefault() {
      this.defaultPrevented = true;
    },
  };
}

function press(recall, key, box, extra) {
  const event = arrow(key, extra);
  const consumed = recall.consume(event, box);
  assert.equal(event.defaultPrevented, consumed);
  return consumed;
}

test("Up and Down walk user rows and restore the draft", () => {
  const alpha = row("alpha");
  const beta = row("beta");
  const log = { children: [alpha, row("skip", ["row", "them"]), beta] };
  const recall = createComposerRecall(log);
  const box = field("draft", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "alpha");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "alpha");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);

  assert.equal(press(recall, "ArrowDown", box), true);
  assert.equal(box.value, "beta");
  assert.equal(box.selectionStart, 4);
  assert.equal(box.selectionEnd, 4);

  assert.equal(press(recall, "ArrowDown", box), true);
  assert.equal(box.value, "draft");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});

test("Down restores a draft caret that was not at 0", () => {
  const log = { children: [row("beta")] };
  const recall = createComposerRecall(log);
  const box = field("draft", 2, 2);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);

  assert.equal(press(recall, "ArrowDown", box), true);
  assert.equal(box.value, "draft");
  assert.equal(box.selectionStart, 2);
  assert.equal(box.selectionEnd, 2);
});

test("Up on a later logical line leaves the value", () => {
  const log = { children: [row("beta")] };
  const recall = createComposerRecall(log);
  const box = field("line1\nline2", "line1\n".length);

  assert.equal(press(recall, "ArrowUp", box), false);
  assert.equal(box.value, "line1\nline2");
  assert.equal(box.selectionStart, 6);
  assert.equal(box.selectionEnd, 6);
});

test("Up on the first logical line yields the newest row", () => {
  const log = { children: [row("alpha"), row("beta")] };
  const recall = createComposerRecall(log);
  const box = field("line1\nline2", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});

test("modified arrows and other keys leave the field", () => {
  let reads = 0;
  const log = {
    get children() {
      reads += 1;
      throw new Error("log read");
    },
  };
  const recall = createComposerRecall(log);
  const extras = [
    { shiftKey: true },
    { ctrlKey: true },
    { altKey: true },
    { metaKey: true },
    { isComposing: true },
  ];
  for (const extra of extras) {
    const box = field("draft", 0);
    assert.equal(press(recall, "ArrowUp", box, extra), false);
    assert.equal(box.value, "draft");
    assert.equal(box.selectionStart, 0);
    assert.equal(box.selectionEnd, 0);
  }
  const box = field("draft", 0);
  assert.equal(press(recall, "Enter", box), false);
  assert.equal(box.value, "draft");
  assert.equal(box.selectionStart, 0);
  assert.equal(reads, 0);
});

test("a non-collapsed selection is not an edge", () => {
  const log = { children: [row("alpha"), row("beta")] };
  const recall = createComposerRecall(log);
  const box = field("draft", 1, 4);

  assert.equal(press(recall, "ArrowUp", box), false);
  assert.equal(box.value, "draft");
  assert.equal(box.selectionStart, 1);
  assert.equal(box.selectionEnd, 4);

  assert.equal(press(recall, "ArrowDown", box), false);
  assert.equal(box.value, "draft");
  assert.equal(box.selectionStart, 1);
  assert.equal(box.selectionEnd, 4);
});

test("editing the field does not write the row", () => {
  const alpha = row("alpha");
  const beta = row("beta");
  const log = { children: [alpha, beta] };
  const recall = createComposerRecall(log);
  const box = field("draft", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  box.value = "edited";
  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "alpha");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
  assert.equal(beta.querySelector(".said").textContent, "beta");
  assert.equal(alpha.querySelector(".said").textContent, "alpha");
});

test("two rows with the same text are two steps", () => {
  const log = { children: [row("ok"), row("ok")] };
  const recall = createComposerRecall(log);
  const box = field("draft", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "ok");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "ok");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);

  assert.equal(press(recall, "ArrowDown", box), true);
  assert.equal(box.value, "ok");
  assert.equal(box.selectionStart, 2);
  assert.equal(box.selectionEnd, 2);

  assert.equal(press(recall, "ArrowDown", box), true);
  assert.equal(box.value, "draft");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});

test("an empty row is a real step", () => {
  const log = { children: [row("alpha"), row("")] };
  const recall = createComposerRecall(log);
  const box = field("draft", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "alpha");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});

test("row text keeps its spaces", () => {
  const log = { children: [row(" beta ")] };
  const recall = createComposerRecall(log);
  const box = field("draft", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, " beta ");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});

test("release starts the next Up at the newest row", () => {
  const alpha = row("alpha");
  const beta = row("beta");
  const gamma = row("gamma");
  const log = { children: [alpha, beta] };
  const recall = createComposerRecall(log);
  const box = field("draft", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  log.children = [alpha, beta, gamma];
  box.value = "";
  box.setSelectionRange(0, 0);
  recall.release();
  assert.equal(box.value, "");
  assert.equal(box.selectionStart, 0);
  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "gamma");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});

test("a detached row restores the draft and the next Up walks", () => {
  const alpha = row("alpha");
  const beta = row("beta");
  const log = { children: [alpha, beta] };
  const recall = createComposerRecall(log);
  const box = field("hello", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  assert.equal(box.selectionStart, 0);
  log.children = [alpha];

  assert.equal(press(recall, "ArrowUp", box), false);
  assert.equal(box.value, "hello");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "alpha");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);

  assert.equal(press(recall, "ArrowDown", box), true);
  assert.equal(box.value, "hello");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});

test("a detached row restores a multiline draft and leaves the Down key", () => {
  const beta = row("beta");
  const log = { children: [beta] };
  const recall = createComposerRecall(log);
  const box = field("line1\nline2", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  log.children = [];

  assert.equal(press(recall, "ArrowDown", box), false);
  assert.equal(box.value, "line1\nline2");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});

test("release then an empty log does not write a previous line", () => {
  const log = { children: [row("alpha"), row("beta")] };
  const recall = createComposerRecall(log);
  const box = field("hello", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  recall.release();
  log.children = [];
  box.value = "kept";
  box.setSelectionRange(4, 4);

  assert.equal(press(recall, "ArrowUp", box), false);
  assert.equal(box.value, "kept");
  assert.equal(box.selectionStart, 4);
  assert.equal(box.selectionEnd, 4);
});

test("Down on the draft leaves the field", () => {
  const log = { children: [row("alpha"), row("beta")] };
  const recall = createComposerRecall(log);
  const atEnd = field("draft");

  assert.equal(press(recall, "ArrowDown", atEnd), false);
  assert.equal(atEnd.value, "draft");
  assert.equal(atEnd.selectionStart, 5);
  assert.equal(atEnd.selectionEnd, 5);

  const atStart = field("draft", 0);
  assert.equal(press(recall, "ArrowDown", atStart), false);
  assert.equal(atStart.value, "draft");
  assert.equal(atStart.selectionStart, 0);
  assert.equal(atStart.selectionEnd, 0);
});

test("a detached row restores the draft caret and leaves the key", () => {
  const beta = row("beta");
  const log = { children: [beta] };
  const recall = createComposerRecall(log);
  const box = field("hello", 5);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  log.children = [];

  assert.equal(press(recall, "ArrowDown", box), false);
  assert.equal(box.value, "hello");
  assert.equal(box.selectionStart, 5);
  assert.equal(box.selectionEnd, 5);
});

test("Up after the last row leaves the restored draft", () => {
  const beta = row("beta");
  const log = { children: [beta] };
  const recall = createComposerRecall(log);
  const box = field("hello", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  log.children = [];

  assert.equal(press(recall, "ArrowUp", box), false);
  assert.equal(box.value, "hello");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});

test("release twice then Up walks the rows on the log", () => {
  const log = { children: [row("alpha"), row("beta")] };
  const recall = createComposerRecall(log);
  const box = field("draft", 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  recall.release();
  recall.release();
  box.value = "fresh";
  box.setSelectionRange(0, 0);

  assert.equal(press(recall, "ArrowUp", box), true);
  assert.equal(box.value, "beta");
  assert.equal(box.selectionStart, 0);
  assert.equal(box.selectionEnd, 0);
});
