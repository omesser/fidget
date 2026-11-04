// start() used to await character (and more) before overlay_listening. On a
// cold WebView2, NavigationCompleted can fire while those awaits are still in
// flight, so PAGE_LOAD_LIMIT gave up before the page reported it listens.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const main = readFileSync(new URL("../src/main.js", import.meta.url), "utf8");
const start = main.indexOf("async function start() {");
const end = main.indexOf("\nstart().catch", start);
assert.notEqual(start, -1, "start() is still defined");
assert.notEqual(end, -1, "start() is still invoked");
const body = main.slice(start, end);

test("start reports overlay_listening before awaiting character", () => {
  const listening = body.indexOf('invoke("overlay_listening")');
  const character = body.indexOf('invoke("character")');
  assert.notEqual(listening, -1, "start still invokes overlay_listening");
  assert.notEqual(character, -1, "start still invokes character");
  assert.ok(
    listening < character,
    "overlay_listening must run before the character await so a slow art fetch cannot burn the wake hold",
  );
});
