import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
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

const topAllocators = join(import.meta.dirname, "..", "scripts", "heap-top-allocators.sh");
const fixture = join(import.meta.dirname, "fixtures", "malloc-history-allbysize.txt");

test("heap-top-allocators ranks live bytes by the innermost frame in the named image", () => {
  const result = spawnSync("/bin/bash", [topAllocators, fixture, "libperl.dylib", "3"], {
    encoding: "utf8",
    timeout: 5000,
  });

  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    result.stdout,
    `top 3 allocators in libperl.dylib, live bytes by innermost frame
   10172 KB    2036 calls  Perl_safesysmalloc
     192 KB       2 calls  ???
      52 KB       2 calls  Perl_safesysrealloc
   10464 KB live in total, 0 KB of it with no libperl.dylib frame
`,
  );
});

test("--heap launches with MallocStackLogging and writes the fidget pid's reports before the kill", () => {
  const dir = mkdtempSync(join(tmpdir(), "bench-rss-macos-"));
  const home = join(dir, "home");
  const tools = join(dir, "tools");
  const heap = join(dir, "heap");
  const bin = join(dir, "fakefid");
  mkdirSync(tools);
  writeFileSync(
    bin,
    `#!/bin/bash
echo "$$ $MallocStackLogging" > "${dir}/app"
mkdir -p "$HOME/Library/Application Support/fidget"
printf 'overlay: 1 display\\n' >> "$HOME/Library/Application Support/fidget/process.log"
exec sleep 30
`,
  );
  writeFileSync(
    join(tools, "malloc_history"),
    `#!/bin/bash
kill -0 "$1" && echo "$@" > "${dir}/malloc_history.args" && cat "${fixture}"
`,
  );
  writeFileSync(
    join(tools, "footprint"),
    `#!/bin/bash
kill -0 "$2" && echo "$@" > "${dir}/footprint.args" && echo "fake [$2]: 64-bit    Footprint: 14 MB"
`,
  );
  for (const file of [bin, join(tools, "malloc_history"), join(tools, "footprint")]) chmodSync(file, 0o755);

  const result = spawnSync(
    "/bin/bash",
    [script, "--bin", bin, "--settle", "0", "--seconds", "0", "--out", join(dir, "rss.tsv"), "--heap", heap],
    { encoding: "utf8", timeout: 15000, env: { ...process.env, HOME: home, PATH: `${tools}:${process.env.PATH}` } },
  );

  assert.equal(result.status, 0, result.stderr);
  const [pid, logging] = readFileSync(join(dir, "app"), "utf8").trim().split(" ");
  assert.equal(logging, "1");
  assert.equal(readFileSync(join(dir, "malloc_history.args"), "utf8"), `${pid} -allBySize\n`);
  assert.equal(readFileSync(join(dir, "footprint.args"), "utf8"), `-p ${pid}\n`);
  assert.equal(readFileSync(join(heap, "malloc-history.txt"), "utf8"), readFileSync(fixture, "utf8"));
  assert.equal(readFileSync(join(heap, "footprint.txt"), "utf8"), `fake [${pid}]: 64-bit    Footprint: 14 MB\n`);
  assert.match(result.stdout, /^heap run: MallocStackLogging inflates footprint/m);
  assert.equal(
    result.stdout.split("top 20 allocators")[1],
    ` in fidget, live bytes by innermost frame
   10464 KB live in total, 10464 KB of it with no fidget frame
`,
  );
});
