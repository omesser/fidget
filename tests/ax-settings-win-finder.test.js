// A Window named Settings is also the Windows Settings app. Live UIA is not
// on this VM or in CI, so this parses Find-SettingsWindow's conditions and
// applies them to fixtures.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const axSrc = readFileSync(join(root, "scripts/ax-settings-win.ps1"), "utf8");

function extractFunction(src, name) {
  const marker = `function ${name}`;
  const start = src.indexOf(marker);
  assert.ok(start >= 0, `missing function ${name}`);
  const brace = src.indexOf("{", start);
  let depth = 0;
  for (let i = brace; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}") {
      depth--;
      if (depth === 0) return src.slice(start, i + 1);
    }
  }
  throw new Error(`unclosed function ${name}`);
}

function parsePropertyConditions(body) {
  const re =
    /\$(\w+)\s*=\s*New-Object System\.Windows\.Automation\.PropertyCondition\(\s*\[System\.Windows\.Automation\.AutomationElement\]::(\w+)\s*,\s*([^)]+?)\s*\)/g;
  return [...body.matchAll(re)].map((m) => ({
    var: m[1],
    property: m[2],
    value: m[3].trim(),
  }));
}

function parseAndConditionArgs(body, targetVar) {
  // targetVar is a literal from this file.
  // nosemgrep: javascript.lang.security.audit.detect-non-literal-regexp.detect-non-literal-regexp
  const re = new RegExp(
    `\\$${targetVar}\\s*=\\s*New-Object System\\.Windows\\.Automation\\.AndCondition\\(([^)]+)\\)`,
    "g",
  );
  return [...body.matchAll(re)].map((m) =>
    m[1]
      .split(",")
      .map((part) => part.trim().replace(/^\$/, ""))
      .filter(Boolean),
  );
}

function unquote(value) {
  const m = value.match(/^"(.*)"$/);
  return m ? m[1] : value;
}

function matches(el, conds) {
  return conds.every((c) => {
    switch (c.property) {
      case "ClassNameProperty":
        return el.className === unquote(c.value);
      case "NameProperty":
        return el.name === unquote(c.value);
      case "ControlTypeProperty":
        if (/ControlType\]::TabItem/.test(c.value)) return el.controlType === "TabItem";
        return el.controlType === "Window" && /ControlType\]::Window/.test(c.value);
      case "ProcessIdProperty":
        return el.processId === el.boundProcessId;
      default:
        throw new Error(`unknown UIA property ${c.property}`);
    }
  });
}

const finder = extractFunction(axSrc, "Find-SettingsWindow");

const webviewConds = parsePropertyConditions(finder);
const webviewByVar = new Map(webviewConds.map((c) => [c.var, c]));
const webviewAnds = parseAndConditionArgs(finder, "webviewCond");

const noPidAnd = webviewAnds.find((args) => !args.includes("pidCond"));
assert.ok(noPidAnd, "webview arm must have a no-ProcessId AndCondition");

const webviewUnconditional = noPidAnd.map((name) => {
  const cond = webviewByVar.get(name);
  assert.ok(cond, `AndCondition names unknown $${name}`);
  return cond;
});

const windowsSettingsApp = {
  className: "ApplicationFrameWindow",
  name: "Settings",
  controlType: "Window",
};
const tauriSettings = {
  className: "Tauri Window",
  name: "Settings",
  controlType: "Window",
  processId: 4242,
  boundProcessId: 4242,
};
const otherTauri = {
  className: "Tauri Window",
  name: "Fidget",
  controlType: "Window",
};

test("Name-only is insufficient: the Windows Settings app is not a match", () => {
  const nameOnly = webviewUnconditional.filter(
    (c) => c.property === "NameProperty" || c.property === "ControlTypeProperty",
  );
  assert.equal(nameOnly.length, 2, "the old two-property matcher is the contrast fixture");
  assert.equal(
    matches(windowsSettingsApp, nameOnly),
    true,
    "Name+Window alone still hits the Windows Settings app",
  );
  assert.equal(matches(windowsSettingsApp, webviewUnconditional), false);
  assert.equal(matches(tauriSettings, webviewUnconditional), true);
  assert.equal(matches(otherTauri, webviewUnconditional), false);
});
