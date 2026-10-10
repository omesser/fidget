// poke-mid-climb.win.ps1 picks a place target from overlay "covers WxH at (x,y)"
// lines. Get-ClimbEdges is loaded through the parser, so the leaf's launch never runs.
// The two lines are a primary at the origin and a portrait display to its left.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { test } from "node:test";

import { resolvePwsh } from "./pwsh-resolve.js";

const script = join(import.meta.dirname, "..", "scripts", "scenarios", "poke-mid-climb.win.ps1");

const resolved = resolvePwsh();
const skip = resolved.kind === "ready" ? false : resolved.reason;

const LINES = [
  "overlay: overlay-0 covers 3440x1440 at (0,0)",
  "overlay: overlay-1 covers 1200x1920 at (-1200,-209)",
];

const LOADER = `
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($env:CLIMB_SCRIPT, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw "parse errors: $($errors | Out-String)" }
$fn = $ast.FindAll({ $args[0] -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $args[0].Name -eq "Get-ClimbEdges" }, $true)
if ($fn.Count -ne 1) { throw "missing function Get-ClimbEdges" }
Invoke-Expression $fn[0].Extent.Text
$edges = Get-ClimbEdges ($env:CLIMB_LINES -split "\`n")
if ($null -eq $edges) { throw "no edges" }
Write-Output "$($edges.Left) $($edges.Right) $($edges.LeftTarget) $($edges.RightTarget)"
`;

const PWSH_HOST_ABORT =
  /FileLoadException|Microsoft\.Management\.Infrastructure|Abort trap|not properly handled/;

function pwshHostAborted(run) {
  return PWSH_HOST_ABORT.test(`${run?.stdout ?? ""}\n${run?.stderr ?? ""}`);
}

function edges(t) {
  const run = spawnSync(resolved.command, [...(resolved.args ?? []), "-NoProfile", "-NonInteractive", "-Command", LOADER], {
    encoding: "utf8",
    env: { ...process.env, CLIMB_SCRIPT: script, CLIMB_LINES: LINES.join("\n") },
  });
  if (run.status !== 0 && pwshHostAborted(run)) {
    t.skip("pwsh aborted before the climb leaf ran");
    return null;
  }
  assert.equal(run.status, 0, `pwsh failed:\n${run.stdout}\n${run.stderr}`);
  const [left, right, leftTarget, rightTarget] = run.stdout.trim().split(/\s+/).map(Number);
  return { left, right, leftTarget, rightTarget };
}

test("the right edge target is the far side of the wide display", { skip }, (t) => {
  const got = edges(t);
  if (got == null) return;
  assert.deepEqual(got, { left: -1200, right: 3440, leftTarget: -1200, rightTarget: 3439 });
});
