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

test("the arguments stand in when there is no content, as a json fence", () => {
  const says = askSays({
    ...ask,
    title: "Run a command",
    kind: "execute",
    input: { command: "rm -rf /", cwd: "/Users/oded" },
  });

  assert.deepEqual(says, [
    { kind: "title", text: "Run a command" },
    {
      kind: "markdown",
      text: '```json\n{\n  "command": "rm -rf /",\n  "cwd": "/Users/oded"\n}\n```',
    },
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

test("content is stripped by the renderer, not before it", () => {
  const text = "read\u202E/etc";
  const says = askSays({ ...ask, content: [text] });

  assert.equal(says.find((part) => part.kind === "markdown").text, text);
});

test("an empty fence and nothing else says so", () => {
  const sentence = "The Harness asked for permission without saying what for.";
  for (const content of ["```json\n{}\n```", "```json\n[]\n```", "```\nnull\n```"]) {
    assert.deepEqual(
      askSays({
        ...ask,
        title: null,
        kind: "execute",
        content: [content],
        input: { title: "Teams" },
        locations: [],
      }),
      [{ kind: "prose", text: sentence }],
      content,
    );
  }
});

test("an empty fence beside real content keeps the content", () => {
  assert.deepEqual(
    askSays({
      ...ask,
      title: null,
      content: ["```json\n{}\n```", "Which branch?"],
      input: { title: "Teams" },
    }),
    [{ kind: "markdown", text: "Which branch?" }],
  );
});

test("an empty fence beside a path leaves the path and hides the input", () => {
  assert.equal(
    askText({
      ...ask,
      title: null,
      kind: "edit",
      content: ["```json\n{}\n```", ""],
      input: { title: "Teams" },
      locations: ["/a.rs"],
    }),
    "edit · /a.rs",
  );
});

test("an empty input is not a body", () => {
  for (const input of [{}, [], null, ""]) {
    assert.deepEqual(
      askSays({ ...ask, title: null, input }),
      [{ kind: "prose", text: "The Harness asked for permission without saying what for." }],
      JSON.stringify(input),
    );
  }
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

test("every path is named", () => {
  const says = askText({
    ...ask,
    locations: ["/a.rs", "/b.rs", "/c.rs", "/d.rs", "/e.rs"],
  });

  assert.equal(says, "Question from MCP server\n/a.rs, /b.rs, /c.rs, /d.rs, /e.rs");
});

test("a large input is the json the Harness sent", () => {
  const blob = "x".repeat(50_000);
  const says = askSays({ ...ask, title: null, input: { blob } });

  assert.equal(says.length, 1);
  assert.equal(says[0].kind, "markdown");
  assert.ok(says[0].text.includes(blob), says[0].text.length);
  assert.ok(!says[0].text.includes("…"));
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

test("a title keeps a newline inside its own part", () => {
  assert.deepEqual(askSays({ ...ask, title: "Question\nedit · /safe/path", content: [] }), [
    { kind: "title", text: "Question\nedit · /safe/path" },
  ]);
});

test("a title loses a bidi override and keeps the rest", () => {
  assert.deepEqual(askSays({ ...ask, title: "read\u202E/etc/passwd", content: [] }), [
    { kind: "title", text: "read/etc/passwd" },
  ]);
});

test("input that is not an object is still a json fence", () => {
  assert.deepEqual(askSays({ ...ask, title: null, input: "rm -rf /" }), [
    { kind: "markdown", text: '```json\n"rm -rf /"\n```' },
  ]);
  assert.deepEqual(askSays({ ...ask, title: null, input: ["a", "b"] }), [
    { kind: "markdown", text: '```json\n[\n  "a",\n  "b"\n]\n```' },
  ]);
  assert.deepEqual(askSays({ ...ask, title: null, input: 0 }), [
    { kind: "markdown", text: "```json\n0\n```" },
  ]);
});

test("whitespace the Harness sent stays, and an empty string is not content", () => {
  assert.deepEqual(
    askSays({
      ...ask,
      title: "   ",
      content: ["", "  "],
      input: { command: "pwd" },
    }),
    [
      { kind: "title", text: "   " },
      { kind: "markdown", text: "  " },
    ],
  );
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
