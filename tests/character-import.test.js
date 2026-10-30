import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { mountCharacterImports } from "../src/character-import.js";

// Small DOM harness: exercise the actual mounted UI and asynchronous Tauri
// commands without introducing jsdom or a browser dependency to the test run.
class Element {
  constructor(tag = "div") {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.listeners = new Map();
    this.isConnected = true;
    this.disabled = false;
    this.textContent = "";
    this.title = "";
  }

  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = [...nodes]; }
  setAttribute(name, value) { this[name] = value; }
  addEventListener(name, callback) { this.listeners.set(name, callback); }
  click() { return this.listeners.get("click")?.(); }
}

function setup(invoke) {
  const oldWindow = globalThis.window;
  const oldDocument = globalThis.document;
  // No confirm or alert: the dialog plugin makes them async and Settings may
  // not call them, so a call here throws.
  globalThis.window = { __TAURI_INTERNALS__: { invoke } };
  globalThis.document = { createElement: (tag) => new Element(tag) };
  const section = new Element();
  mountCharacterImports({ querySelector: () => section });
  const [, intro, controls, warning, status, list] = section.children[0].children;
  const [importButton, folderButton] = controls.children;
  return {
    intro, warning, importButton, folderButton, status, list,
    restore() {
      globalThis.window = oldWindow;
      globalThis.document = oldDocument;
    },
  };
}

async function settle() {
  await Promise.resolve();
  await Promise.resolve();
}

test("import and remove run straight through native commands and say to restart", async () => {
  const calls = [];
  let characters = [];
  const ui = setup(async (command, args) => {
    calls.push([command, args]);
    if (command === "character_import_list") return characters;
    if (command === "character_import_zip") {
      characters = [{ id: "dosidicus-squid", name: "Dosidicus Squid", removable: true }];
      return "Dosidicus Squid";
    }
    if (command === "character_import_remove") {
      assert.deepEqual(args, { id: "dosidicus-squid" });
      characters = [];
      return;
    }
    throw new Error(`unexpected command ${command}`);
  });
  try {
    await settle();
    assert.match(ui.warning.textContent, /Only import ones from people you trust/);
    assert.equal(ui.intro.textContent, "Add a Character from a ZIP file or folder.");
    assert.equal(ui.importButton.textContent, "Import from zip");
    assert.equal(ui.folderButton.textContent, "Import from folder");
    assert.equal(ui.warning.className, "set-notice set-caution");
    assert.equal(ui.list.children[0].textContent, "No imported Characters yet.");

    await ui.importButton.click();
    assert.equal(ui.status.textContent, "Dosidicus Squid installed. Restart Fidget to use it.");
    assert.equal(ui.list.children.length, 1);
    const [label, removeButton] = ui.list.children[0].children;
    assert.equal(label.textContent, "Dosidicus Squid");
    assert.equal(removeButton.disabled, false);

    await removeButton.click();
    assert.equal(ui.status.textContent, "Fidget's copy of Dosidicus Squid removed. Restart Fidget to update the Character menu.");
    assert.equal(ui.list.children[0].textContent, "No imported Characters yet.");
    assert.deepEqual(calls.map(([name]) => name), [
      "character_import_list",
      "character_import_zip",
      "character_import_list",
      "character_import_remove",
      "character_import_list",
    ]);
  } finally {
    ui.restore();
  }
});

test("import from folder runs its own native command and says to restart", async () => {
  const calls = [];
  let characters = [];
  const ui = setup(async (command) => {
    calls.push(command);
    if (command === "character_import_list") return characters;
    if (command === "character_import_folder") {
      characters = [{ id: "dosidicus-squid", name: "Dosidicus Squid", removable: true }];
      return "Dosidicus Squid";
    }
    throw new Error(`unexpected command ${command}`);
  });
  try {
    await settle();
    await ui.folderButton.click();
    assert.equal(ui.status.textContent, "Dosidicus Squid installed. Restart Fidget to use it.");
    assert.equal(ui.list.children[0].children[0].textContent, "Dosidicus Squid");
    assert.equal(ui.importButton.disabled, false);
    assert.equal(ui.folderButton.disabled, false);
    assert.deepEqual(calls, ["character_import_list", "character_import_folder", "character_import_list"]);
  } finally {
    ui.restore();
  }
});

test("selected or running Characters cannot be removed from Character Imports", async () => {
  const calls = [];
  const ui = setup(async (command) => {
    calls.push(command);
    if (command === "character_import_list") {
      return [{ id: "running", name: "Running Character", removable: false }];
    }
    throw new Error("remove must not be called");
  });
  try {
    await settle();
    const removeButton = ui.list.children[0].children[1];
    assert.equal(removeButton.disabled, true);
    assert.match(removeButton.title, /Selected or running/);
    assert.deepEqual(calls, ["character_import_list"]);
  } finally {
    ui.restore();
  }
});

test("invalid packages present the actual error and do not show a restart prompt", async () => {
  const ui = setup(async (command) => {
    if (command === "character_import_list") return [];
    throw new Error("Character package is invalid: missing frame idle-0.png");
  });
  try {
    await settle();
    await ui.importButton.click();
    assert.equal(ui.status.textContent, "Character Imports: Character package is invalid: missing frame idle-0.png");
    assert.doesNotMatch(ui.status.textContent, /Restart/);
    assert.equal(ui.importButton.disabled, false);
  } finally {
    ui.restore();
  }
});

test("Shell errors reach the status line labelled once", () => {
  // showError adds "Character Imports:", so the Shell's errors must not.
  const shell = readFileSync(new URL("../src-tauri/src/character_import.rs", import.meta.url), "utf8");
  assert.doesNotMatch(shell, /"Character Imports/);
});
