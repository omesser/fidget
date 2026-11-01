// landing-link-click.win.ps1 waits for the click's real outcome. Get-LinkOutcome
// is loaded through the parser, so the leaf's launch never runs.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { test } from "node:test";

import { resolvePwsh } from "./pwsh-resolve.js";

const script = join(import.meta.dirname, "..", "scripts", "scenarios", "ci", "landing-link-click.win.ps1");
const resolved = resolvePwsh();
const skip = resolved.kind === "ready" ? false : resolved.reason;

const LANDING = ["window|BMO", "label|Codex needs npx", "link|https://nodejs.org/"];

function outcome(t, dump, titles) {
  const command = `
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($env:LEAF, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw "parse errors: $($errors | Out-String)" }
$fn = $ast.FindAll({ $args[0] -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $args[0].Name -eq "Get-LinkOutcome" }, $true)
if ($fn.Count -ne 1) { throw "missing Get-LinkOutcome" }
Invoke-Expression $fn[0].Extent.Text
$o = Get-LinkOutcome ($env:DUMP -split "\`n") ($env:TITLES -split "\`n" | Where-Object { $_ })
if ($null -eq $o) { "none" } else { $o }
`;
  const run = spawnSync(resolved.command, [...(resolved.args ?? []), "-NoProfile", "-NonInteractive", "-Command", command], {
    encoding: "utf8",
    env: { ...process.env, LEAF: script, DUMP: dump.join("\n"), TITLES: titles.join("\n") },
  });
  assert.equal(run.status, 0, `pwsh failed:\n${run.stdout}\n${run.stderr}`);
  return run.stdout.trim();
}

test("an unchanged landing with no browser window is no outcome yet", { skip }, (t) => {
  assert.equal(outcome(t, LANDING, ["BMO", "Fidget"]), "none");
});

test("a new window on an unchanged landing is not the handoff", { skip }, (t) => {
  assert.equal(outcome(t, LANDING, ["Notepad"]), "none");
});

test("the did-not-open note is a refusal", { skip }, (t) => {
  assert.equal(outcome(t, [...LANDING, "label|That link did not open: no handler."], []), "refused");
});

test("a landing gone from Chat is a webview navigation", { skip }, (t) => {
  assert.equal(outcome(t, ["window|BMO", "web-area|Node.js"], []), "navigated");
});
