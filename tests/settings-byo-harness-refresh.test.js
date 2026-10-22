import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { foldDraft, processResponse, stageDraft } from "../src/settings.js";

test("picking a registration Harness keeps the previous preview until the new one arrives", () => {
  const values = {
    ...JSON.parse(readFileSync(new URL("./fixtures/settings-values-modelApi.json", import.meta.url), "utf8")),
    byo_token: "old-token", // pragma: allowlist secret
  };
  const shown = (draft) => ({ ...values, ...draft });
  const fields = ["byo_snippet", "byo_steps", "byo_token"];

  let draft = stageDraft({}, "byo_harness", "hermes");
  for (const id of fields) assert.equal(shown(draft)[id], values[id], `${id} while the preview is in flight`);
  for (const id of fields) assert.notEqual(shown(draft)[id], "", id);

  draft = foldDraft(
    draft,
    processResponse({ action: "preview_byo", snippet: "hermes mcp add fidget", steps: "Run it.", token: "t" }),
    { pick: "byo_harness", value: "hermes" },
  );
  assert.equal(shown(draft).byo_snippet, "hermes mcp add fidget");
  assert.equal(shown(draft).byo_harness, "hermes");
});
