import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, writeFileSync } from "node:fs";
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

test("an absolute --bin is launched, and the overlay count is read from process.log", () => {
  const dir = mkdtempSync(join(tmpdir(), "bench-rss-macos-"));
  const home = join(dir, "home");
  const bin = join(dir, "fakefid");
  const out = join(dir, "rss.tsv");
  writeFileSync(
    bin,
    `#!/bin/bash
mkdir -p "$HOME/Library/Application Support/fidget"
printf 'overlay: 2 displays\\n' >> "$HOME/Library/Application Support/fidget/process.log"
`,
  );
  chmodSync(bin, 0o755);

  const result = spawnSync(
    "/bin/bash",
    [script, "--bin", bin, "--settle", "0", "--seconds", "0", "--out", out],
    { encoding: "utf8", timeout: 15000, env: { ...process.env, HOME: home } },
  );

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /displays: 2/);
  assert.equal(result.stderr.includes("never reported its overlays"), false);
});
