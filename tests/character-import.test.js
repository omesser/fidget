import assert from "node:assert/strict";
import { test } from "node:test";
import { mountCharacterManager } from "../src/character-manager.js";

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

function setup(invoke, { confirm = () => true } = {}) {
  const oldWindow = globalThis.window;
  const oldDocument = globalThis.document;
  const prompts = [];
  const alerts = [];
  globalThis.window = {
    __TAURI_INTERNALS__: { invoke },
    confirm: (message) => { prompts.push(message); return confirm(message); },
    alert: (message) => { alerts.push(message); },
  };
  globalThis.document = { createElement: (tag) => new Element(tag) };
  const section = new Element();
  mountCharacterManager({ querySelector: () => section });
  const [, intro, warning, importButton, status, list] = section.children[0].children;
  return {
    intro, warning, importButton, status, list, prompts, alerts,
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

test("initial listing, import, restart prompt, and remove use native commands", async () => {
  const calls = [];
  let characters = [];
  const ui = setup(async (command, args) => {
    calls.push([command, args]);
    if (command === "character_manager_list") return characters;
    if (command === "character_manager_import") {
      characters = [{ id: "dosidicus-squid", name: "Dosidicus Squid", removable: true }];
      return "Dosidicus Squid";
    }
    if (command === "character_manager_remove") {
      assert.deepEqual(args, { id: "dosidicus-squid" });
      characters = [];
      return;
    }
    throw new Error(`unexpected command ${command}`);
  });
  try {
    await settle();
    assert.match(ui.warning.textContent, /Only import ones from people you trust/);
    assert.equal(ui.list.children[0].textContent, "No imported Character ZIPs yet.");

    await ui.importButton.click();
    assert.match(ui.status.textContent, /Dosidicus Squid installed/);
    assert.equal(ui.status.textContent, "Dosidicus Squid installed.");
    assert.deepEqual(ui.alerts, ["Restart Fidget to use it."]);
    assert.match(ui.prompts[0], /personality the AI reads/);
    assert.equal(ui.list.children.length, 1);
    const [label, removeButton] = ui.list.children[0].children;
    assert.equal(label.textContent, "Dosidicus Squid");
    assert.equal(removeButton.disabled, false);

    await removeButton.click();
    assert.match(ui.prompts[1], /Remove Dosidicus Squid/);
    assert.match(ui.status.textContent, /removed/);
    assert.equal(ui.list.children[0].textContent, "No imported Character ZIPs yet.");
    assert.deepEqual(calls.map(([name]) => name), [
      "character_manager_list",
      "character_manager_import",
      "character_manager_list",
      "character_manager_remove",
      "character_manager_list",
    ]);
  } finally {
    ui.restore();
  }
});

test("selected or running Characters cannot be removed from the manager", async () => {
  const calls = [];
  const ui = setup(async (command) => {
    calls.push(command);
    if (command === "character_manager_list") {
      return [{ id: "running", name: "Running Character", removable: false }];
    }
    throw new Error("remove must not be called");
  });
  try {
    await settle();
    const removeButton = ui.list.children[0].children[1];
    assert.equal(removeButton.disabled, true);
    assert.match(removeButton.title, /Selected or running/);
    assert.deepEqual(calls, ["character_manager_list"]);
  } finally {
    ui.restore();
  }
});

test("the user can cancel the trust warning without opening a file picker", async () => {
  const calls = [];
  const ui = setup(async (command) => {
    calls.push(command);
    return [];
  }, { confirm: () => false });
  try {
    await settle();
    await ui.importButton.click();
    assert.deepEqual(calls, ["character_manager_list"]);
    assert.equal(ui.alerts.length, 0);
  } finally {
    ui.restore();
  }
});

test("invalid packages present the actual error and do not show a restart prompt", async () => {
  const ui = setup(async (command) => {
    if (command === "character_manager_list") return [];
    throw new Error("Character package is invalid: missing frame idle-0.png");
  });
  try {
    await settle();
    await ui.importButton.click();
    assert.match(ui.status.textContent, /missing frame idle-0.png/);
    assert.equal(ui.alerts.length, 0);
    assert.equal(ui.importButton.disabled, false);
  } finally {
    ui.restore();
  }
});
