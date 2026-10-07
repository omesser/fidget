// The consent row: what an ask says, drawn on a stand-in document that refuses
// innerHTML, because every word of an ask is untrusted.

import assert from "node:assert/strict";
import { test } from "node:test";

import { drawAskDetails } from "../src/chat-ask-row.js";

class Node {
  constructor() {
    this.parentNode = null;
  }

  remove() {
    const siblings = this.parentNode?.childNodes;
    const at = siblings ? siblings.indexOf(this) : -1;
    siblings?.splice(at, 1);
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
  constructor(tagName, ownerDocument) {
    super();
    this.nodeType = 1;
    this.tagName = tagName.toUpperCase();
    this.ownerDocument = ownerDocument;
    this.className = "";
    this.dataset = {};
    this.childNodes = [];
  }

  set innerHTML(_) {
    throw new Error("untrusted text must not become HTML");
  }

  append(...nodes) {
    for (const node of nodes) {
      if (node.parentNode) {
        node.remove();
      }
      node.parentNode = this;
      this.childNodes.push(node);
    }
  }

  replaceChildren(...nodes) {
    for (const node of this.childNodes) {
      node.parentNode = null;
    }
    this.childNodes = [];
    this.append(...nodes);
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
      if (kid.className.split(/\s+/).includes(wanted) || kid.tagName === wanted.toUpperCase()) {
        return kid;
      }
      const deeper = kid.querySelector(selector);
      if (deeper) {
        return deeper;
      }
    }
    return null;
  }
}

const document = {
  createElement(tagName) {
    return new Element(tagName, this);
  },
  createTextNode(text) {
    return new Text(text);
  },
};

function render(ask) {
  const body = document.createElement("div");
  drawAskDetails(body, ask);
  return body;
}

function elements(node) {
  return node.children.flatMap((kid) => [kid, ...elements(kid)]);
}

function drawn(ask) {
  return render(ask).children.map(({ tagName, className, textContent }) => ({
    tagName,
    className,
    textContent,
  }));
}

test("an execute ask draws the command as text, and the kind and path as metadata", () => {
  const body = render({
    title: "Open <img src=x>",
    kind: "execute",
    content: ["open -a Calculator && <script>alert(1)</script>"],
    input: { command: "not displayed twice" },
    locations: ["/tmp/<report>"],
  });

  assert.equal(body.querySelector(".ask-title").textContent, "Open <img src=x>");
  assert.equal(body.textContent.includes("open -a Calculator && <script>alert(1)</script>"), true);
  assert.equal(body.textContent.includes("not displayed twice"), false);
  assert.equal(elements(body).some((node) => node.tagName === "SCRIPT"), false);
  assert.equal(body.querySelector(".ask-metadata").textContent, "execute · /tmp/<report>");
});

test("a question is a paragraph, and input with no content is a json fence", () => {
  const question = render({
    title: "Question",
    kind: "other",
    content: ["Which branch? <b>main</b>"],
  });
  assert.equal(question.querySelector(".ask-title").textContent, "Question");
  assert.equal(question.querySelector("p").textContent, "Which branch? <b>main</b>");
  assert.equal(elements(question).some((node) => node.tagName === "B"), false);

  const fallback = render({ title: "Run", kind: "execute", content: [], input: { command: "pwd" } });
  const code = fallback.querySelector("code");

  assert.equal(fallback.querySelector(".ask-title").textContent, "Run");
  assert.equal(code.textContent, '{\n  "command": "pwd"\n}');
  assert.equal(code.parentNode.tagName, "PRE");
  assert.equal(fallback.textContent.includes("```"), false);
  assert.equal(fallback.querySelector(".ask-code"), null);
  assert.equal(fallback.querySelector(".ask-metadata").textContent, "execute");
});

test("an empty json fence draws no body under the tool name", () => {
  for (const content of [
    "```json\n{}\n```",
    "```json\n{ }\n```",
    "```json\n[]\n```",
    "```json\nnull\n```",
    '```json\n""\n```',
    "```json\n\n```",
    "```\n{}\n```",
  ]) {
    const body = render({
      title: "fidget-describe_screen",
      kind: "other",
      content: [content],
      input: { title: "Teams" },
    });
    assert.equal(body.querySelector(".ask-title").textContent, "fidget-describe_screen", content);
    assert.equal(body.querySelector("code"), null, content);
    assert.equal(body.textContent.includes("```"), false, content);
    assert.equal(body.textContent.includes("Teams"), false, content);
  }
});

test("a filled json fence is a code block and the backticks are gone", () => {
  const body = render({
    title: "fidget-list_windows",
    kind: "read",
    content: ['```json\n{"title":"Teams","limit":5}\n```'],
    input: {},
  });
  const code = body.querySelector("code");

  assert.equal(code.textContent, '{"title":"Teams","limit":5}');
  assert.equal(code.parentNode.tagName, "PRE");
  assert.equal(body.textContent.includes("```"), false);
  assert.equal(body.querySelector(".ask-metadata").textContent, "read");
});

test("a non-json fence keeps its lines as code", () => {
  const body = render({
    title: "Run",
    kind: "other",
    content: ["```sh\nls\nread · /etc/passwd\n```"],
  });
  const code = body.querySelector("code");

  assert.equal(code.textContent, "ls\nread · /etc/passwd");
  assert.equal(body.textContent.includes("```"), false);
});

test("prose and a fence in one entry are a paragraph and a code block", () => {
  const body = render({
    title: "Question",
    kind: "other",
    content: ['Check this\n\n```json\n{"title":"Teams"}\n```\n\nbefore allowing'],
    input: { question: "not shown" },
  });

  assert.equal(body.querySelector("p").textContent, "Check this");
  assert.equal(body.querySelector("code").textContent, '{"title":"Teams"}');
  assert.equal(body.textContent.includes("before allowing"), true);
  assert.equal(body.textContent.includes("```"), false);
  assert.equal(body.textContent.includes("not shown"), false);
});

test("bidi and zero-width characters are removed and newlines in a fence stay", () => {
  const body = render({
    title: "Tool",
    content: ["```\nalpha\nbeta\u202E\u200B\n```"],
  });
  const code = body.querySelector("code");

  assert.equal(code.textContent, "alpha\nbeta");
  assert.equal(code.textContent.includes("\u202E"), false);
  assert.equal(code.textContent.includes("\u200B"), false);
});

test("an empty input draws no code under the tool name", () => {
  for (const input of [{}, [], null, ""]) {
    const body = render({ title: "Tool", kind: "other", content: [], input });
    assert.equal(body.querySelector(".ask-title").textContent, "Tool", JSON.stringify(input));
    assert.equal(body.querySelector("code"), null, JSON.stringify(input));
    assert.equal(body.textContent.includes("```"), false, JSON.stringify(input));
  }
});

test("a bidi override in input json is removed and the newlines stay", () => {
  const body = render({
    title: "Tool",
    content: [],
    input: { note: "a\u202Eb\nc" },
  });
  const code = body.querySelector("code");

  assert.equal(code.textContent.includes("\u202E"), false);
  assert.equal(code.textContent.includes("\n"), true);
  assert.equal(code.textContent.includes("ab"), true);
});

test("a javascript link in an ask is not clickable", () => {
  const body = render({
    title: "Question",
    content: ["[click](javascript:alert(1))"],
  });

  assert.equal(body.textContent.includes("click"), true);
  assert.equal(
    elements(body).some((node) => node.className.split(/\s+/).includes("md-link")),
    false,
  );
  assert.equal(
    elements(body).some((node) => node.dataset.href !== undefined),
    false,
  );
});
