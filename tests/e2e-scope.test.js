import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { tempDir } from "./temp-path.js";

const script = join(import.meta.dirname, "..", "scripts", "e2e-scope.mjs");

// A scenario tree: each key a path under the repository, each value its text.
function repo(t, files) {
  const dir = tempDir(t, "e2e-scope-");
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, path)), { recursive: true });
    writeFileSync(join(dir, path), text);
  }
  return dir;
}

function scope(dir, platform, changed) {
  return execFileSync("node", [script, platform], { cwd: dir, input: changed.join("\n") + "\n", encoding: "utf8" });
}

const ci = "scripts/scenarios/ci";
const helpers = "scripts/scenarios/helpers";

test("a changed leaf runs only that leaf", (t) => {
  const dir = repo(t, {
    [`${ci}/control-click-menu.win.ps1`]: "",
    [`${ci}/question-bubble.win.ps1`]: "",
  });
  assert.equal(scope(dir, "windows", [`${ci}/control-click-menu.win.ps1`]), "run=true\nscenarios=control-click-menu\n");
});

test("a leaf of another platform runs nothing", (t) => {
  const dir = repo(t, {
    [`${ci}/thinking-row.sh`]: "",
    [`${ci}/thinking-row.x11.sh`]: "",
    [`${ci}/thinking-row.win.ps1`]: "",
  });
  assert.equal(scope(dir, "macos", [`${ci}/thinking-row.x11.sh`, `${ci}/thinking-row.win.ps1`]), "run=false\nscenarios=\n");
});

test("a deleted leaf is not run", (t) => {
  const dir = repo(t, { [`${ci}/thinking-row.x11.sh`]: "" });
  assert.equal(scope(dir, "linux", [`${ci}/gone.x11.sh`, `${ci}/thinking-row.x11.sh`]), "run=true\nscenarios=thinking-row\n");
});

test("a helper a leaf uses runs every leaf", (t) => {
  const dir = repo(t, {
    [`${ci}/thinking-row.win.ps1`]: `$h = "$root/scripts/scenarios/helpers/fixture-harness.sh"\n`,
    [`${ci}/glass-tint.win.ps1`]: "",
    [`${helpers}/fixture-harness.sh`]: "",
  });
  assert.equal(
    scope(dir, "windows", [`${helpers}/fixture-harness.sh`, `${ci}/glass-tint.win.ps1`]),
    "run=true\nscenarios=\n",
  );
});

test("a helper another helper loads runs every leaf", (t) => {
  const dir = repo(t, {
    [`${ci}/chat-level.sh`]: `. "$root/scripts/scenarios/helpers/wait-helpers.mac.sh"\n`,
    [`${helpers}/wait-helpers.mac.sh`]: `. "$(dirname "$0")/wait-helpers.sh"\n`,
    [`${helpers}/wait-helpers.sh`]: "",
  });
  assert.equal(scope(dir, "macos", [`${helpers}/wait-helpers.sh`]), "run=true\nscenarios=\n");
});

test("a helper only another platform loads runs nothing", (t) => {
  const dir = repo(t, {
    [`${ci}/thinking-row.win.ps1`]: `. "$PSScriptRoot/../helpers/wait-helpers.win.ps1"\n`,
    [`${ci}/thinking-row.x11.sh`]: `. "$root/scripts/scenarios/helpers/wait-helpers.x11.sh"\n`,
    [`${helpers}/wait-helpers.win.ps1`]: "",
    [`${helpers}/wait-helpers.x11.sh`]: "",
  });
  assert.equal(scope(dir, "windows", [`${helpers}/wait-helpers.x11.sh`]), "run=false\nscenarios=\n");
});
