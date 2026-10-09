// What a reply's Markdown draws as, and the two things that must hold however
// the parser behaves: the caret survives every chunk, and nothing a model
// wrote becomes markup or a live URL scheme. The stand-in `document` below
// refuses an `insertBefore` against a non-child on purpose, as a browser does.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

import { appendReply, drawReply, drawThought, stripUnsafe } from "../src/markdown.js";

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

  insertBefore(node, reference) {
    if (reference === null || reference === undefined) {
      this.append(node);
      return node;
    }
    const at = this.childNodes.indexOf(reference);
    assert.ok(at >= 0, "DOMException: The child can not be found in the parent");
    this.#adopt(node, at);
    return node;
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
}

const doc = {
  createElement: (tag) => new Element(tag),
  createTextNode: (text) => new Text(text),
};

// The row's body, as `chat.js` builds it.
function saidBody() {
  const body = doc.createElement("div");
  body.className = "said md";
  return body;
}

function draw(source) {
  const body = saidBody();
  drawReply(body, source, doc);
  return body;
}

// Every element in the subtree, so a test can assert on what was built
// without knowing where it landed.
function elements(node) {
  return node.children.flatMap((kid) => [kid, ...elements(kid)]);
}

function tags(node) {
  return [...new Set(elements(node).map((kid) => kid.tagName))].sort();
}

// `tag.class[children]`, with text nodes quoted.
function sketch(node) {
  if (node.nodeType === 3) {
    return JSON.stringify(node.data);
  }
  const cls = node.className ? `.${node.className.trim().replace(/\s+/g, ".")}` : "";
  return `${node.tagName.toLowerCase()}${cls}[${node.childNodes.map(sketch).join(" ")}]`;
}

test("unsafe characters are stripped and newline and tab stay", () => {
  const cases = [
    ["\u202A\u202B\u202C\u202D\u202E", ""],
    ["\u2066\u2067\u2068\u2069", ""],
    ["\u200B\u2060\uFEFF", ""],
    ["\u200C\u200D", "\u200C\u200D"],
    ["a\u0000b\u0007c\u007Fd\u0085e", "abcde"],
    ["keep\n\tthem", "keep\n\tthem"],
    ["read\u202E/etc/passwd", "read/etc/passwd"],
  ];

  for (const [input, expected] of cases) {
    assert.equal(stripUnsafe(input), expected, JSON.stringify(input));
  }
});

test("an empty json fence in a reply still draws", () => {
  const body = draw("the config is now\n\n```json\n{}\n```");
  const code = elements(body).find((node) => node.tagName === "CODE");

  assert.equal(elements(body).find((node) => node.tagName === "P").textContent, "the config is now");
  assert.equal(code.textContent, "{}");
  assert.equal(body.textContent.includes("```"), false);
});

test("a joined emoji and a zwnj word survive a reply", () => {
  const family = "\u{1F468}\u200D\u{1F469}\u200D\u{1F467}";
  const word = "می\u200Cخواهم";

  assert.equal(draw(family).textContent, family);
  assert.equal(draw(word).textContent, word);
});

test("emphasis, code and strikethrough become elements, not punctuation", () => {
  const body = draw("**bold** and _italic_ and `code` and ~~gone~~");

  assert.equal(
    sketch(body.childNodes[0]),
    'p[strong["bold"] " and " em["italic"] " and " code["code"] " and " del["gone"]]',
  );
});

test("nested emphasis renders without leaving a marker behind", () => {
  const body = draw("***very important***");

  assert.equal(sketch(body.childNodes[0]), 'p[em[strong["very important"]]]');
  assert.equal(body.textContent, "very important");
});

test("a list is a list, and a nested one nests", () => {
  const body = draw("- one\n- two\n  - deeper\n");

  assert.equal(
    sketch(body.childNodes[0]),
    'ul[li["one"] li["two" ul[li["deeper"]]]]',
  );
});

test("a list item keeps the formatting inside it", () => {
  const body = draw("- **one** and `two`\n- [a link](https://example.com)\n");

  assert.equal(
    sketch(body.childNodes[0]),
    'ul[li[strong["one"] " and " code["two"]] li[span.md-link["a link"]]]',
  );
});

test("an ordered list keeps the number it started at", () => {
  const body = draw("3. three\n4. four\n");

  assert.equal(body.childNodes[0].tagName, "OL");
  assert.equal(body.childNodes[0].start, 3);
});

test("a task list draws a box that cannot be clicked", () => {
  const body = draw("- [x] done\n- [ ] todo\n");

  const boxes = elements(body).filter((node) => node.tagName === "INPUT");
  assert.deepEqual(
    boxes.map((box) => [box.type, box.checked, box.disabled]),
    [
      ["checkbox", true, true],
      ["checkbox", false, true],
    ],
  );
});

test("a fence keeps its text byte for byte and scrolls inside the row", () => {
  const body = draw("```rust\nfn main() {\n    let x = 1;\n}\n```");

  const fence = body.childNodes[0];
  assert.equal(fence.className, "md-wide", "the overflow container ADR-0012 needs");
  assert.equal(fence.childNodes[0].tagName, "PRE");
  const code = fence.childNodes[0].childNodes[0];
  assert.equal(code.className, "language-rust");
  assert.equal(code.textContent, "fn main() {\n    let x = 1;\n}");
});

// The ACP adapter for Claude Code grows the fence until it is longer than
// anything inside, so a tool result arrives behind four or more backticks.
test("a fence longer than three backticks is still a fence", () => {
  const body = draw("````\nsee ```js\n````");

  assert.equal(body.childNodes[0].className, "md-wide");
  assert.equal(body.textContent, "see ```js");
});

test("an info string that is not a word names no class", () => {
  const body = draw('```" onload="x\ncode\n```');

  const code = body.childNodes[0].childNodes[0].childNodes[0];
  assert.equal(code.className, "");
});

test("a table becomes a table, with the delimiter row's alignment", () => {
  const body = draw("| a | b |\n|:--|--:|\n| 1 | **2** |");

  assert.equal(body.childNodes[0].className, "md-wide");
  assert.equal(
    sketch(body.childNodes[0].childNodes[0]),
    'table[thead[tr[th.md-left["a"] th.md-right["b"]]] tbody[tr[td.md-left["1"] td.md-right[strong["2"]]]]]',
  );
});

test("headings, quotes and a rule render as their own blocks", () => {
  const body = draw("## Heading\n\n> quoted\n\n---\n");

  assert.deepEqual(
    body.children.map((kid) => kid.tagName),
    ["H2", "BLOCKQUOTE", "HR"],
  );
  assert.equal(body.children[1].childNodes[0].textContent, "quoted");
});

test("a hard break is a break and a soft one is left to the stylesheet", () => {
  const body = draw("one  \ntwo\nthree");

  assert.equal(sketch(body.childNodes[0]), 'p["one" br[] "two\\nthree"]');
});

test("an allowed link is styled and carries its target", () => {
  const body = draw("see [the docs](https://example.com/a?b=c) and <https://plain.example>");

  const links = elements(body).filter((node) => node.className === "md-link");
  assert.deepEqual(
    links.map((link) => [link.tagName, link.textContent, link.dataset.href]),
    [
      ["SPAN", "the docs", "https://example.com/a?b=c"],
      ["SPAN", "https://plain.example", "https://plain.example"],
    ],
  );
  assert.equal(links[0].href, undefined, "nothing here navigates; see src/markdown.js");
});

test("mailto is allowed and a relative target is not", () => {
  const mail = elements(draw("[a](mailto:x@y.z)")).find((n) => n.className === "md-link");
  assert.equal(mail.dataset.href, "mailto:x@y.z");

  const local = elements(draw("[a](/etc/passwd)")).find((n) => n.dataset.href !== undefined);
  assert.equal(local, undefined, "a relative target has nowhere to open and is refused");
});

// The constructs this renderer does not draw. Each reads as the source the
// model wrote.
test("an image and raw HTML read as their source", () => {
  const image = draw("![alt](https://example.com/x.png)");
  assert.deepEqual(tags(image), ["P"]);
  assert.equal(image.textContent, "![alt](https://example.com/x.png)");

  const html = draw("a <b>bold</b> c");
  assert.equal(html.textContent, "a <b>bold</b> c");
  assert.deepEqual(tags(html), ["P"]);
});

test("an HTML entity reads as its source, not as the character", () => {
  // Declared, not accidental: `marked` leaves entities in the token text and
  // decoding them would be a table of 2000 names this file does not own.
  assert.equal(draw("5 &lt; 6").textContent, "5 &lt; 6");
});

// The four-chunk stream, with one landing mid-marker and one mid-fence.
// Everything about the caret is asserted after every chunk.
test("the caret survives every chunk of a streamed reply", () => {
  const body = saidBody();
  const caret = doc.createElement("span");
  caret.className = "caret";
  caret.textContent = "▍";
  body.append(caret);

  const chunks = [
    "Here is **bo",
    "ld** and a list:\n\n- one\n- tw",
    "o\n\n```js\nlet x = 1;\n``",
    "`\n\nDone.",
  ];
  const seen = [];

  for (const chunk of chunks) {
    appendReply(body, chunk, doc);

    const found = body.querySelector(".caret");
    assert.ok(found, `the caret left the row on chunk ${JSON.stringify(chunk)}`);
    assert.equal(found, caret, "the caret was replaced rather than moved");
    assert.equal(
      elements(body).filter((node) => node.className === "caret").length,
      1,
      "the caret was drawn more than once",
    );
    assert.equal(found.textContent, "▍");
    seen.push(body.textContent.replace("▍", ""));
  }

  // Chunk one ends inside a marker, so it is text until chunk two closes it.
  assert.equal(seen[0], "Here is **bo");
  // SPAN is the caret, which is the point.
  assert.deepEqual(tags(body), ["CODE", "DIV", "LI", "P", "PRE", "SPAN", "STRONG", "UL"]);
  assert.equal(body.querySelector(".md-wide").textContent, "let x = 1;");
  assert.equal(
    caret.parentNode.tagName,
    "P",
    "the caret should blink at the end of the last line, not on one of its own",
  );
  assert.equal(caret.parentNode.textContent, "Done.▍");
});

test("a whole reply resets what the chunks were adding to", () => {
  // The Shell replays a line said before this window existed by drawing the
  // row outright, and that row must not inherit a previous one's text.
  const body = saidBody();

  appendReply(body, "first", doc);
  drawReply(body, "second", doc);
  appendReply(body, " and third", doc);

  assert.equal(body.textContent, "second and third");
});

test("the caret goes back to the row when the last block is not a paragraph", () => {
  const body = saidBody();
  const caret = doc.createElement("span");
  caret.className = "caret";
  body.append(caret);

  drawReply(body, "```\nx\n```", doc);

  assert.equal(caret.parentNode, body, "a caret inside a fence would be read as code");
});

test("an empty reply keeps the caret and draws nothing else", () => {
  const body = saidBody();
  const caret = doc.createElement("span");
  caret.className = "caret";
  body.append(caret);

  drawReply(body, "", doc);

  assert.deepEqual(body.childNodes, [caret]);
});

// Run, not reasoned about. A reply is a model's output and an MCP server's
// content can steer it, so these are the payloads that would matter.
const PAYLOADS = [
  "<script>alert(1)</script>",
  '<img src=x onerror=alert(1)>',
  "[x](javascript:alert(1))",
  "[x](data:text/html,<script>alert(1)</script>)",
  "[x](JaVaScRiPt:alert(1))",
  "[x](&#106;avascript:alert(1))",
  "[x](\njavascript:alert(1))",
  "[x](java\tscript:alert(1))",
  "[x](vbscript:msgbox(1))",
  "**unclosed and [broken](",
  "```\nunterminated fence",
  "| a |\n| broken table",
  "<iframe src=javascript:alert(1)></iframe>",
  "<a href=javascript:alert(1)>x</a>",
];

// Everything the renderer is allowed to build. Anything else in a payload's
// output is markup a reply produced.
const DRAWN = new Set([
  "P",
  "STRONG",
  "EM",
  "DEL",
  "CODE",
  "BR",
  "SPAN",
  "H1",
  "H2",
  "H3",
  "H4",
  "H5",
  "H6",
  "BLOCKQUOTE",
  "HR",
  "UL",
  "OL",
  "LI",
  "INPUT",
  "DIV",
  "PRE",
  "TABLE",
  "THEAD",
  "TBODY",
  "TR",
  "TH",
  "TD",
]);

test("no payload becomes markup", () => {
  for (const payload of PAYLOADS) {
    const body = draw(payload);
    const built = tags(body).filter((tag) => !DRAWN.has(tag));
    assert.deepEqual(built, [], `${JSON.stringify(payload)} built ${built.join(", ")}`);
  }
});

test("no payload becomes a live target", () => {
  for (const payload of PAYLOADS) {
    const body = draw(payload);
    for (const node of elements(body)) {
      assert.equal(node.href, undefined, `${JSON.stringify(payload)} set an href`);
      const target = node.dataset.href;
      if (target !== undefined) {
        assert.match(
          target,
          /^(https?|mailto):/,
          `${JSON.stringify(payload)} carried the target ${JSON.stringify(target)}`,
        );
      }
    }
  }
});

test("a refused link keeps its words", () => {
  const body = draw("[click me](javascript:alert(1))");

  assert.equal(body.textContent, "click me");
  assert.equal(elements(body).some((node) => node.dataset.href !== undefined), false);
});

test("a script payload reads as the text a model typed", () => {
  assert.equal(draw("<script>alert(1)</script>").textContent, "<script>alert(1)</script>");
  // The blank lines around a block of raw HTML go with it; the paragraphs
  // either side are still their own blocks, so it reads on a line of its own.
  assert.equal(
    draw("before\n\n<img src=x onerror=alert(1)>\n\nafter").textContent,
    "before<img src=x onerror=alert(1)>after",
  );
});

// The property the whole design rests on, asserted on the source rather than
// on one payload: there is no sink in the webview for a reply to reach. A new
// one somewhere else in src/ would make every test above beside the point.
test("nothing in the webview writes HTML", () => {
  const sinks = /innerHTML|outerHTML|insertAdjacentHTML|document\.write|new Function|\beval\(/;
  const offenders = ["../src/", "../src/vendor/"].flatMap((dir) => {
    const at = new URL(dir, import.meta.url);
    return readdirSync(at)
      .filter((name) => name.endsWith(".js"))
      .filter((name) => sinks.test(readFileSync(new URL(name, at), "utf8")))
      .map((name) => dir + name);
  });

  // The vendored parser is swept too: it has no sink today, and a re-vendor
  // that brought one in would put model output one call away from markup.
  assert.deepEqual(offenders, []);
});

test("the vendored parser is the file src/vendor/README.md documents", () => {
  // A hook that reformatted it, or an edit made in place, would leave the
  // registry hash in that file claiming something it cannot check.
  const marked = readFileSync(new URL("../src/vendor/marked.esm.js", import.meta.url));

  assert.equal(
    createHash("sha256").update(marked).digest("hex"),
    "2e70fea3ee49f98ab67ee395e5af51cc6bee4fafed15910da9ccb7f650df8014",
    "marked@18.0.13 lib/marked.esm.js; see src/vendor/README.md to re-vendor",
  );
});

// A thought is not a document: its line breaks and punctuation stay as the
// harness wrote them. The one thing drawn is a link, because a resource link
// can arrive there (ADR-0028), and it goes through the same target check.
function think(source) {
  const body = saidBody();
  drawThought(body, source, doc);
  return body;
}

test("a link in a thought is clickable and the rest is left as written", () => {
  const body = think("See [the spec](https://example.com/spec.md)\n\n**not bold** and `not code`");
  const links = elements(body).filter((node) => node.className === "md-link");

  assert.deepEqual(
    links.map((link) => [link.textContent, link.dataset.href]),
    [["the spec", "https://example.com/spec.md"]],
  );
  assert.equal(body.textContent, "See the spec\n\n**not bold** and `not code`");
  assert.deepEqual(tags(body), ["SPAN"]);
});

test("a link a thought may not open keeps its words and nothing is clickable", () => {
  const body = think("[click me](javascript:alert(1)) [link x file:///etc/passwd]");

  assert.equal(body.textContent, "click me [link x file:///etc/passwd]");
  assert.equal(elements(body).some((node) => node.dataset.href !== undefined), false);
});

test("a mark's escapes read back as the characters they stand for", () => {
  assert.equal(think("[link a file:///a\\_b\\(1\\)]").textContent, "[link a file:///a_b(1)]");
});

test("a thought redrawn from the whole text replaces the last drawing", () => {
  const body = think("a [one](https://example.com/1)");
  drawThought(body, "a [one](https://example.com/1) b [two](https://example.com/2)", doc);

  assert.deepEqual(
    elements(body).map((node) => node.dataset.href),
    ["https://example.com/1", "https://example.com/2"],
  );
});

// Only a link the author wrote as `[text](url)` is live. A bare URL, a `www.`
// host and an email address are links to `marked` and to nobody else, and a
// mark carries an agent's uri, so they stay as the characters they are.
const NO_LINKS = [
  ["a resource uri", "[resource https://evil.example/x, text/plain, 5 bytes]"],
  ["a www host", "[resource www.evil.example, 1 bytes]"],
  ["an email", "[resource mailto:evil@example.com, 1 bytes] and evil@example.com"],
  ["a bare link in a refused mark", "[link x https://evil.example/ ]"],
  ["an angle autolink", "<https://evil.example/>"],
];

for (const [name, source] of NO_LINKS) {
  test(`${name} in a thought is text, not a link`, () => {
    const body = think(source);

    assert.equal(body.textContent, source);
    assert.equal(elements(body).length, 0);
  });
}

// Characters a mark leaves bare, through a name, a uri and a mime type. The
// literals are the output of `content_mark`'s own test of the same strings.
test("the characters a mark leaves bare draw as themselves and make no element", () => {
  const spaced = "a\\\\b\\`c!d#e|f-g+h=i j k l";
  const shown = "a\\b`c!d#e|f-g+h=i j k l";
  const link = think(`[${spaced}](https://example.com/)`);
  assert.deepEqual(
    elements(link).map((node) => [node.className, node.textContent]),
    [["md-link", shown]],
  );
  for (const mark of [`[link ${spaced} no scheme]`, `[resource ${spaced}, ${spaced}, 0 bytes]`]) {
    const body = think(mark);
    assert.equal(elements(body).length, 0, mark);
    assert.equal(body.textContent, mark.replaceAll(spaced, shown).replaceAll("\\\\", "\\"), mark);
  }
  for (const source of [`[${spaced}](https://example.com/)\n\n`, `[link ${spaced} no scheme]\n\n`]) {
    const body = draw(source);
    assert.deepEqual(tags(body).filter((tag) => tag !== "P" && tag !== "SPAN"), [], source);
  }
});
