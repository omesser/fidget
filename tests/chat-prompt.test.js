// ADR-0012's seam on this side of the wire: saving an Instance Prompt throws
// the Director session away, so the trigger has to be an explicit act. A
// keystroke listener would wipe the conversation mid-sentence, quietly.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const js = readFileSync(new URL("../src/chat.js", import.meta.url), "utf8");
const html = readFileSync(new URL("../src/chat.html", import.meta.url), "utf8");
const css = readFileSync(new URL("../src/chat-ui.css", import.meta.url), "utf8");

function ruleBlock(source, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = source.match(new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{([^}]+)\\}`));
  assert.ok(match, `${selector} has no rule of its own`);
  return match[1];
}

function promptSection(source) {
  const match = source.match(/<section class="prompt"[^>]*>([\s\S]*?)<\/section>/);
  assert.ok(match, "the Prompt tab section is missing");
  return match[1];
}

// Every event this window listens for, as `target.event`, in the order it
// wires them.
const LISTENED = [...js.matchAll(/(\w+)\.addEventListener\(\s*"([a-z-]+)"/g)].map(
  ([, target, name]) => `${target}.${name}`,
);

test("nothing in the Chat surface saves on a keystroke", () => {
  assert.ok(LISTENED.length >= 4, "the listener parse stopped matching — this cannot pass vacuously");

  const typing = LISTENED.filter((wiring) =>
    ["input", "keyup", "keydown", "keypress", "change"].includes(wiring.split(".")[1]),
  );

  // The composer's send key is the one listener that may fire mid-typing. A
  // textarea does not submit on Enter, so Enter is wired by hand and sends a
  // turn. It does not save. Prompt-tab input and change only arm Save and Discard.
  assert.deepEqual(
    [...typing].sort(),
    ["line.keydown", "promptText.change", "promptText.input"].sort(),
    `these fire while the user is still typing, and saving reopens the session: ${typing.join(", ")}`,
  );

  const at = js.indexOf('line.addEventListener("keydown"');
  const end = js.indexOf("\n});", at);
  assert.doesNotMatch(
    js.slice(at, end),
    /invoke\(/,
    "the send key reaches the Shell only through the submit listener, never the save",
  );

  for (const name of ["input", "change"]) {
    const start = js.indexOf(`promptText.addEventListener("${name}"`);
    const stop = js.indexOf(";", start);
    const body = js.slice(start, stop);
    assert.doesNotMatch(body, /invoke\(/, `promptText.${name} must not reach the Shell`);
    assert.doesNotMatch(
      body,
      /askingToSave\(true\)/,
      `promptText.${name} must not open the confirm`,
    );
  }
});

test("the save is asked for twice before the conversation is spent", () => {
  assert.match(js, /promptSave\.addEventListener\("click"/);
  assert.match(js, /promptConfirm\.addEventListener\("click"/);

  // The first click only asks; the call itself hangs off the second.
  const asked = js.indexOf('promptConfirm.addEventListener("click"');
  const called = js.indexOf('invoke("chat_prompt"');
  assert.ok(called > asked, "chat_prompt must be invoked from the confirming click");
  assert.equal(
    js.split('invoke("chat_prompt"').length - 1,
    1,
    "one save path, so one place the warning has to be shown before",
  );
});

function listenerSlice(source, target, event) {
  const token = `${target}.addEventListener("${event}"`;
  const start = source.indexOf(token);
  assert.ok(start >= 0, `${target}.${event} is missing`);
  const next = source.indexOf(".addEventListener(", start + token.length);
  return source.slice(start, next < 0 ? undefined : next);
}

test("Prompt tab Save and Discard stay disabled until the field is dirty", () => {
  const section = promptSection(html);

  assert.match(section, /id="prompt-discard"/, "Discard sits in the sticky footer");
  assert.match(
    section,
    /id="prompt-save"[^>]*\bdisabled\b/,
    "Save starts disabled: a clean field must not open the confirm",
  );
  assert.match(
    section,
    /id="prompt-discard"[^>]*\bdisabled\b/,
    "Discard starts disabled with Save",
  );

  const discard = ruleBlock(css, "#prompt-discard");
  assert.match(discard, /background:\s*transparent/, "Discard is the outline twin of Cancel, not accent");
  assert.match(ruleBlock(css, "#prompt-save:disabled"), /cursor:\s*default|background:\s*var\(--ui-fill-soft\)/);

  assert.match(js, /function promptDirty\(/);
  assert.match(js, /promptText\.value !== savedPrompt/);
  assert.match(
    js,
    /promptSave\.disabled = confirming \|\| !dirty/,
    "Save is disabled while clean or confirming",
  );
  assert.match(
    js,
    /promptDiscard\.disabled = confirming \|\| !dirty/,
    "Discard is disabled while clean or confirming",
  );
  assert.match(js, /promptDiscard\.hidden = asking/, "Discard does not compete with Confirm/Cancel");
  assert.match(js, /showPrompt\([\s\S]*?syncPromptActions\(\)/s);

  const saveClick = listenerSlice(js, "promptSave", "click");
  assert.match(saveClick, /if \(!promptDirty\(\)\)/, "a clean Save click is a no-op, not a confirm");
  assert.match(saveClick, /askingToSave\(true\)/);
});

test("Discard restores the last saved text; Cancel only aborts confirm", () => {
  const discard = listenerSlice(js, "promptDiscard", "click");
  assert.match(discard, /promptText\.value = savedPrompt/, "Discard puts the last saved text back");
  assert.match(discard, /promptSaid\.textContent = ""/);
  assert.match(discard, /askingToSave\(false\)/, "and leaves confirm if it was open");
  assert.doesNotMatch(discard, /invoke\(/, "Discard never reaches the Shell");

  const cancel = listenerSlice(js, "promptCancel", "click");
  assert.match(cancel, /askingToSave\(false\)/);
  assert.doesNotMatch(
    cancel,
    /promptText\.value/,
    "Cancel aborts confirm only; the field keeps the unsaved sentence",
  );
});
