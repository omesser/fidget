// What a permission ask says, the one line of copy in the product that stands
// in front of a security decision. The orderings matter as much as the wording:
// a row that leads with `other` and withholds the question teaches a reflexive Allow.

import assert from "node:assert/strict";
import { test } from "node:test";

import { askSays, elicitChoices, elicitSays } from "../src/chat-ask.js";

// One ask, as the Shell serializes it.
const ask = {
  request: "99",
  title: "Question from MCP server",
  kind: "other",
  content: [],
  input: null,
  locations: [],
  options: [],
};

// The row as one string, for the cases about wording and budget rather than shape.
function askText(value) {
  return askSays(value).map(({ text }) => text).join("\n");
}

test("the content is what the row says, under the title", () => {
  const says = askSays({ ...ask, content: ["Which branch should I push to?"] });

  assert.deepEqual(says, [
    { kind: "title", text: "Question from MCP server" },
    { kind: "markdown", text: "Which branch should I push to?" },
  ]);
});

test("the arguments stand in when there is no content, and are code", () => {
  const says = askSays({
    ...ask,
    title: "Run a command",
    kind: "execute",
    input: { command: "rm -rf /", cwd: "/Users/oded" },
  });

  assert.deepEqual(says, [
    { kind: "title", text: "Run a command" },
    { kind: "code", text: "command: rm -rf /" },
    { kind: "code", text: "cwd: /Users/oded" },
    { kind: "metadata", text: "execute" },
  ]);
});

test("execute content stays the command the Harness sent", () => {
  assert.deepEqual(
    askSays({ ...ask, title: "Open Calculator", kind: "execute", content: ["open -a Calculator"] }),
    [
      { kind: "title", text: "Open Calculator" },
      { kind: "markdown", text: "open -a Calculator" },
      { kind: "metadata", text: "execute" },
    ],
  );
  assert.deepEqual(askSays({ ...ask, content: ["Which branch?"] }), [
    { kind: "title", text: "Question from MCP server" },
    { kind: "markdown", text: "Which branch?" },
  ]);
});

test("a long question does not drop the path", () => {
  const says = askSays({
    ...ask,
    kind: "edit",
    content: ["why ".repeat(1000)],
    locations: ["/a.rs"],
  });

  assert.ok(says.some((part) => part.kind === "markdown" && part.text.length > 600));
  assert.ok(says.some((part) => part.text === "edit · /a.rs"));
});

test("content wins over the arguments rather than joining them", () => {
  const says = askText({
    ...ask,
    content: ["Which branch should I push to?"],
    input: { question: "Which branch should I push to?" },
  });

  assert.ok(!says.includes("question:"), says);
});

test("an ask with nothing to show says so, and says it as a sentence", () => {
  const says = askText({ ...ask, title: null });

  assert.equal(says, "The Harness asked for permission without saying what for.");
  assert.ok(!says.includes("untitled"), says);
  assert.ok(!says.includes("other"), says);
});

test("kind never leads, and `other` never appears at all", () => {
  assert.equal(askText(ask), "Question from MCP server");
  assert.equal(
    askText({ ...ask, kind: "execute" }),
    "Question from MCP server\nexecute",
  );
});

test("a tool that touches a path names the path beside the kind", () => {
  const says = askText({
    ...ask,
    title: "Edit a file",
    kind: "edit",
    locations: ["/Users/oded/src/main.rs"],
  });

  assert.equal(says, "Edit a file\nedit · /Users/oded/src/main.rs");
});

test("more paths than fit are counted, not listed", () => {
  const says = askText({
    ...ask,
    locations: ["/a.rs", "/b.rs", "/c.rs", "/d.rs", "/e.rs"],
  });

  assert.equal(says, "Question from MCP server\n/a.rs, /b.rs, /c.rs, and 2 more paths");
});

test("a huge argument payload is bounded and marked", () => {
  const says = askText({ ...ask, input: { blob: "x".repeat(50_000) } });

  assert.ok(says.length <= 600, `${says.length} characters`);
  assert.ok(says.endsWith("…"), says);
});

// The kind and the paths are drawn after the question, so the budget has to
// cover them as well.
test("a long question leaves the kind and the paths in place", () => {
  const says = askSays({
    ...ask,
    title: "Edit some files",
    kind: "edit",
    content: ["Q".repeat(1000)],
    locations: ["/a".repeat(100)],
  });

  assert.ok(says.some((part) => part.kind === "markdown" && part.text.length === 1000));
  assert.ok(says.some((part) => part.kind === "metadata" && part.text.startsWith("edit · ")));
});

// A chatty server, not a hostile one: the row is back to withholding the
// question if a long title can spend the whole budget first.
test("a verbose title cannot crowd out the question", () => {
  const says = askText({
    ...ask,
    title: "Permission ".repeat(60),
    content: ["Which branch should I push to?"],
  });

  assert.ok(says.includes("Which branch should I push to?"), says);
});

test("more arguments than fit are counted, not listed", () => {
  const input = Object.fromEntries(
    Array.from({ length: 20 }, (_, n) => [`arg${n}`, n]),
  );

  const says = askText({ ...ask, input });

  assert.ok(says.includes("and 14 more arguments"), says);
  assert.ok(!says.includes("arg7:"), says);
});

// A title is one element. A newline inside it must not become a second line
// that looks like the metadata the row writes itself.
test("a title cannot forge a line of its own", () => {
  const says = askText({
    ...ask,
    title: "Question\nedit · /safe/path",
    content: [],
  });

  assert.equal(says, "Question edit · /safe/path");
  assert.equal(says.split("\n").length, 1);
});

test("an argument value loses a bidi override and stays one line", () => {
  assert.equal(askText({ ...ask, title: null, input: { note: "a\u202Eb\nc" } }), "note: ab c");
});

test("arguments that are not an object still read as one line", () => {
  assert.equal(askText({ ...ask, title: null, input: "rm -rf /" }), '"rm -rf /"');
  assert.equal(askText({ ...ask, title: null, input: ["a", "b"] }), '["a","b"]');
});

test("an ask whose every field is blank is still the sentence", () => {
  const says = askText({
    ...ask,
    title: "   ",
    content: ["", "  "],
    input: {},
  });

  assert.equal(says, "The Harness asked for permission without saying what for.");
});

test("a missing field is not a crash", () => {
  assert.equal(
    askText({ request: "1", options: [] }),
    "The Harness asked for permission without saying what for.",
  );
});

// A kind cannot be the whole of an ask: it is a category, not the question. A
// path can: it is a fact about what happens, not a label for it.
test("an elicitation form says the question the Harness sent", () => {
  assert.equal(
    elicitSays({
      request: "43",
      message: "How should I approach this refactoring?",
      field: "strategy",
      options: [{ value: "balanced", name: "balanced" }],
    }),
    "How should I approach this refactoring?",
  );
});

test("an elicitation with no message says so as a sentence", () => {
  assert.equal(
    elicitSays({ request: "43", message: "  ", field: "", options: [] }),
    "The Harness asked a question without saying what for.",
  );
});

test("untrusted elicitation text cannot forge a line of its own", () => {
  assert.equal(
    elicitSays({
      request: "43",
      message: "Which branch?\nedit · /safe/path",
      field: "branch",
      options: [],
    }),
    "Which branch? edit · /safe/path",
  );
});

test("a kind is not enough on its own, and a path is", () => {
  assert.equal(
    askText({ ...ask, title: null, kind: "execute" }),
    "The Harness asked for permission without saying what for.",
  );
  assert.equal(
    askText({ ...ask, title: null, kind: "edit", locations: ["/a.rs"] }),
    "edit · /a.rs",
  );
});

test("a form offers its options and then Decline", () => {
  assert.deepEqual(
    elicitChoices({
      request: "43",
      message: "Which?",
      field: "strategy",
      options: [{ value: "balanced", name: "Balanced" }, { value: "aggressive", name: "" }],
      url: null,
    }),
    [
      { name: "Balanced", value: "balanced" },
      { name: "aggressive", value: "aggressive" },
      { name: "Decline", value: null },
    ],
  );
});

test("a URL form offers to open its link, or Decline", () => {
  assert.deepEqual(
    elicitChoices({
      request: "44",
      message: "Enter ABCD-1234 on the sign-in page.",
      field: "",
      options: [],
      url: "https://example.test/device?code=ABCD-1234",
    }),
    [
      { name: "Open", value: "open", url: "https://example.test/device?code=ABCD-1234" },
      { name: "Decline", value: null },
    ],
  );
});
