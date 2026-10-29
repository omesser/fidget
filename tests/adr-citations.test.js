// A comment citing a superseded ADR sends the reader to a document whose
// first line points somewhere else, and a citation reads as authority.
// A removed ADR sends them nowhere. Markdown and Dated design pages stay exempt.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const repo = new URL("..", import.meta.url);

// Empty on purpose. A code citation of a superseded or removed ADR fails this test.
// Markdown, `docs/`, `website/` and `.agents/` stay exempt. Narrating a supersession is
// what prose is for, and a Dated page keeps the link it shipped with.
const KNOWN_STALE = {};

const tracked = (...args) =>
  execFileSync("git", ["ls-files", "-z", ...args], {
    cwd: repo,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean);

function liveAdrs() {
  const numbers = new Set();
  for (const path of tracked("docs/adr")) {
    const status = readFileSync(new URL(path, repo), "utf8").match(
      /^\*\*Status:\*\*\s*(.*)$/m,
    );
    if (!(status && /^superseded/i.test(status[1]))) {
      numbers.add(path.match(/\d{4}/)[0]);
    }
  }
  return numbers;
}

function citedStale(live, paths = tracked()) {
  const counts = {};
  for (const path of paths) {
    const exempt =
      path.endsWith(".md") ||
      path.startsWith("docs/") ||
      path.startsWith("website/") ||
      path.startsWith(".agents/");
    if (exempt) continue;
    let text;
    try {
      text = readFileSync(new URL(path, repo), "utf8");
    } catch (error) {
      // A file deleted but not staged, or a tracked symlink to a directory.
      // Neither has text of its own to cite from.
      if (error.code === "ENOENT" || error.code === "EISDIR") continue;
      throw error;
    }
    const hits = text.match(/ADR[- ]?\d{4}|adr\/\d{4}-/g) ?? [];
    const stale = hits.filter(
      (hit) => !live.has(hit.match(/\d{4}/)[0]),
    ).length;
    if (stale) counts[path] = stale;
  }
  return counts;
}

test("no code cites a superseded or removed ADR", () => {
  const live = liveAdrs();
  assert.ok(live.size > 0, "docs/adr still lists ADRs");
  assert.deepEqual(
    citedStale(live),
    KNOWN_STALE,
    "a count that rose is a new stale citation; one that fell means KNOWN_STALE is behind",
  );
});

test("a missing file or a directory symlink holds no citation", () => {
  assert.deepEqual(
    citedStale(new Set(), ["no-such-file.js", ".claude/skills"]),
    {},
  );
});

test("any other read error fails the run", () => {
  assert.throws(() => citedStale(new Set(), ["AGENTS.md/child"]), {
    code: "ENOTDIR",
  });
});
