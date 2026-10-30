// #1503's bar for a live scenario leaf: it waits through a shared helper,
// seeds the first-run tour, prints how long it took, and leaves no fixture
// blocked or running behind it.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";

const root = path.join(import.meta.dirname, "..");
const scenarios = path.join(root, "scripts", "scenarios");

// A person drives these, so they keep person-sized waits (#1503, 2026-10-16).
const PERSON_DRIVEN = new Set(["hero-gif.sh", "antigravity-sign-in.sh"]);

const leaves = ["ci", "manual-only"]
  .flatMap((dir) =>
    fs
      .readdirSync(path.join(scenarios, dir))
      .filter((name) => /\.(sh|ps1)$/.test(name) && !PERSON_DRIVEN.has(name))
      .map((name) => path.join(scenarios, dir, name)),
  )
  .sort();

const read = (file) => fs.readFileSync(file, "utf8");
const rel = (file) => path.relative(root, file);

function missing(rule) {
  return leaves.filter((leaf) => !rule(read(leaf), leaf)).map(rel);
}

test("the scan finds the leaves", () => {
  assert.ok(leaves.length >= 30, `found only ${leaves.length} leaves`);
});

test("every leaf waits through its platform's shared helper", () => {
  assert.deepEqual(
    missing((text) => /helpers\/wait-helpers\.(?:mac|x11)\.sh|wait-helpers\.win\.ps1/.test(text)),
    [],
  );
});

test("every leaf seeds the first-run tour as shown", () => {
  assert.deepEqual(
    missing((text) => /seed_tour_shown|Initialize-ScenarioData|first_run_tour_shown/.test(text)),
    [],
  );
});

test("every leaf prints how long its run took", () => {
  assert.deepEqual(
    missing((text) => /(?:now_ms\)|Get-NowMs\)|elapsed_?[mM]s\})[^\n]*ms\)/.test(text)),
    [],
  );
});

test("every Windows leaf ends the app and its Harness as one tree", () => {
  assert.deepEqual(
    missing((text, leaf) => !leaf.endsWith(".win.ps1") || /\bStop-FidgetTree\b/.test(text)),
    [],
  );
});

// The fixture's ask holds the turn until someone answers it.
test("every script that starts the asking fixture answers the ask", () => {
  const scripts = fs
    .readdirSync(path.join(root, "scripts"), { recursive: true })
    .map((name) => path.join(root, "scripts", name))
    .filter((file) => /\.(sh|ps1)$/.test(file) && !file.includes(`${path.sep}helpers${path.sep}`))
    .filter((file) => read(file).includes("script=scenario-asking"));
  assert.ok(scripts.length >= 3, `found only ${scripts.length} asking scripts`);
  const unanswered = scripts
    .filter((file) => !/answer_permission|"Allow"/.test(read(file)))
    .map(rel);
  assert.deepEqual(unanswered, []);
});
