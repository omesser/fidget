import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const script = join(import.meta.dirname, "..", "scripts", "bench-rss-macos.sh");

test("--bin /nonexistent fails with the missing-binary error and launches nothing", () => {
  const dir = mkdtempSync(join(tmpdir(), "bench-rss-macos-"));
  const out = join(dir, "rss.tsv");
  const result = spawnSync(
    "/bin/bash",
    [script, "--bin", "/nonexistent", "--out", out],
    { encoding: "utf8", timeout: 5000 },
  );

  assert.equal(result.status, 2);
  assert.equal(
    result.stderr,
    "no /nonexistent — run: (cd src-tauri && cargo build --bin fidget)\n",
  );
  assert.equal(result.stdout, "");
  assert.equal(existsSync(out), false);
  assert.equal(existsSync(`${out}.app.log`), false);
});
