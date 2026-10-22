import assert from "node:assert/strict";
import { test } from "node:test";

import { applyEventOutcome, copyRunForPress, processResponse, writeRunClipboard } from "../src/settings.js";

const SNIPPET =
  "claude mcp add fidget -- /Applications/fidget.app/Contents/MacOS/fidget --mcp";

test("processResponse maps a run to the operation string", () => {
  assert.deepEqual(processResponse({ action: "run", operation: "copy_byo_snippet" }), {
    run: "copy_byo_snippet",
  });
  assert.deepEqual(processResponse({ action: "run", operation: "copy_byo_token" }), {
    run: "copy_byo_token",
  });
});

test("a copy_byo_snippet run writes the snippet already in values", async () => {
  const written = [];
  const outcome = processResponse({ action: "run", operation: "copy_byo_snippet" });
  await applyEventOutcome(outcome, { byo_snippet: SNIPPET }, async (text) => {
    written.push(text);
  });
  assert.deepEqual(written, [SNIPPET]);
});

test("the Copy press writes the same snippet processResponse names", async () => {
  const written = [];
  const run = copyRunForPress({ press: "byo_copy" });
  assert.equal(run, "copy_byo_snippet");
  await writeRunClipboard(run, { byo_snippet: SNIPPET }, async (text) => {
    written.push(text);
  });
  assert.deepEqual(written, [SNIPPET]);
});

test("a copy_byo_token run writes the token already in values", async () => {
  const written = [];
  const outcome = processResponse({ action: "run", operation: "copy_byo_token" });
  await applyEventOutcome(outcome, { byo_token: "beef" }, async (text) => { // pragma: allowlist secret
    written.push(text);
  });
  assert.deepEqual(written, ["beef"]);
});

test("refresh does not write the clipboard", async () => {
  const written = [];
  await applyEventOutcome(processResponse({ action: "refresh" }), { byo_snippet: SNIPPET }, async (text) => {
    written.push(text);
  });
  await applyEventOutcome(
    processResponse({ action: "run", operation: "copy_byo_snippet" }),
    { byo_snippet: SNIPPET },
    async (text) => {
      written.push(text);
    },
  );
  assert.deepEqual(written, [SNIPPET]);
});
