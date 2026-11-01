// Later in _NET_CLIENT_LIST_STACKING is in front. A flipped comparison would
// pass a Settings window that sits under the overlay. xprop is a fixture, so
// this runs with no display.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";

import { tempDir } from "./temp-path.js";

const script = path.join(import.meta.dirname, "..", "scripts", "verify-settings-zorder-x11.sh");

function functionBody(name) {
  const text = fs.readFileSync(script, "utf8");
  const start = text.indexOf(`${name}() {`);
  assert.notEqual(start, -1, `${name} is missing from the verifier`);
  let depth = 0;
  for (let i = text.indexOf("{", start); i < text.length; i++) {
    if (text[i] === "{") depth++;
    else if (text[i] === "}") {
      depth--;
      if (depth === 0) return text.slice(start, i + 1);
    }
  }
  throw new Error(`unclosed ${name}`);
}

function run(t, line, overlayId, settingsId) {
  const dir = tempDir(t, "zorder-stack-");
  fs.writeFileSync(path.join(dir, "xprop"), `#!/usr/bin/env bash\necho '${line}'\n`, { mode: 0o755 });
  const body = `${functionBody("stacking_ids")}\n${functionBody("settings_stacks_above")}\n`;
  return spawnSync(
    "bash",
    ["-c", `set -euo pipefail\n${body}\nOVERLAY_ID=${overlayId}\nSETTINGS_ID=${settingsId}\nif settings_stacks_above; then echo yes; else echo no; fi\nprintf 'pos:%s:%s\\n' "$OVERLAY_POS" "$SETTINGS_POS"\nstacking_ids`],
    { env: { ...process.env, PATH: `${dir}${path.delimiter}${process.env.PATH}` }, encoding: "utf8" },
  );
}

const stacking = "_NET_CLIENT_LIST_STACKING(WINDOW): window id # 0x10, 0x20";

test("Settings later in the stacking list is above the overlay", (t) => {
  const r = run(t, stacking, 16, 32);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /^yes\npos:1:2\n16\n32\n$/);
});

test("Settings earlier in the stacking list is under the overlay", (t) => {
  const r = run(t, stacking, 32, 16);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /^no\npos:2:1\n/);
});

test("a window missing from the stacking list is not above", (t) => {
  const r = run(t, stacking, 16, 99);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /^no\npos:1:\n/);
});
