// A live accessibility dump asks which rows are the user's to edit for the AI
// source in force. `controls()` is the same flat list in render order, so the
// same questions are asked of the fixtures.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { controls, render } from "../src/settings.js";

// Two files per state. The description keeps the `settings-snapshot-` name and
// the bare `FormDescription` shape that the Rust side's own fixture has, so
// that one drops in here unchanged; the values ride alongside it.
function snapshot(name) {
  const read = (kind) =>
    JSON.parse(readFileSync(new URL(`./fixtures/settings-${kind}-${name}.json`, import.meta.url), "utf8"));
  return { form: read("snapshot"), values: read("values") };
}

const MODEL_API = snapshot("modelApi");
const HARNESS = snapshot("harnessDriving");
const STATES = [
  ["Model API drives", MODEL_API],
  ["a Harness drives", HARNESS],
];

const TABS = ["Presence", "Character", "AI", "Chat", "Privacy", "Development"];

function tab(state, title) {
  const found = state.form.tabs.find((candidate) => candidate.title === title);
  assert.ok(found, `the snapshot carries no ${title} tab`);
  return controls(found, state.values);
}

const headings = (list) => list.filter((control) => control.role === "heading").map((control) => control.label);
const byId = (list, id) => list.find((control) => control.id === id);
const byLabel = (list, label) =>
  list.find((control) => control.role !== "heading" && control.label === label);

// The script's four section names. It greps for exactly these, so the BYO
// section between "AI source" and "Model / API" is invisible to it, and the
// order it reports is the order of the four that are left.
const AI_SECTIONS = ["AI", "AI source", "Model / API", "Last user turn"];

test("AI tab section order is AI, AI source, Model / API, Last user turn", () => {
  for (const [when, state] of STATES) {
    const order = headings(tab(state, "AI")).filter((heading) => AI_SECTIONS.includes(heading));
    assert.deepEqual(order, AI_SECTIONS, when);
  }
});

// By id: the Harness section has a Model row of its own (#1427).
const HTTP_ROWS = ["director_base_url", "director_model", "director_api_key", "clear_key"];

test("the HTTP rows are the user's when Model API drives", () => {
  const list = tab(MODEL_API, "AI");
  for (const id of HTTP_ROWS) {
    assert.equal(byId(list, id).frozen, false, id);
  }
  assert.equal(byId(list, "harness").value, "Model API");
});

test("the HTTP rows freeze when a Harness drives", () => {
  const list = tab(HARNESS, "AI");
  for (const id of HTTP_ROWS) {
    assert.equal(byId(list, id).frozen, true, `${id} must be frozen (#452)`);
  }
  assert.equal(byId(list, "harness").value, "Harness · claude");
  assert.equal(byId(list, "director_apply").frozen, false, "Apply stays live, or the source cannot be changed back");
});

// #1427: the AI source picker decides which source section is the user's,
// and it decides as soon as it changes, before Apply saves the pick.
const HARNESS_ROWS = ["harness_model", "pi_project_mcp", "harness_command"];
const MODEL_API_ROWS = [
  "director_base_url_pick",
  "director_base_url",
  "director_model",
  "director_api_key",
  "clear_key",
];

test("the Harness rows are off while the picker says Model API", () => {
  const list = tab(MODEL_API, "AI");
  for (const id of HARNESS_ROWS) {
    assert.equal(byId(list, id).frozen, true, id);
  }
  assert.equal(byId(list, "harness").frozen, false, "the picker is the way to a Harness");
});

test("a picked Harness takes the whole Model / API section off before Apply", () => {
  const picked = { ...MODEL_API, values: { ...MODEL_API.values, harness: "Harness · grok" } };
  const list = tab(picked, "AI");
  for (const id of MODEL_API_ROWS) {
    assert.equal(byId(list, id).frozen, true, id);
  }
  for (const id of HARNESS_ROWS) {
    assert.equal(byId(list, id).frozen, false, id);
  }
  assert.equal(byId(list, "director_apply").frozen, false, "Apply saves the pick");
  assert.equal(byId(list, "director_cancel").frozen, false, "Cancel takes it back");
});

test("the Harness Model row is the user's while that Harness drives", () => {
  const model = byId(tab(HARNESS, "AI"), "harness_model");
  assert.deepEqual(model, {
    role: "textfield",
    id: "harness_model",
    label: "Model",
    value: "gpt-4o-mini",
    frozen: false,
  });
});

// The picker carries no label of its own, so it is addressed the way the
// script addresses it: the popup nearest above the Base URL row in render
// order. A picker live over a frozen field is the bug this catches.
function endpointPicker(list) {
  const at = list.findIndex((control) => control.label === "Base URL");
  assert.notEqual(at, -1, "the AI tab has no Base URL row");
  const above = list.slice(0, at).filter((control) => control.role === "popup");
  return above[above.length - 1];
}

test("the endpoint picker sits over Base URL and freezes with it", () => {
  assert.equal(endpointPicker(tab(MODEL_API, "AI")).id, "director_base_url_pick");
  assert.equal(endpointPicker(tab(MODEL_API, "AI")).frozen, false);
  assert.equal(endpointPicker(tab(HARNESS, "AI")).frozen, true);
});

// `has_line`, against the labels and values an accessibility dump would show.
const spoken = (list) => list.map((control) => `${control.label ?? ""}\n${control.value ?? ""}`).join("\n");

test("the state line says the HTTP endpoint is the AI brain", () => {
  assert.equal(spoken(tab(MODEL_API, "AI")).includes("The HTTP endpoint below is the AI brain."), true);
});

test('"the Director\'s mind" stays out of the state line', () => {
  for (const [when, state] of STATES) {
    assert.equal(spoken(tab(state, "AI")).includes("is the Director's mind"), false, when);
  }
});

const HEADINGS = {
  Presence: ["Do Not Disturb", "Hide", "Launch"],
  Character: ["Character", "Instances"],
  AI: ["AI", "AI source", "Harness", "Model / API", "BYO - Point existing Harness at Fidget", "Last user turn"],
  Privacy: ["What the fidget can see", "Excluded applications", "Memory File"],
  Development: ["Traces", "Blank AI", "HTTP limits", "Harness attachment"],
  Chat: ["Appearance"],
};

test("every tab draws its sections, whichever source drives", () => {
  for (const [when, state] of STATES) {
    for (const title of TABS) {
      assert.deepEqual(headings(tab(state, title)), HEADINGS[title], `${title} when ${when}`);
    }
  }
});

test("no row in either snapshot is one this renderer has no case for", () => {
  const unrendered = [];
  for (const [when, state] of STATES) {
    for (const title of TABS) {
      for (const control of tab(state, title)) {
        if (control.role === "unknown") unrendered.push(`${control.label} in ${title} when ${when}`);
      }
    }
  }
  assert.deepEqual(unrendered, [], "a FormRow variant reached the page with no case in controls()");
});

// Labels belong to the form and values to the file, so a change of AI source
// moves `frozen` and the values and nothing else. A label that moved with the
// source would be a second description of the form, drifting from the first.
const AI_ROWS = [
  ["heading", null, "AI"],
  ["checkbox", "director", "AI on"],
  ["checkbox", "proactive", "Proactive model calls"],
  ["textfield", "director_wake_secs", "First wake, in seconds"],
  ["button", "new_session", "Start new session"],
  ["heading", null, "AI source"],
  ["popup", "harness", "AI source"],
  ["statictext", "harness_state", null],
  ["heading", null, "Harness"],
  ["textfield", "harness_model", "Model"],
  ["checkbox", "pi_project_mcp", "Write .mcp.json in the working directory"],
  ["textfield", "harness_command", "Custom command line"],
  ["heading", null, "Model / API"],
  ["popup", "director_base_url_pick", null],
  ["textfield", "director_base_url", "Base URL"],
  ["textfield", "director_model", "Model"],
  ["securefield", "director_api_key", "API key"],
  ["button", "clear_key", "Clear key"],
  ["button", "director_apply", "Apply"],
  ["button", "director_cancel", "Cancel"],
  ["heading", null, "BYO - Point existing Harness at Fidget"],
  ["popup", "byo_harness", "Harness"],
  ["statictext", "byo_snippet", null],
  ["button", "byo_copy", "Copy"],
  ["statictext", "byo_token", null],
  ["button", "byo_copy_token", "Copy"],
  ["statictext", "byo_steps", null],
  ["heading", null, "Last user turn"],
  ["statictext", "payload", null],
];

test("labels are the same under both sources; only frozen and the values move", () => {
  const shape = (list) => list.map((control) => [control.role, control.id, control.label]);

  assert.deepEqual(shape(tab(MODEL_API, "AI")), AI_ROWS);
  for (const title of TABS) {
    assert.deepEqual(shape(tab(HARNESS, title)), shape(tab(MODEL_API, title)), title);
  }
});

test("Checkbox, TextField, SecureField, Popup and InspectBlock keep their kind", () => {
  const list = tab(MODEL_API, "AI");

  assert.deepEqual(byId(list, "director"), {
    role: "checkbox",
    id: "director",
    label: "AI on",
    value: true,
    frozen: false,
  });
  assert.deepEqual(byId(list, "director_base_url"), {
    role: "textfield",
    id: "director_base_url",
    label: "Base URL",
    value: "https://api.openai.com",
    frozen: false,
  });
  assert.deepEqual(byId(list, "director_api_key"), {
    role: "securefield",
    id: "director_api_key",
    label: "API key",
    value: "",
    frozen: false,
  });
  assert.deepEqual(byId(list, "harness"), {
    role: "popup",
    id: "harness",
    label: "AI source",
    value: "Model API",
    frozen: false,
  });
  assert.deepEqual(byId(list, "harness_state"), {
    role: "statictext",
    id: "harness_state",
    label: null,
    value: "Not attached. The HTTP endpoint below is the AI brain.",
    frozen: true,
  });
});

test("a List is one row and one dismiss button per Instance, labelled Name (character)", () => {
  const instances = tab(MODEL_API, "Character").filter((control) => control.id === "instances");

  assert.deepEqual(instances, [
    { role: "statictext", id: "instances", label: "BMO (bmo)", value: "BMO (bmo)", frozen: false },
    { role: "button", id: "instances", label: "Dismiss", value: "BMO (bmo)", frozen: false },
    { role: "statictext", id: "instances", label: "Boo (ghost)", value: "Boo (ghost)", frozen: false },
    { role: "button", id: "instances", label: "Dismiss", value: "Boo (ghost)", frozen: false },
  ]);
});

test("a Multiline keeps its lines and is live while the form says editable", () => {
  assert.deepEqual(byId(tab(MODEL_API, "Privacy"), "excluded"), {
    role: "textarea",
    id: "excluded",
    label: null,
    value: "Keynote\nzoom.us",
    frozen: false,
  });
});

test("an InspectPath is a path and nothing else", () => {
  assert.deepEqual(byId(tab(MODEL_API, "Privacy"), "memory_path"), {
    role: "statictext",
    id: "memory_path",
    label: null,
    value: "/Users/fixture/Library/Application Support/fidget/memory",
    frozen: true,
  });
});

// The Character popup starts on the Character in force, which is what New
// spawns under until the user picks another (#875).
test("a Composite spreads into one control per member, its text field included", () => {
  const spawn = tab(MODEL_API, "Character").filter((control) =>
    ["new_name", "new_character", "spawn"].includes(control.id),
  );

  assert.deepEqual(spawn, [
    { role: "textfield", id: "new_name", label: null, value: "", frozen: false },
    { role: "popup", id: "new_character", label: null, value: "bmo", frozen: false },
    { role: "button", id: "spawn", label: "New", value: null, frozen: false },
  ]);
});

// One literal per shape the row-id reconciliation has to get right, because
// a row the view has no value for draws blank however the file is set. #875.
test("a row whose id is not its field name still draws the value the snapshot carries", () => {
  const presence = tab(MODEL_API, "Presence");
  assert.equal(byId(presence, "hotkey").value, "Control-Option-Command-B");
  assert.equal(byId(presence, "launch").value, false);
  assert.equal(byId(presence, "capturable").value, true);

  const ai = tab(MODEL_API, "AI");
  assert.equal(byId(ai, "director").value, true);
  assert.equal(byId(ai, "proactive").value, true);
  assert.equal(byId(ai, "director_wake_secs").value, "180");
  assert.equal(byId(ai, "payload").value, "Ambient wake for bmo, 3 behaviors, 412 tokens.");

  const privacy = tab(MODEL_API, "Privacy");
  assert.equal(byId(privacy, "excluded").value, "Keynote\nzoom.us");
  assert.equal(byId(privacy, "consent_accessibility").value, true);

  assert.equal(byId(tab(MODEL_API, "Development"), "director_timeout_secs").value, "30");
});

// #888 named the macOS row for what the character reads rather than for the grant.
// The id it draws under is the one the value map is keyed by, and deriving that
// id from the label instead left the lookup empty, so the checkbox drew
// unchecked whatever the grant said. #975 renamed the label a second time,
// which is the case this test exists for.
test("the Privacy consent row keeps the id its value is keyed by, whatever it is named", () => {
  const label = "Window and application names";
  for (const [state, snap] of STATES) {
    const row = byLabel(tab(snap, "Privacy"), label);
    assert.ok(row, `${state}: the Privacy tab draws no "${label}" row`);
    assert.equal(row.id, "consent_screen_recording", state);
    assert.ok(row.id in snap.values, `${state}: no value is keyed by ${row.id}`);
  }
});

// `render()` needs a DOM. These nodes carry what the assertions read and
// nothing else, and the click handler is kept so a press can be driven.
function stubDocument() {
  globalThis.document = {
    getElementById: () => null,
    createElement(tag) {
      const node = {
        tagName: tag,
        id: "",
        textContent: "",
        value: "",
        dataset: {},
        attributes: {},
        children: [],
        style: {},
        handlers: {},
        setAttribute(name, value) {
          node.attributes[name] = value;
        },
        append(...nodes) {
          node.children.push(...nodes);
          // A <select> reads its value off the option marked selected, which
          // is how a press learns the Character the popup shows.
          for (const child of nodes) {
            if (tag === "select" && child.attributes.selected !== undefined) {
              node.value = child.attributes.value;
            }
          }
        },
        addEventListener(name, handler) {
          node.handlers[name] = handler;
        },
        closest: () => null,
        getRootNode: () => node,
        focus() {
          document.activeElement = node;
        },
      };
      return node;
    },
    activeElement: null,
  };
}

function drawn(title, emit) {
  const root = {
    children: [],
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children.push(...nodes);
    },
  };
  stubDocument();
  render(root, MODEL_API.form.tabs.find((candidate) => candidate.title === title), MODEL_API.values, emit);
  const walk = (node) => [node, ...(node.children ?? []).flatMap(walk)];
  return root.children.flatMap(walk);
}

// Both Character <select>s offer every installed package, with the one in
// force selected. A list of one is the bug: New could only spawn the
// character already worn (#921).
test("the Character popups offer every installed package, the worn one selected", () => {
  const selects = drawn("Character").filter((node) => node.tagName === "select");
  const offered = Object.fromEntries(
    selects.map((select) => [select.dataset.id, select.children.map((option) => option.attributes.value)]),
  );
  assert.deepEqual(offered, {
    character: ["bmo", "ghost"],
    new_character: ["bmo", "ghost"],
  });
  for (const select of selects) {
    assert.equal(select.value, "bmo", select.dataset.id);
  }
});

test("the UI Theme and Light/Dark Mode popups live on Chat", () => {
  const selects = drawn("Chat").filter((node) => node.tagName === "select");
  const offered = Object.fromEntries(
    selects.map((select) => [select.dataset.id, select.children.map((option) => option.attributes.value)]),
  );
  assert.deepEqual(offered, {
    chat_ui: ["Minimal", "Terminal", "Glass"],
    chat_appearance: ["System", "Light", "Dark"],
  });
  assert.equal(byId(tab(MODEL_API, "Chat"), "chat_ui").label, "UI Theme");
  assert.equal(byId(tab(MODEL_API, "Chat"), "chat_appearance").label, "Light/Dark Mode");
  assert.equal(selects.find((select) => select.dataset.id === "chat_ui").value, "Minimal");
  assert.equal(selects.find((select) => select.dataset.id === "chat_appearance").value, "System");
  for (const title of TABS) {
    if (title === "Chat") continue;
    assert.equal(drawn(title).some((node) => node.dataset.id === "chat_ui"), false, title);
    assert.equal(drawn(title).some((node) => node.dataset.id === "chat_appearance"), false, title);
  }
});

// A redraw is render() again, and replaceChildren() takes the focused control
// with it. The stub keeps activeElement and answers querySelectorAll by tag,
// which is all the restore reads. #937.
function focusRoot() {
  const walk = (node) => [node, ...(node.children ?? []).flatMap(walk)];
  return {
    children: [],
    replaceChildren() {
      this.children = [];
      document.activeElement = null;
    },
    append(...nodes) {
      this.children.push(...nodes);
    },
    querySelectorAll(selector) {
      const tags = selector.split(",").map((tag) => tag.trim());
      return this.children.flatMap(walk).filter((node) => tags.includes(node.tagName));
    },
  };
}

// The row a control was drawn for, read off the wrapper's data-row.
function rowOf(root, control) {
  const walk = (node) => [node, ...(node.children ?? []).flatMap(walk)];
  const owner = root.children
    .flatMap(walk)
    .find((node) => node.attributes["data-row"] !== undefined && walk(node).includes(control));
  return owner?.attributes["data-row"];
}

test("a redraw hands focus back to the control that held it", () => {
  const cases = [
    ["Character", (node) => node.tagName === "select", "character"],
    ["Presence", (node) => node.attributes.type === "checkbox", "dnd"],
    ["Presence", (node) => node.tagName === "summary", "dnd"],
  ];
  for (const [title, pick, row] of cases) {
    stubDocument();
    const root = focusRoot();
    const tab = MODEL_API.form.tabs.find((candidate) => candidate.title === title);
    render(root, tab, MODEL_API.values);
    const before = root.querySelectorAll("input, textarea, select, button, summary, pre, label").find(pick);
    assert.equal(rowOf(root, before), row);
    before.focus();

    render(root, tab, MODEL_API.values);

    const after = document.activeElement;
    assert.notEqual(after, before, `${row}: the redraw rebuilt the ${before.tagName}`);
    assert.equal(after?.tagName, before.tagName, `${row}: focus is on a ${before.tagName}`);
    assert.equal(rowOf(root, after), row, `${row}: focus is on the same row's control`);
  }
});

test("a secure row takes its placeholder from the key status beside it", () => {
  const key = drawn("AI").find((node) => node.attributes.type === "password");
  assert.equal(key.attributes.placeholder, "Set: \u20264f2a");
});

// `SettingsSession::spawn` takes a Character and a name, and `AiDraft`
// carries neither. #875.
test("New carries the name typed beside it and the Character it shows", () => {
  const emitted = [];
  const nodes = drawn("Character", (payload) => emitted.push(payload));
  const name = nodes.find((node) => node.dataset.id === "new_name");
  name.value = "Nim";
  nodes.find((node) => node.textContent === "New").handlers.click();

  assert.deepEqual(emitted, [
    { press: "spawn", fields: { new_name: "Nim", new_character: "bmo" } },
  ]);
});

// The line is ambiguous between two characters of one name and Character, and
// `SettingsSession::dismiss` takes an id. #875.
test("Dismiss names the Instance by id, not by the line it draws", () => {
  const emitted = [];
  const buttons = drawn("Character", (payload) => emitted.push(payload)).filter(
    (node) => node.tagName === "button" && node.textContent === "Dismiss",
  );

  assert.equal(buttons.length, 2);
  buttons[1].handlers.click();
  assert.deepEqual(emitted, [{ dismiss: "instances", value: "ghost-1" }]);
});

test("handleEvent returns false for nothing response", async () => {
  const { handleEvent } = await import("../src/settings.js");

  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async () => ({ action: "nothing" }),
    },
  };

  const result = await handleEvent({ press: "director_apply" });
  assert.equal(result, false);

  delete globalThis.window;
});

test("handleEvent returns true for refresh response", async () => {
  const { handleEvent } = await import("../src/settings.js");

  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async () => ({ action: "refresh" }),
    },
  };

  const result = await handleEvent({ pick: "character", value: "bmo" });
  assert.equal(result, true);

  delete globalThis.window;
});

test("handleEvent returns fill object for fill response", async () => {
  const { handleEvent } = await import("../src/settings.js");

  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async () => ({
        action: "fill",
        id: "director_base_url",
        value: "https://api.openai.com",
      }),
    },
  };

  const result = await handleEvent({
    pick: "director_base_url_pick",
    value: "OpenAI",
    fills: { row: "director_base_url" },
  });

  assert.deepEqual(result, {
    fill: {
      id: "director_base_url",
      value: "https://api.openai.com",
    },
  });

  delete globalThis.window;
});

test("handleEvent returns reset object for reset response", async () => {
  const { handleEvent } = await import("../src/settings.js");

  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async () => ({ action: "reset" }),
    },
  };

  const result = await handleEvent({ press: "director_cancel" });
  assert.deepEqual(result, { reset: true });

  delete globalThis.window;
});

test("handleEvent returns clearKey object for clear_key response", async () => {
  const { handleEvent } = await import("../src/settings.js");

  globalThis.window = {
    __TAURI_INTERNALS__: {
      invoke: async () => ({ action: "clear_key" }),
    },
  };

  const result = await handleEvent({ press: "director_api_key_clear" });
  assert.deepEqual(result, { clearKey: true });

  delete globalThis.window;
});

test("director_actions renders to footer when one exists", async () => {
  const { render } = await import("../src/settings.js");

  const mockFooter = {
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children = this.children || [];
      this.children.push(...nodes);
    },
    children: [],
    style: {},
  };

  const mockRoot = {
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children = this.children || [];
      this.children.push(...nodes);
    },
    children: [],
  };

  globalThis.document = {
    getElementById(id) {
      return id === "set-footer" ? mockFooter : null;
    },
    createElement(tag) {
      const node = {
        tagName: tag,
        id: "",
        textContent: "",
        dataset: {},
        attributes: {},
        children: [],
        style: {},
        setAttribute(name, value) {
          node.attributes[name] = value;
          if (name.startsWith("data-")) {
            const key = name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
            node.dataset[key] = value;
          }
        },
        append(...nodes) {
          node.children.push(...nodes);
        },
        addEventListener() {},
        closest() {
          return null;
        },
        getRootNode() {
          return node;
        },
      };
      return node;
    },
  };

  const tab = MODEL_API.form.tabs.find((t) => t.title === "AI");
  render(mockRoot, tab, MODEL_API.values);

  const footerHasDirectorActions = mockFooter.children.some(
    (node) => node.dataset && node.dataset.row === "director_actions",
  );

  assert.ok(
    mockFooter.children.length > 0,
    `footer should have children, got ${mockFooter.children.length} children`,
  );
  assert.ok(footerHasDirectorActions, "director_actions should be in footer when footer element exists");

  delete globalThis.document;
});

test("non-AI tabs hide footer when it has no content (Bug 1)", async () => {
  const { render } = await import("../src/settings.js");

  const mockFooter = {
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children = this.children || [];
      this.children.push(...nodes);
    },
    children: [],
    style: {},
  };

  const mockRoot = {
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children = this.children || [];
      this.children.push(...nodes);
    },
    children: [],
  };

  globalThis.document = {
    getElementById(id) {
      return id === "set-footer" ? mockFooter : null;
    },
    createElement(tag) {
      const node = {
        tagName: tag,
        id: "",
        textContent: "",
        dataset: {},
        attributes: {},
        children: [],
        style: {},
        setAttribute(name, value) {
          node.attributes[name] = value;
          if (name.startsWith("data-")) {
            const key = name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
            node.dataset[key] = value;
          }
        },
        append(...nodes) {
          node.children.push(...nodes);
        },
        addEventListener() {},
        closest() {
          return null;
        },
        getRootNode() {
          return node;
        },
      };
      return node;
    },
  };

  const NON_AI_TABS = ["Presence", "Character", "Chat", "Privacy", "Development"];

  for (const tabTitle of NON_AI_TABS) {
    mockFooter.children = [];
    mockFooter.style = {};
    const tab = MODEL_API.form.tabs.find((t) => t.title === tabTitle);
    render(mockRoot, tab, MODEL_API.values);

    assert.equal(
      mockFooter.children.length,
      0,
      `${tabTitle} tab footer should have no children`,
    );
    assert.equal(
      mockFooter.style.display,
      "none",
      `${tabTitle} tab footer should be hidden (display: none)`,
    );
  }

  delete globalThis.document;
});

test("AI tab shows footer with Apply and Cancel buttons", async () => {
  const { render } = await import("../src/settings.js");

  const mockFooter = {
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children = this.children || [];
      this.children.push(...nodes);
    },
    children: [],
    style: {},
  };

  const mockRoot = {
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children = this.children || [];
      this.children.push(...nodes);
    },
    children: [],
  };

  globalThis.document = {
    getElementById(id) {
      return id === "set-footer" ? mockFooter : null;
    },
    createElement(tag) {
      const node = {
        tagName: tag,
        id: "",
        textContent: "",
        dataset: {},
        attributes: {},
        children: [],
        style: {},
        setAttribute(name, value) {
          node.attributes[name] = value;
          if (name.startsWith("data-")) {
            const key = name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
            node.dataset[key] = value;
          }
        },
        append(...nodes) {
          node.children.push(...nodes);
        },
        addEventListener() {},
        closest() {
          return null;
        },
        getRootNode() {
          return node;
        },
      };
      return node;
    },
  };

  const tab = MODEL_API.form.tabs.find((t) => t.title === "AI");
  render(mockRoot, tab, MODEL_API.values);

  assert.ok(mockFooter.children.length > 0, "AI tab footer should have children");
  assert.notEqual(mockFooter.style.display, "none", "AI tab footer should be visible");

  delete globalThis.document;
});

test("showError clears and hides footer to prevent empty-payload wipes (Bug 2)", async () => {
  const { showError } = await import("../src/settings.js");

  const mockPanel = {
    replaceChildren() {
      this.children = [];
    },
    appendChild(node) {
      this.children = this.children || [];
      this.children.push(node);
    },
    children: [],
  };

  const mockFooter = {
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children = this.children || [];
      this.children.push(...nodes);
    },
    children: [{ tagName: "button", textContent: "Apply" }],
    style: {},
  };

  const mockDocument = {
    createElement(tag) {
      return {
        tagName: tag,
        className: "",
        style: {},
        textContent: "",
        appendChild() {},
        addEventListener() {},
      };
    },
    querySelector(selector) {
      if (selector === '[role="tabpanel"]') return mockPanel;
      return null;
    },
    getElementById(id) {
      return id === "set-footer" ? mockFooter : null;
    },
  };

  globalThis.document = mockDocument;

  mockFooter.children = [{ tagName: "button", textContent: "Apply" }];
  mockFooter.style = {};

  showError("Could not save changes", () => {});

  assert.equal(mockFooter.children.length, 0, "footer children should be cleared after showError");
  assert.equal(mockFooter.style.display, "none", "footer should be hidden after showError");

  delete globalThis.document;
});

test("a redraw preserves open disclosure state across rebuild", () => {
  stubDocument();
  const walk = (node) => [node, ...(node.children ?? []).flatMap(walk)];
  const root = {
    children: [],
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children.push(...nodes);
    },
    querySelectorAll(selector) {
      if (selector === "details") {
        return this.children.flatMap(walk).filter((node) => node.tagName === "details");
      }
      return [];
    },
  };

  const tab = MODEL_API.form.tabs.find((t) => t.title === "Presence");
  render(root, tab, MODEL_API.values);

  const firstDetails = root.querySelectorAll("details");
  assert.ok(firstDetails.length > 0, "the Presence tab has disclosure rows");

  firstDetails[0].open = true;
  if (firstDetails.length > 1) firstDetails[1].open = true;

  render(root, tab, MODEL_API.values);

  const secondDetails = root.querySelectorAll("details");
  assert.equal(secondDetails[0].open, true, "first disclosure stays open across redraw");
  if (secondDetails.length > 1) {
    assert.equal(secondDetails[1].open, true, "second disclosure stays open across redraw");
  }
});
