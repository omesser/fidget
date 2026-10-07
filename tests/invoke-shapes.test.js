import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// This test pins JS invoke call shapes against Rust command signatures
// to catch mismatches like the Tauri 2 parameter mapping bugs.

test("overlay_trace_bubble invoke shape matches Rust signature", () => {
  const mainJs = readFileSync("src/main.js", "utf8");
  const mainRs = readFileSync("src-tauri/src/main.rs", "utf8");

  // Find all overlay_trace_bubble invokes - need to match across newlines
  const tracePattern = /invoke\("overlay_trace_bubble",\s*\{[\s\S]*?\}\)/g;
  const traceInvokes = mainJs.match(tracePattern);
  assert.ok(traceInvokes && traceInvokes.length > 0, "overlay_trace_bubble invokes not found");

  // Verify all pass a single object literal with {label, message}
  for (const invoke of traceInvokes) {
    assert.ok(invoke.includes("label:"), `overlay_trace_bubble object must have label key: ${invoke.substring(0, 100)}`);
    assert.ok(invoke.includes("message:"), `overlay_trace_bubble object must have message key: ${invoke.substring(0, 100)}`);
  }

  // Rust signature: fn overlay_trace_bubble(app: ..., label: String, message: String)
  assert.ok(
    /fn overlay_trace_bubble\([^)]*label: String,\s*message: String/.test(mainRs),
    "Rust overlay_trace_bubble signature not found or changed"
  );
});

test("overlay_report_qm_draft invoke shape matches Rust signature", () => {
  const mainJs = readFileSync("src/main.js", "utf8");
  const mainRs = readFileSync("src-tauri/src/main.rs", "utf8");

  // Find overlay_report_qm_draft invokes
  const draftPattern = /invoke\("overlay_report_qm_draft",\s*\{[\s\S]*?\}\)/g;
  const draftInvokes = mainJs.match(draftPattern);
  assert.ok(draftInvokes && draftInvokes.length > 0, "overlay_report_qm_draft invokes not found");

  // Verify all pass {instance, payload} where payload is object or null
  for (const invoke of draftInvokes) {
    assert.ok(invoke.includes("instance:"), `overlay_report_qm_draft must have instance key: ${invoke.substring(0, 100)}`);
    assert.ok(invoke.includes("payload:"), `overlay_report_qm_draft must have payload key: ${invoke.substring(0, 100)}`);
  }

  // Rust signature: fn overlay_report_qm_draft(instance: String, payload: Option<QmDraftPayload>)
  assert.ok(
    /fn overlay_report_qm_draft\([^)]*instance: String,\s*payload: Option<QmDraftPayload>/.test(mainRs),
    "Rust overlay_report_qm_draft signature not found or changed"
  );
});
