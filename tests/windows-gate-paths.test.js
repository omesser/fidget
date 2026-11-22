// The `windows` job in tests.yml runs only when the diff hits windows-gate's
// path list. A Rust file with Windows-only code that the list misses can
// break the Windows build without the Windows tests running.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const repo = new URL("..", import.meta.url);

const WINDOWS_ONLY = /cfg!?\(windows\)|target_os = "windows"|cfg!?\(not\(unix\)\)/;

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

test("windows-gate lists every Rust file with Windows-only code", () => {
  const patterns = gatePatterns().map(toRegExp);
  const rust = execFileSync("git", ["ls-files", "-z", "*.rs"], { cwd: repo, encoding: "utf8" })
    .split("\0")
    .filter(Boolean);
  const missing = rust.filter(
    (path) =>
      WINDOWS_ONLY.test(readFileSync(new URL(path, repo), "utf8")) &&
      !patterns.some((pattern) => pattern.test(path)),
  );
  assert.deepEqual(missing, [], `add to windows-gate's files in tests.yml: ${missing.join(", ")}`);
});
