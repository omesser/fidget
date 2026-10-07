import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// This test pins JS invoke call shapes against Rust command signatures
// to catch mismatches like the overlay_qm_state bug (7f89d150).

test("overlay_qm_state invoke shape matches Rust signature", () => {
  const mainJs = readFileSync("src/main.js", "utf8");
  const mainRs = readFileSync("src-tauri/src/main.rs", "utf8");

  // Find the invoke call in main.js
  const invokeMatch = mainJs.match(/invoke\("overlay_qm_state",\s*\{([^}]+)\}/);
  assert.ok(invokeMatch, "overlay_qm_state invoke not found in main.js");
  const jsPayload = invokeMatch[1].trim();

  // Rust signature: fn overlay_qm_state(payload: QmStatePayload)
  // JS must call with { payload: {...} } in Tauri 2
  assert.ok(jsPayload.startsWith("payload:"), "overlay_qm_state must pass { payload: {...} } in Tauri 2");

  // Verify the Rust command exists with the expected signature
  assert.ok(
    /fn overlay_qm_state\(payload: QmStatePayload\)/.test(mainRs),
    "Rust overlay_qm_state signature not found or changed"
  );
});

test("overlay_qm_drag_dismiss invoke shape matches Rust signature", () => {
  const mainJs = readFileSync("src/main.js", "utf8");
  const mainRs = readFileSync("src-tauri/src/main.rs", "utf8");

  // Find the invoke call
  const invokeMatch = mainJs.match(/invoke\("overlay_qm_drag_dismiss",\s*\{([^}]+)\}/);
  assert.ok(invokeMatch, "overlay_qm_drag_dismiss invoke not found");
  const jsPayload = invokeMatch[1].trim();

  // Rust signature: fn overlay_qm_drag_dismiss(instance: String)
  // JS must pass { instance: "..." }
  assert.ok(jsPayload.startsWith("instance:"), "overlay_qm_drag_dismiss must pass { instance }");

  assert.ok(
    /fn overlay_qm_drag_dismiss\(instance: String\)/.test(mainRs),
    "Rust overlay_qm_drag_dismiss signature not found or changed"
  );
});

test("overlay_trace_bubble invoke shape matches Rust signature", () => {
  const mainJs = readFileSync("src/main.js", "utf8");
  const mainRs = readFileSync("src-tauri/src/main.rs", "utf8");

  // Find all overlay_trace_bubble invokes
  const traceInvokes = mainJs.match(/invoke\("overlay_trace_bubble"[^;)]+\)/g);
  assert.ok(traceInvokes && traceInvokes.length > 0, "overlay_trace_bubble invokes not found");

  // Verify none use the wrong shape { label:, message: } or { line: }
  for (const invoke of traceInvokes) {
    assert.ok(
      !invoke.includes("{ label:") && !invoke.includes("{ line:"),
      `overlay_trace_bubble must pass label and message as separate args, not an object: ${invoke.substring(0, 100)}`
    );
  }

  // Rust signature: fn overlay_trace_bubble(app: ..., label: String, message: String)
  assert.ok(
    /fn overlay_trace_bubble\([^)]*label: String,\s*message: String/.test(mainRs),
    "Rust overlay_trace_bubble signature not found or changed"
  );
});
