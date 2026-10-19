import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
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
     192 KB       2 calls  Perl_init_i18nl10n
      52 KB       2 calls  Perl_safesysrealloc
   10464 KB live in total, 0 KB of it with no libperl.dylib frame
`,
  );
});

test("heap-top-allocators walks past Rust allocator glue, and keeps it when nothing else is there", () => {
  const dir = mkdtempSync(join(tmpdir(), "bench-rss-macos-"));
  const report = join(dir, "report.txt");
  writeFileSync(
    report,
    `1 call for 4096 bytes: 0x1 (fidget) main | 0x2 (fidget) fidget::overlay::build | 0x3 (fidget) <alloc::vec::Vec<T> as core::clone::Clone>::clone | 0x4 (fidget) alloc::raw_vec::finish_grow | 0x5 (fidget) __rust_alloc | 0x6 (libsystem_malloc.dylib) _malloc
1 call for 1024 bytes: 0x7 (fidget) __rust_alloc | 0x8 (libsystem_malloc.dylib) _malloc
`,
  );

  const result = spawnSync("/bin/bash", [topAllocators, report, "fidget"], { encoding: "utf8", timeout: 5000 });

  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    result.stdout,
    `top 20 allocators in fidget, live bytes by innermost frame
       4 KB       1 calls  fidget::overlay::build
       1 KB       1 calls  __rust_alloc
       5 KB live in total, 0 KB of it with no fidget frame
`,
  );
});

// Real sleeps stand in for WebKit helpers, so the script only ever reads
// processes this test started; fake tools record what they were pointed at.
function heapRun(helperCount) {
  const dir = mkdtempSync(join(tmpdir(), "bench-rss-macos-"));
  const tools = join(dir, "tools");
  const heap = join(dir, "heap");
  const bin = join(dir, "fakefid");
  const helpers = Array.from({ length: helperCount }, () => spawn("sleep", ["30"]));
  const helperPids = helpers.map((helper) => String(helper.pid));
  mkdirSync(tools);
  const fakes = {
    [bin]: `echo "$$ $MallocStackLogging" > "${dir}/app"
mkdir -p "$HOME/Library/Application Support/fidget"
printf 'overlay: 1 display\\n' >> "$HOME/Library/Application Support/fidget/process.log"
exec sleep 30`,
    [join(tools, "pgrep")]: `[ -e "${dir}/pgrep.seen" ] && printf '%s\\n' ${helperPids.join(" ")}
touch "${dir}/pgrep.seen"`,
    [join(tools, "malloc_history")]: `kill -0 "$1" && echo "$@" >> "${dir}/malloc_history.args" && cat "${fixture}"`,
    [join(tools, "footprint")]: `kill -0 "$2" && echo "$@" >> "${dir}/footprint.args" && echo "fake [$2]: 64-bit    Footprint: 14 MB"`,
    [join(tools, "heap")]: `kill -0 "$2" && echo "$@" >> "${dir}/heap.args" && echo "fake heap of $2"`,
  };
  for (const [file, body] of Object.entries(fakes)) {
    writeFileSync(file, `#!/bin/bash\n${body}\n`);
    chmodSync(file, 0o755);
  }

  try {
    const result = spawnSync(
      "/bin/bash",
      [script, "--bin", bin, "--settle", "0", "--seconds", "0", "--out", join(dir, "rss.tsv"), "--heap", heap],
      {
        encoding: "utf8",
        timeout: 15000,
        env: { ...process.env, HOME: join(dir, "home"), PATH: `${tools}:${process.env.PATH}` },
      },
    );
    const [pid, logging] = readFileSync(join(dir, "app"), "utf8").trim().split(" ");
    const read = (name) => (existsSync(join(dir, name)) ? readFileSync(join(dir, name), "utf8") : null);
    return { result, pid, logging, heap, helperPids, read };
  } finally {
    for (const helper of helpers) helper.kill();
  }
}

test("--heap launches with MallocStackLogging and writes fidget's and its helpers' reports before the kill", () => {
  const { result, pid, logging, heap, helperPids, read } = heapRun(3);

  assert.equal(result.status, 0, result.stderr);
  assert.equal(logging, "1");
  assert.equal(read("malloc_history.args"), `${pid} -allBySize\n`);
  assert.equal(read("footprint.args"), [pid, ...helperPids].map((p) => `-p ${p}\n`).join(""));
  assert.equal(read("heap.args"), helperPids.map((p) => `-s ${p}\n`).join(""));
  assert.equal(readFileSync(join(heap, "malloc-history.txt"), "utf8"), readFileSync(fixture, "utf8"));
  assert.equal(readFileSync(join(heap, "footprint.txt"), "utf8"), `fake [${pid}]: 64-bit    Footprint: 14 MB\n`);
  assert.equal(
    readFileSync(join(heap, `footprint-${helperPids[0]}.txt`), "utf8"),
    `fake [${helperPids[0]}]: 64-bit    Footprint: 14 MB\n`,
  );
  assert.equal(readFileSync(join(heap, `heap-${helperPids[2]}.txt`), "utf8"), `fake heap of ${helperPids[2]}\n`);
  assert.match(result.stdout, /^heap run: MallocStackLogging inflates footprint/m);
  assert.equal(
    result.stdout.split("top 20 allocators")[1],
    ` in fidget, live bytes by innermost frame
   10464 KB live in total, 10464 KB of it with no fidget frame
`,
  );
});

test("--heap leaves the helpers alone when the set is not one display's three", () => {
  const { result, pid, read } = heapRun(4);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /^not profiling WebKit helpers: 4 found, 3 expected$/m);
  assert.equal(read("footprint.args"), `-p ${pid}\n`);
  assert.equal(read("heap.args"), null);
});
