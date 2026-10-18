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

test("an execute ask draws its content as text, and the kind and path as metadata", () => {
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

// A title is a label rather than a document: the Harness's characters are drawn
// as written, and only a span it marked as code is rendered. Emphasis, lists,
// headings, links and tags stay text.
test("a title keeps the characters the Harness wrote", () => {
  for (const title of ["Read __init__.py", "1. Do it", "*a* and _b_", "# x", "[a](b) <b>x</b>"]) {
    const body = render({ title, kind: "other", content: [] });

    assert.equal(body.querySelector(".ask-title").textContent, title, title);
    for (const tag of ["STRONG", "EM", "H1", "OL", "UL", "LI", "A", "B", "PRE"]) {
      assert.equal(body.querySelector(tag) ?? null, null, `${title} drew a ${tag}`);
    }
  }
});

test("a title draws each code span and leaves the text between them alone", () => {
  const body = render({ title: "Run `ls` in `~/src`", kind: "execute", content: [] });

  assert.deepEqual(
    body.querySelector(".ask-title").childNodes.map((node) => node.textContent),
    ["Run ", "ls", " in ", "~/src"],
  );
  assert.equal(body.textContent.includes("`"), false);
});

// `cursor-agent` sends a shell ask with the command in the call's title, as a
// code span. The Harness marked it, so it draws as code: the backticks go and
// the command reads as a command.
test("a title the Harness marked as code is drawn as code", () => {
  const body = render({
    title: "`(sleep 120 && osascript -e 'display notification') &`",
    kind: "execute",
    content: ["Shell allowlist is empty"],
    locations: [],
  });
  const code = body.querySelector(".ask-title").querySelector("code");

  assert.equal(code.tagName, "CODE");
  assert.equal(code.textContent, "(sleep 120 && osascript -e 'display notification') &");
  assert.equal(body.textContent.includes("`"), false);
  assert.equal(body.querySelector(".ask-metadata").textContent, "execute");
});

// The mark is the Harness's, not the shape's or the kind's: a span inside a
// longer title is code too, and a title that is only words keeps its words.
test("a code span inside a longer title is code too", () => {
  const marked = render({ title: "Rust `cargo test`", kind: "read", content: [] });

  assert.equal(marked.querySelector(".ask-title").querySelector("code").textContent, "cargo test");
  assert.equal(marked.querySelector(".ask-title").textContent, "Rust cargo test");
  assert.equal(marked.textContent.includes("`"), false);
  assert.equal(marked.querySelector(".ask-metadata").textContent, "read");

  const plain = render({ title: "Open Calculator", kind: "edit", content: [] });

  assert.equal(plain.querySelector(".ask-title").textContent, "Open Calculator");
  assert.equal(plain.querySelector(".ask-title").querySelector("code"), null);
});

test("markup inside a code span stays text", () => {
  const body = render({ title: "`echo <script>alert(1)</script>`", kind: "execute", content: [] });

  assert.equal(
    body.querySelector(".ask-title").querySelector("code").textContent,
    "echo <script>alert(1)</script>",
  );
  assert.equal(elements(body).some((node) => node.tagName === "SCRIPT"), false);
});

// A code span keeps CommonMark's rule: its line endings are spaces, so a
// command that spans lines arrives on one.
test("a code span's line endings are spaces, as in CommonMark", () => {
  const body = render({ title: "`python3 -c '\nprint(1)\n'`", kind: "execute", content: [] });

  assert.equal(
    body.querySelector(".ask-title").querySelector("code").textContent,
    "python3 -c ' print(1) '",
  );
});

// Only the backticks are Markdown in a title. A span inside emphasis,
// strikethrough or a link is still code, and the marks around it stay text.
test("a code span inside other markup is code, and the markup stays text", () => {
  for (const [title, around, code] of [
    ["**`rm -rf`**", ["**", "**"], "rm -rf"],
    ["**`rm -rf`** now", ["**", "** now"], "rm -rf"],
    ["~~`x`~~", ["~~", "~~"], "x"],
    ["*run `ls`*", ["*run ", "*"], "ls"],
    ["[`a`](http://b)", ["[", "](http://b)"], "a"],
  ]) {
    const host = render({ title, kind: "execute", content: [] }).querySelector(".ask-title");

    assert.deepEqual(
      host.childNodes.map((node) => [node.tagName ?? "#text", node.textContent]),
      [
        ["#text", around[0]],
        ["CODE", code],
        ["#text", around[1]],
      ],
      title,
    );
    for (const tag of ["STRONG", "EM", "DEL", "S", "A", "SPAN"]) {
      assert.equal(host.querySelector(tag) ?? null, null, `${title} drew a ${tag}`);
    }
  }
});

test("a title mixes text and several code spans in the order written", () => {
  const host = render({
    title: "Copy `__init__.py` to **`dist/`** then run `make` (1. of 2)",
    kind: "execute",
    content: [],
  }).querySelector(".ask-title");

  assert.deepEqual(
    host.childNodes.map((node) => [node.tagName ?? "#text", node.textContent]),
    [
      ["#text", "Copy "],
      ["CODE", "__init__.py"],
      ["#text", " to **"],
      ["CODE", "dist/"],
      ["#text", "** then run "],
      ["CODE", "make"],
      ["#text", " (1. of 2)"],
    ],
  );
});

// A backtick run closes only at a run of the same length, so a lone backtick
// is a character and a double-backtick span can hold one.
test("an unclosed backtick is text, and a longer run holds a backtick", () => {
  const lone = render({ title: "Use a ` here", kind: "other", content: [] }).querySelector(".ask-title");

  assert.equal(lone.textContent, "Use a ` here");
  assert.equal(lone.querySelector("code"), null);

  const held = render({ title: "Type `` a`b ``", kind: "other", content: [] }).querySelector(".ask-title");

  assert.equal(held.querySelector("code").textContent, "a`b");
});

test("a question is a paragraph, and input with no content is a json fence", () => {
  const question = render({
    title: "Question",
    kind: "other",
    content: ["Which branch? <b>main</b>"],
  });
  assert.equal(question.querySelector(".ask-title").textContent, "Question");
  assert.equal(question.querySelector(".ask-content").querySelector("p").textContent, "Which branch? <b>main</b>");
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

  assert.equal(body.querySelector(".ask-content").querySelector("p").textContent, "Check this");
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

test("an empty fence with no title says so instead of a blank card", () => {
  const body = render({
    title: null,
    kind: "execute",
    content: ["```json\n{}\n```"],
    input: { title: "Teams" },
    locations: [],
  });

  assert.equal(body.textContent, "The Harness asked for permission without saying what for.");
  assert.equal(body.querySelector("code"), null);
  assert.equal(body.textContent.includes("Teams"), false);
  assert.equal(body.textContent.includes("execute"), false);
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
