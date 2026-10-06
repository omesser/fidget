// `.agents/skills/` is one flat directory with three owners: the pstack sync,
// the Matt Pocock sync, and this repository. Each sync deletes by name from its
// own lock file, so the lock files are the only thing standing between a sync
// and somebody else's skill. Two lists that both claim a name mean whichever
// sync ran last wins and the other's skill is silently gone.
//
// Prose in `docs/agents/` says the lists are disjoint. This asserts it, because
// the failure is invisible: the loser is a directory that still exists, holding
// the wrong skill under the right name.

import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const root = new URL("../", import.meta.url);
const path = (p) => fileURLToPath(new URL(p, root));

const SKILLS = ".agents/skills";

// The lock files, and the script that rewrites each. The script name is in the
// failure message because the fix is almost never to edit the lock file by hand.
const OWNERS = [
  { name: "pstack", meta: ".agents/pstack/UPSTREAM.json", script: "scripts/sync-pstack.sh" },
  { name: "mattpocock", meta: ".agents/mattpocock/UPSTREAM.json", script: "scripts/sync-mattpocock.sh" },
];

// Skills this repository wrote. Neither sync may ever claim one: a claim is
// what makes it eligible for deletion when it goes missing upstream, and no
// upstream has ever heard of these.
const REPO_OWNED = ["resolving-merge-conflicts", "verify-fidget"];

const owned = (meta) => JSON.parse(readFileSync(path(meta), "utf8")).skills;

const dirsIn = (dir) =>
  readdirSync(path(dir), { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => e.name)
    .sort();

test("every owner's lock file exists and claims at least one skill", () => {
  for (const { name, meta, script } of OWNERS) {
    assert.ok(existsSync(path(meta)), `${meta} is missing; run ${script} --fetch`);
    const names = owned(meta);
    assert.ok(
      Array.isArray(names) && names.length > 0,
      `${name} claims no skills, so every assertion below would pass over nothing`,
    );
  }
});

test("no two syncs claim the same skill name", () => {
  for (let i = 0; i < OWNERS.length; i++) {
    for (let j = i + 1; j < OWNERS.length; j++) {
      const a = OWNERS[i];
      const b = OWNERS[j];
      const both = owned(a.meta).filter((n) => owned(b.meta).includes(n));
      assert.deepEqual(
        both,
        [],
        `${a.name} and ${b.name} both claim ${both.join(", ")}. ` +
          `${SKILLS} is flat, so one copy of those bytes exists and whichever of ` +
          `${a.script} / ${b.script} ran last decides what it holds. Exclude the ` +
          `name from one sync — ${b.script} reserves every name the pstack lock ` +
          `file claims, which is how tdd is kept out.`,
      );
    }
  }
});

test("no sync claims a skill this repository owns", () => {
  for (const { name, meta, script } of OWNERS) {
    const claimed = owned(meta).filter((n) => REPO_OWNED.includes(n));
    assert.deepEqual(
      claimed,
      [],
      `${name} claims ${claimed.join(", ")}, which this repository wrote. ` +
        `A claimed name is one ${script} may delete the moment upstream stops ` +
        `shipping it, and upstream never shipped it at all.`,
    );
  }
});

test("every claimed skill exists on disk with a SKILL.md", () => {
  for (const { name, meta, script } of OWNERS) {
    for (const skill of owned(meta)) {
      const file = `${SKILLS}/${skill}/SKILL.md`;
      assert.ok(
        existsSync(path(file)),
        `${name} claims ${skill} but ${file} does not exist. The lock file and ` +
          `the tree have drifted; run ${script} --fetch.`,
      );
    }
  }
});

test("every skill on disk has exactly one owner", () => {
  // The partition is what makes the two assertions above meaningful: without
  // it, a skill could dodge them by being claimed by nobody, and then no sync
  // maintains it and nothing says so.
  const claims = new Map();
  for (const { name, meta } of OWNERS) {
    for (const skill of owned(meta)) claims.set(skill, name);
  }
  for (const skill of REPO_OWNED) claims.set(skill, "this repository");

  const orphans = dirsIn(SKILLS).filter((d) => !claims.has(d));
  assert.deepEqual(
    orphans,
    [],
    `${orphans.join(", ")} in ${SKILLS} belongs to no owner. Either a sync ` +
      `should claim it, or it is this repository's and belongs in REPO_OWNED ` +
      `here and in tests/skills-layout.test.js. An unowned skill is one no sync ` +
      `updates and no test protects.`,
  );
});
