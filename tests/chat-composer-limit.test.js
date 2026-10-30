// The composer's bound is `CHAT_LIMIT`, and it belongs to the field rather than
// to what happens after Send.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const html = readFileSync(new URL("../src/chat.html", import.meta.url), "utf8");
const js = readFileSync(new URL("../src/chat.js", import.meta.url), "utf8").replace(/\s+/g, " ");
const director = readFileSync(new URL("../crates/core/src/director.rs", import.meta.url), "utf8");

test("the composer field is bounded at CHAT_LIMIT", () => {
  const declared = director.match(/pub const CHAT_LIMIT: usize = ([0-9_]+);/);
  assert.ok(declared, "CHAT_LIMIT is not declared where this test reads it");
  const limit = Number(declared[1].replaceAll("_", ""));

  // The field, whichever element it is drawn as. It is a textarea.
  const field = html.match(/<(?:input|textarea)\b[^>]*\bid="line"[^>]*>/);
  assert.ok(field, "the composer has no field with id=line");

  const maxlength = field[0].match(/\bmaxlength="([0-9]+)"/);
  assert.ok(maxlength, "the composer field carries no maxlength");
  assert.equal(
    Number(maxlength[1]),
    limit,
    "a field that holds more than chat_send takes puts unsent text in the log",
  );
});

// What the maxlength is worth only holds while the row and the payload are
// one string. Matched against whitespace-flattened source so reformatting the
// submit path cannot fail this.
test("the log row and what is sent are the same string", () => {
  assert.match(js, /const text = line\.value\.trim\(\);/, "one read of the field");
  assert.match(js, /said\(t\("You"\), text, "you"\)/, "the row is built from it");
  assert.match(js, /invoke\("chat_send", \{ instance, text \}\)/, "and so is the send");
});
