import assert from "node:assert/strict";
import { test } from "node:test";
import { importMessage, removeMessage } from "../src/character-manager.js";

test("import message explains the only required next step", () => {
  assert.match(importMessage("Dosidicus Squid"), /Dosidicus Squid installed/);
  assert.match(importMessage("Dosidicus Squid"), /Quit and reopen Fidget/);
});

test("remove message explains why an old menu may still show an entry", () => {
  assert.match(removeMessage("Dosidicus Squid"), /Restart Fidget/);
});
