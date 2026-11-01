// The bar for a live scenario leaf: it waits through a shared helper, seeds
// the first-run tour, prints how long it took, and leaves no app or fixture
// blocked or running behind it.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";

const root = path.join(import.meta.dirname, "..");
const scenarios = path.join(root, "scripts", "scenarios");

// A person drives these, so they keep person-sized waits.
const PERSON_DRIVEN = new Set(["hero-gif.sh", "antigravity-sign-in.sh"]);

const leaves = ["ci", "manual-only"]
  .flatMap((dir) =>
    fs
      .readdirSync(path.join(scenarios, dir))
      .filter((name) => /\.(sh|ps1)$/.test(name) && !PERSON_DRIVEN.has(name))
      .map((name) => path.join(scenarios, dir, name)),
  )
  .sort();

// Code only: a rule a comment could satisfy proves nothing.
const code = (file) =>
  fs
    .readFileSync(file, "utf8")
    .split("\n")
    .filter((line) => !/^\s*#/.test(line))
    .join("\n");
const rel = (file) => path.relative(root, file);

function missing(files, rule) {
  return files.filter((file) => !rule(code(file), file)).map(rel);
}

// A scan that finds nothing would pass every rule below.
test("the scan finds the leaves", () => {
  assert.ok(leaves.length >= 30, `found only ${leaves.length} leaves`);
});

test("every leaf waits through its platform's shared helper", () => {
  assert.deepEqual(
    missing(leaves, (text) =>
      /helpers\/wait-helpers\.(?:mac|x11)\.sh|wait-helpers\.win\.ps1/.test(text),
    ),
    [],
  );
});

test("every leaf seeds the first-run tour as shown", () => {
  assert.deepEqual(
    missing(leaves, (text) =>
      /seed_tour_shown|Initialize-ScenarioData|first_run_tour_shown/.test(text),
    ),
    [],
  );
});

test("every leaf prints how long its run took", () => {
  assert.deepEqual(
    missing(leaves, (text) =>
      /(?:now_ms\)|Get-NowMs\)|elapsed_?[mM]s\})[^\n]*ms\)/.test(text),
    ),
    [],
  );
});

test("every bash leaf cleans up on exit", () => {
  assert.deepEqual(
    missing(leaves, (text, leaf) => !leaf.endsWith(".sh") || /^\s*trap .*\bEXIT\b/m.test(text)),
    [],
  );
});

test("every Windows leaf ends the app and its Harness as one tree", () => {
  assert.deepEqual(
    missing(leaves, (text, leaf) => !leaf.endsWith(".win.ps1") || /\bStop-FidgetTree\b/.test(text)),
    [],
  );
});

// Windows has no gesture timing to hold, so a sleep there only stands in for
// a wait on state.
test("no Windows leaf or verifier sleeps between steps", () => {
  const windows = [
    ...leaves.filter((leaf) => leaf.endsWith(".ps1")),
    path.join(root, "scripts", "verify-chat-perch-win.ps1"),
  ];
  assert.deepEqual(
    missing(windows, (text) => !/\bStart-Sleep\b/i.test(text)),
    [],
  );
});

// The fixture's ask holds the turn until someone answers it.
test("every script that starts the asking fixture answers the ask", () => {
  const asking = fs
    .readdirSync(path.join(root, "scripts"), { recursive: true })
    .map((name) => path.join(root, "scripts", name))
    .filter((file) => /\.(sh|ps1)$/.test(file) && !file.includes(`${path.sep}helpers${path.sep}`))
    .filter((file) => code(file).includes("script=scenario-asking"));
  assert.ok(asking.length >= 3, `found only ${asking.length} asking scripts`);
  assert.deepEqual(
    missing(asking, (text) => /answer_permission "\$|Arg3 = "Allow"/.test(text)),
    [],
  );
});
