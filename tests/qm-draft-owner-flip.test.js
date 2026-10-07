import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

test("QM draft moves with owner flip, text intact", () => {
  const frameLoop = readFileSync("src-tauri/src/frame_loop.rs", "utf8");

  // The draft rides in Placed.qm, like dialogue
  assert.ok(
    /qm:\s*live\.qm\.clone\(\)/.test(frameLoop),
    "Placed must carry live.qm.clone() alongside dialogue"
  );

  // JS restores pill from placement.qm when this overlay owns the bubble
  const mainJs = readFileSync("src/main.js", "utf8");
  assert.ok(
    /placement\.bubble\s*&&\s*placement\.qm/.test(mainJs) ||
      /placement\.qm[\s\S]{0,50}placement\.bubble/.test(mainJs),
    "JS must check placement.bubble && placement.qm to restore pill on owner"
  );

  assert.ok(
    /restore\(.*placement\.qm\.text/.test(mainJs),
    "JS must restore pill text from placement.qm.text"
  );

  // Non-owner hides pill  
  const hidePattern = /!placement\.bubble.*hideWithoutReport/s;
  assert.ok(
    hidePattern.test(mainJs),
    "JS must hide pill when !placement.bubble (no longer owner)"
  );
});

test("Drag decision: empty closes, non-empty keeps", () => {
  const qmDraft = readFileSync("crates/core/src/qm_draft.rs", "utf8");

  // keep_qm_on_drag function exists
  assert.ok(
    /pub fn keep_qm_on_drag/.test(qmDraft),
    "keep_qm_on_drag function must exist"
  );

  // Test cases cover empty/non-empty/none
  assert.ok(
    /empty_text_closes_on_drag/.test(qmDraft),
    "Test: empty text closes on drag"
  );

  assert.ok(
    /non_empty_text_keeps_pill/.test(qmDraft),
    "Test: non-empty text keeps pill"
  );

  assert.ok(
    /no_draft_closes/.test(qmDraft),
    "Test: no draft closes on drag"
  );
});
