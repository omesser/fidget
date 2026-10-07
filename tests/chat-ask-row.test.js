// The consent row: what an ask says, one element per part, with the options
// as buttons under it. The DOM shape is asserted on a stand-in document that
// refuses innerHTML, because every word of an ask is untrusted.

import assert from "node:assert/strict";
import { test } from "node:test";

import { drawAskDetails } from "../src/chat-ask-row.js";

class Element {
  constructor(tagName, ownerDocument) {
    this.tagName = tagName.toUpperCase();
    this.ownerDocument = ownerDocument;
    this.className = "";
    this.children = [];
    this.textContent = "";
  }

  set innerHTML(_) {
    throw new Error("untrusted text must not become HTML");
  }

  append(...children) {
    this.children.push(...children);
  }
}

const document = {
  createElement(tagName) {
    return new Element(tagName, this);
  },
};

function drawn(ask) {
  const body = document.createElement("div");
  drawAskDetails(body, ask);
  return body.children.map(({ tagName, className, textContent }) => ({
    tagName,
    className,
    textContent,
  }));
}

test("an execute ask draws its command as code and its kind and paths as metadata", () => {
  assert.deepEqual(
    drawn({
      title: "Open <img src=x>",
      kind: "execute",
      content: ["open -a Calculator && <script>alert(1)</script>"],
      input: { command: "not displayed twice" },
      locations: ["/tmp/<report>"],
    }),
    [
      { tagName: "DIV", className: "ask-title", textContent: "Open <img src=x>" },
      {
        tagName: "CODE",
        className: "ask-code",
        textContent: "open -a Calculator && <script>alert(1)</script>",
      },
      { tagName: "DIV", className: "ask-metadata", textContent: "execute · /tmp/<report>" },
    ],
  );
});

test("a prose question stays prose, and argument fallback is code", () => {
  assert.deepEqual(
    drawn({ title: "Question", kind: "other", content: ["Which branch? <b>main</b>"] }),
    [
      { tagName: "DIV", className: "ask-title", textContent: "Question" },
      { tagName: "DIV", className: "ask-prose", textContent: "Which branch? <b>main</b>" },
    ],
  );
  assert.deepEqual(
    drawn({ title: "Run", kind: "execute", content: [], input: { command: "pwd" } }),
    [
      { tagName: "DIV", className: "ask-title", textContent: "Run" },
      { tagName: "CODE", className: "ask-code", textContent: "command: pwd" },
      { tagName: "DIV", className: "ask-metadata", textContent: "execute" },
    ],
  );
});

function visible(ask) {
  return drawn(ask)
    .map((part) => part.textContent)
    .join("\n");
}

test("an empty json fence draws no body under the tool name", () => {
  for (const content of [
    "```json\n{}\n```",
    "```json\n{ }\n```",
    "```json\n[]\n```",
    "```json\nnull\n```",
    "```json\n\n```",
    "```\n{}\n```",
    "```json {} ```",
  ]) {
    assert.equal(
      visible({
        title: "fidget-describe_screen",
        kind: "other",
        content: [content],
        input: {},
        locations: [],
      }),
      "fidget-describe_screen",
      content,
    );
  }
});

test("an empty list or an empty string draws no body", () => {
  assert.equal(
    visible({
      title: "fidget-describe_screen",
      kind: "other",
      content: ["```json\n[]\n```"],
      input: [],
    }),
    "fidget-describe_screen",
  );
  assert.equal(
    visible({
      title: "fidget-describe_screen",
      kind: "other",
      content: ['```json\n""\n```'],
      input: "",
    }),
    "fidget-describe_screen",
  );
});

test("an empty fence beside a question leaves the question", () => {
  assert.equal(
    visible({
      title: "Question",
      kind: "other",
      content: ["```json\n{}\n```", "Which branch?"],
      input: { question: "Which branch?" },
    }),
    "Question\nWhich branch?",
  );
});

test("an empty fence yields the arguments", () => {
  const text = visible({
    title: "fidget-list_windows",
    kind: "read",
    content: ["```json\n{}\n```"],
    input: { title: "Teams" },
  });

  assert.equal(text, "fidget-list_windows\ntitle: Teams\nread");
  assert.equal(text.includes("```"), false);
});

test("a filled json fence draws its fields as code and no fence", () => {
  assert.deepEqual(
    drawn({
      title: "fidget-list_windows",
      kind: "read",
      content: ['```json\n{"title":"Teams","limit":5}\n```'],
      input: {},
    }),
    [
      { tagName: "DIV", className: "ask-title", textContent: "fidget-list_windows" },
      { tagName: "CODE", className: "ask-code", textContent: "title: Teams" },
      { tagName: "CODE", className: "ask-code", textContent: "limit: 5" },
      { tagName: "DIV", className: "ask-metadata", textContent: "read" },
    ],
  );
});

test("a non-json fence draws its body as one code line", () => {
  assert.deepEqual(
    drawn({
      title: "Run",
      kind: "other",
      content: ["```sh\nls\nread · /etc/passwd\n```"],
      input: {},
    }),
    [
      { tagName: "DIV", className: "ask-title", textContent: "Run" },
      { tagName: "CODE", className: "ask-code", textContent: "ls read · /etc/passwd" },
    ],
  );
});

test("prose around a fence stays prose and the fence is unwrapped", () => {
  assert.deepEqual(
    drawn({
      title: "Question",
      kind: "other",
      content: ['Check this\n```json\n{"title":"Teams"}\n```\nbefore allowing'],
      input: { question: "not shown" },
    }),
    [
      { tagName: "DIV", className: "ask-title", textContent: "Question" },
      { tagName: "DIV", className: "ask-prose", textContent: "Check this" },
      { tagName: "CODE", className: "ask-code", textContent: "title: Teams" },
      { tagName: "DIV", className: "ask-prose", textContent: "before allowing" },
    ],
  );
});

test("a json fence keeps the argument cap", () => {
  const payload = Object.fromEntries(Array.from({ length: 8 }, (_, n) => [`arg${n}`, n]));
  const text = visible({
    title: "Tool",
    kind: "read",
    content: [`\`\`\`json\n${JSON.stringify(payload)}\n\`\`\``],
    input: {},
  });

  assert.equal(text.includes("```"), false, text);
  assert.equal(text.includes("and 2 more arguments"), true, text);
  assert.equal(text.includes("arg6:"), false, text);
});
