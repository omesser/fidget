// landing-link-click.win.ps1 classifies the click from the UI Automation dump
// and the URL string Fidget handed to ShellExecuteW. Get-LinkOutcome is loaded
// through the parser, so the leaf never launches.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { resolvePwsh } from "./pwsh-resolve.js";

const script = join(import.meta.dirname, "..", "scripts", "scenarios", "ci", "landing-link-click.win.ps1");
const resolved = resolvePwsh();
const skip = resolved.kind === "ready" ? false : resolved.reason;
const URL_ = "https://nodejs.org/";

const LANDING = ["window|BMO", "label|Codex needs npx", "link|https://nodejs.org/"];

function outcome(t, dump, handed) {
  const command = `
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($env:LEAF, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw "parse errors: $($errors | Out-String)" }
$fn = $ast.FindAll({ $args[0] -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $args[0].Name -eq "Get-LinkOutcome" }, $true)
if ($fn.Count -ne 1) { throw "missing Get-LinkOutcome" }
Invoke-Expression $fn[0].Extent.Text
$Url = $env:WANT
$o = Get-LinkOutcome ($env:DUMP -split "\`n") $env:HANDED
if ($null -eq $o) { "none" } else { $o }
`;
  const run = spawnSync(resolved.command, [...(resolved.args ?? []), "-NoProfile", "-NonInteractive", "-Command", command], {
    encoding: "utf8",
    env: { ...process.env, LEAF: script, DUMP: dump.join("\n"), HANDED: handed, WANT: URL_ },
  });
  assert.equal(run.status, 0, `pwsh failed:\n${run.stdout}\n${run.stderr}`);
  return run.stdout.trim();
}

test("an empty handoff on an unchanged landing is not opened", { skip }, (t) => {
  assert.equal(outcome(t, LANDING, ""), "none");
});

test("a window title is not the handed URL", { skip }, (t) => {
  assert.equal(outcome(t, LANDING, "Notepad"), "wrong");
});

test("a different URL is not the nodejs.org handoff", { skip }, (t) => {
  assert.equal(outcome(t, LANDING, "https://example.test/"), "wrong");
});

test("https://nodejs.org/ is the handoff", { skip }, (t) => {
  assert.equal(outcome(t, LANDING, "https://nodejs.org/\n"), "opened");
});

test("the did-not-open note is a refusal", { skip }, (t) => {
  assert.equal(outcome(t, [...LANDING, "label|That link did not open: no handler."], URL_), "refused");
});

test("a landing gone from Chat is a webview navigation", { skip }, (t) => {
  assert.equal(outcome(t, ["window|BMO", "web-area|Node.js"], URL_), "navigated");
});

test("the open log wakes the shared directory watcher, not a new UI signal", () => {
  const leaf = readFileSync(script, "utf8");
  const signal = readFileSync(join(import.meta.dirname, "..", "scripts", "uia-signal-win.ps1"), "utf8");
  assert.match(leaf, /\$handoff = Join-Path \$out "handoff"/);
  assert.match(leaf, /Wait-OnChanges 3 \$handoff/);
  assert.doesNotMatch(leaf, /AlsoFile/);
  assert.doesNotMatch(signal, /FileSystemWatcher|AlsoFile/);
});
