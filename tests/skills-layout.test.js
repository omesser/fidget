// `.agents/skills/` holds every skill this repository offers an agent, and
// `.claude/skills` and `.cursor/skills` are symlinks to it, since both loaders
// glob `<dir>/skills/*/SKILL.md`. A broken link makes every skill silently invisible.

import assert from "node:assert/strict";
import { lstatSync, readFileSync, readdirSync, readlinkSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const root = new URL("../", import.meta.url);
const path = (p) => fileURLToPath(new URL(p, root));

// Every link `scripts/sync-pstack.sh` creates, and what each must point at.
// `.claude/agents` is here for the same reason as the other two: the sync
// creates it, so a break in it is a break this test is claimed to catch.
const LINKS = [
  [".claude/skills", "../.agents/skills"],
  [".cursor/skills", "../.agents/skills"],
  [".claude/agents", "../.agents/agents"],
];

// Where the skill bytes live, and the links that must reach them. `.agents/agents`
// holds agent definitions rather than skills, so the skill-shaped assertions
// below run over these two only.
const REAL = ".agents/skills";
const SKILL_LINKS = LINKS.filter(([, target]) => target === `../${REAL}`);

// Skills this repository owns rather than vendors. They prove the directory is
// shared, so a sync that blew it away would fail here rather than in six
// months when someone next runs the verifier.
const OURS = ["resolving-merge-conflicts", "verify-fidget"];

const skillsIn = (dir) =>
  readdirSync(path(dir), { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => e.name)
    .sort();

test("the real skills directory holds a non-zero number of skills", () => {
  const names = skillsIn(REAL);
  assert.ok(
    names.length > 0,
    `${REAL} is empty; every assertion below would pass over nothing`,
  );
  for (const skill of OURS) {
    assert.ok(
      names.includes(skill),
      `${REAL} has lost ${skill}, which this repository owns. No sync may ever remove it.`,
    );
  }
});

for (const [link, target] of LINKS) {
  test(`${link} is a symlink to ${target}`, () => {
    let stat;
    try {
      stat = lstatSync(path(link));
    } catch (err) {
      assert.fail(
        `${link} does not exist (${err.code}). Skill discovery is dead. Run scripts/sync-pstack.sh.`,
      );
    }
    assert.ok(
      stat.isSymbolicLink(),
      `${link} is not a symlink. One copy of the bytes is the point; run scripts/sync-pstack.sh.`,
    );
    assert.equal(
      readlinkSync(path(link)),
      target,
      `${link} points somewhere unexpected. Run scripts/sync-pstack.sh.`,
    );
  });

  test(`${link} resolves to a directory`, () => {
    let stat;
    try {
      // statSync follows the link. A dangling one throws here, which is the
      // case a readlink check alone would miss.
      stat = statSync(path(link));
    } catch (err) {
      assert.fail(
        `${link} is dangling (${err.code}); it points at nothing. Run scripts/sync-pstack.sh.`,
      );
    }
    assert.ok(stat.isDirectory(), `${link} does not resolve to a directory`);
  });
}

for (const [link] of SKILL_LINKS) {
  test(`${link} sees the same skills as ${REAL}`, () => {
    const through = skillsIn(link);
    assert.deepEqual(
      through,
      skillsIn(REAL),
      `${link} does not see the same skills as ${REAL}`,
    );
    for (const skill of OURS) {
      assert.ok(through.includes(skill), `${link} cannot see ${skill}`);
    }
  });

  test(`${link} serves a readable SKILL.md for every skill`, () => {
    // What the loader's `<dir>/skills/*/SKILL.md` glob actually needs.
    for (const name of skillsIn(link)) {
      const file = `${link}/${name}/SKILL.md`;
      let body;
      try {
        body = readFileSync(path(file), "utf8");
      } catch (err) {
        assert.fail(`${file} is not readable through the symlink (${err.code})`);
      }
      assert.match(body, /^---\r?\n/, `${file} has no frontmatter`);
    }
  });
}
