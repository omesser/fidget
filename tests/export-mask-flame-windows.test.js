// scripts/export-mask-flame-windows.ps1 folds the CSV that wpaexporter writes
// into flamegraph input. The fixture follows the documented layout of the
// UIforETW profile's export (Process, Thread ID, Stack; one row a sample),
// not a capture from a real trace.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { resolvePwsh } from "./pwsh-resolve.js";
import { tempDir } from "./temp-path.js";

const script = join(import.meta.dirname, "..", "scripts", "export-mask-flame-windows.ps1");
const fixture = join(import.meta.dirname, "fixtures", "wpa-cpu-usage-sampled.csv");
const resolved = resolvePwsh();
const skip = resolved.kind === "ready" ? false : resolved.reason;

function fold(t) {
  const out = tempDir(t, "export-mask-flame-");
  const run = spawnSync(
    resolved.command,
    [...(resolved.args ?? []), "-NoProfile", "-NonInteractive", "-File", script, "-Csv", fixture, "-Out", out],
    { encoding: "utf8", timeout: 60000, env: { ...process.env, TERM: "dumb" } },
  );
  const read = (name) => readFileSync(join(out, name), "utf8").trim().split(/\r?\n/);
  return { run, read };
}

test("each fidget stack folds root first, with its sample count", { skip }, (t) => {
  const { run, read } = fold(t);
  assert.equal(run.status, 0, run.stdout + run.stderr);
  const folded = read("folded.txt");
  assert.ok(
    folded.includes(
      "ntdll.dll!RtlUserThreadStart;kernel32.dll!BaseThreadInitThunk;fidget.exe!fidget::frame_loop::run;" +
        "fidget.exe!fidget::platform::windows::overlay::update_input_region;user32.dll!SetWindowRgn 2",
    ),
    folded.join("\n"),
  );
  assert.ok(folded.includes("[no stack] 1"), "an n/a stack still counts");
  assert.ok(!folded.some((line) => line.includes("msedge")), "another process's samples stay out");
});

test("a quoted stack keeps its comma and loses its semicolon", { skip }, (t) => {
  const { read } = fold(t);
  assert.ok(
    read("folded.txt").includes(
      "ntdll.dll!RtlUserThreadStart;fidget.exe!core::ptr::drop_in_place<alloc::vec::Vec<u8, alloc::alloc::Global>>;fidget.exe!a:b 1",
    ),
  );
});

test("the mask fold keeps only stacks through the region calls", { skip }, (t) => {
  const { run, read } = fold(t);
  assert.deepEqual(
    read("mask-folded.txt").map((line) => line.split(";").at(-1)),
    ["gdi32.dll!CombineRgn 1", "user32.dll!SetWindowRgn 2"],
  );
  assert.match(run.stdout, /fidget\.exe samples: 5 /);
  assert.match(run.stdout, /mask path samples: 3 /);
});
