// The `windows` job in tests.yml runs only when the diff hits windows-gate's
// path list. A Rust file with a cfg that holds on Windows, missing from the
// list, can break the Windows build without the Windows tests running.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const repo = new URL("..", import.meta.url);

// A cfg line that can hold on Windows. `all(unix, not(target_os = ...))` is a
// Unix subset, so it reads as `unix` first and never counts as `not(target_os`.
const CFG = /\bcfg(?:_attr)?!?\(.*/g;
const UNIX_SUBSET = /all\(unix, not\(target_os = "\w+"\)\)/g;
const WINDOWS = /\bwindows\b|not\(unix\)|not\(target_os/;
const hasWindowsCode = (source) =>
  (source.match(CFG) ?? []).some((cfg) => WINDOWS.test(cfg.replace(UNIX_SUBSET, "unix")));

function gatePatterns() {
  const workflow = readFileSync(new URL(".github/workflows/tests.yml", repo), "utf8");
  const gate = workflow.slice(workflow.indexOf("\n  windows-gate:"));
  const list = gate.match(/\n {10}files: \|\n((?: {12}.*\n)+)/);
  assert.ok(list, "windows-gate has no `files: |` list");
  return list[1]
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"));
}

// `**` crosses directories, `*` does not.
const toRegExp = (glob) =>
  new RegExp(
    `^${glob
      .replace(/[.+^${}()|[\]\\]/g, "\\$&")
      .replace(/\*\*/g, "\0")
      .replace(/\*/g, "[^/]*")
      .replace(/\0/g, ".*")}$`,
  );

test("windows-gate lists every Rust file with a cfg that holds on Windows", () => {
  const patterns = gatePatterns().map(toRegExp);
  const rust = execFileSync("git", ["ls-files", "-z", "*.rs"], { cwd: repo, encoding: "utf8" })
    .split("\0")
    .filter(Boolean);
  const missing = rust.filter(
    (path) =>
      hasWindowsCode(readFileSync(new URL(path, repo), "utf8")) &&
      !patterns.some((pattern) => pattern.test(path)),
  );
  assert.deepEqual(missing, [], `add to windows-gate's files in tests.yml: ${missing.join(", ")}`);
});
