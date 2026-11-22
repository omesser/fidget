// The `windows` job in tests.yml runs only when the diff hits windows-gate's
// path list. A Rust file with a cfg that holds on Windows, missing from the
// list, can break the Windows build without the Windows tests running.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const repo = new URL("..", import.meta.url);

// A cfg that can hold on Windows. `all(unix, not(target_os = ...))` is a Unix
// subset, so it reads as `unix` first and never counts as `not(target_os`.
// An attribute runs to its `)]`, which may be lines later.
const CFG = /#!?\[cfg(?:_attr)?\([^\]]*\)\]|\bcfg!\(.*/g;
const UNIX_SUBSET = /all\(unix, not\(target_os = "\w+"\)\)/g;
const WINDOWS = /\bwindows\b|not\(unix\)|not\(target_os/;
const holdsOnWindows = (cfg) => WINDOWS.test(cfg.replace(UNIX_SUBSET, "unix"));

// `#[cfg(..)] mod x;` gates x.rs or x/mod.rs, which may carry no cfg of its own.
// A file declares its modules beside it if it is main.rs, lib.rs or mod.rs,
// else in the directory named after it. `cfg_attr` gates nothing.
const GATED_MOD = /#\[cfg\(([^\]]*)\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;/g;
function gatedModules(path, source) {
  const dir = /(^|\/)(main|lib|mod)\.rs$/.test(path)
    ? path.replace(/[^/]*$/, "")
    : path.replace(/\.rs$/, "/");
  return [...source.matchAll(GATED_MOD)]
    .filter(([, cfg]) => holdsOnWindows(cfg))
    .flatMap(([, , name]) => [`${dir}${name}.rs`, `${dir}${name}/mod.rs`]);
}

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

test("windows-gate lists every Rust file a cfg that holds on Windows reaches", () => {
  const patterns = gatePatterns().map(toRegExp);
  const rust = execFileSync("git", ["ls-files", "-z", "*.rs"], { cwd: repo, encoding: "utf8" })
    .split("\0")
    .filter(Boolean);
  const windows = new Set();
  for (const path of rust) {
    const source = readFileSync(new URL(path, repo), "utf8");
    if ((source.match(CFG) ?? []).some(holdsOnWindows)) windows.add(path);
    for (const child of gatedModules(path, source)) windows.add(child);
  }
  const missing = rust.filter(
    (path) => windows.has(path) && !patterns.some((pattern) => pattern.test(path)),
  );
  assert.deepEqual(missing, [], `add to windows-gate's files in tests.yml: ${missing.join(", ")}`);
});
