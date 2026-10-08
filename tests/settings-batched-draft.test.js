// A tab switch redraws from the snapshot. The draft preserves staged edits
// across those redraws until Apply or Cancel.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { foldDraft, pressFeedback, processResponse, pruneDraft, render } from "../src/settings.js";

function snapshot(name) {
  const read = (kind) =>
    JSON.parse(readFileSync(new URL(`./fixtures/settings-${kind}-${name}.json`, import.meta.url), "utf8"));
  return { form: read("snapshot"), values: read("values") };
}

const MODEL_API = snapshot("modelApi");
const AI = MODEL_API.form.tabs.find((tab) => tab.title === "AI");

// Apply reads the drawn controls back through querySelector, so the stub
// answers the shapes the page asks: a control by id, a row by data-row with an
// optional descendant tag, and a list of tags.
function query(node, selector) {
  const below = (n) => (n.children ?? []).flatMap((child) => [child, ...below(child)]);
  const id = selector.match(/^#(\S+)$/);
  if (id) return below(node).find((n) => n.id === id[1]) ?? null;
  const row = selector.match(/^\[data-row="([^"]+)"\](?: (\w+))?$/);
  if (row) {
    const found = below(node).find((n) => n.attributes?.["data-row"] === row[1]);
    return row[2] ? (found && query(found, row[2])) ?? null : found ?? null;
  }
  const tags = selector.split(/,\s*/);
  return below(node).find((n) => tags.includes(n.tagName)) ?? null;
}

function stubDocument(root) {
  globalThis.document = {
    getElementById: () => null,
    createElement(tag) {
      const node = {
        tagName: tag,
        id: "",
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
        getRootNode: () => root,
        querySelector: (selector) => query(node, selector),
        focus() {},
      };
      return node;
    },
    activeElement: null,
  };
}

function draw(values, emit, stage, tab = AI) {
  const root = {
    children: [],
    replaceChildren() {
      this.children = [];
    },
    append(...nodes) {
      this.children.push(...nodes);
    },
    querySelector(selector) {
      return query(this, selector);
    },
  };
  stubDocument(root);
  render(root, tab, values, emit, stage);
  const walk = (node) => [node, ...(node.children ?? []).flatMap(walk)];
  const all = root.children.flatMap(walk);
  delete globalThis.document;
  const find = (id) =>
    all.find((node) => node.id === `set-f-${id}` || node.dataset.id === id || node.attributes?.["data-id"] === id);
  find.row = (id) => all.find((node) => node.attributes?.["data-row"] === id);
  find.notes = () => all.filter((node) => node.attributes?.class === "set-status").map((node) => node.textContent);
  return find;
}

test("a batched row reports each edit through stage and writes nothing on blur", () => {
  const emitted = [];
  const staged = [];
  const control = draw(
    MODEL_API.values,
    (payload) => emitted.push(payload),
    (id, value) => staged.push([id, value]),
  );

  const model = control("director_model");
  model.value = "gpt-5";
  model.handlers.input?.();
  model.handlers.blur?.();

  const key = control("director_api_key");
  key.value = "sk-draft";
  key.handlers.input?.();
  key.handlers.blur?.();

  const harness = control("harness");
  harness.value = "Claude Code";
  harness.handlers.change?.();

  assert.deepEqual(staged, [
    ["harness_model", "gpt-5"],
    ["director_model", "gpt-5"],
    ["director_api_key", "sk-draft"],
    ["harness", "Claude Code"],
  ]);
  assert.deepEqual(emitted, [], "a batched row never writes on blur (#663)");
});

test("AI switches and wake interval wait for Apply and can be discarded", () => {
  const emitted = [];
  let draft = {};
  const control = draw(MODEL_API.values, (payload) => emitted.push(payload), (id, value) => {
    draft[id] = value;
  });
  const director = control.row("director").children[0].children[0];
  director.checked = false;
  director.handlers.change();
  const proactive = control.row("proactive").children[0].children[0];
  proactive.checked = false;
  proactive.handlers.change();
  const piMcp = control.row("pi_project_mcp").children[0].children[0];
  piMcp.checked = false;
  piMcp.handlers.change();
  const wake = control("director_wake_secs");
  wake.value = "240";
  wake.handlers.input();
  wake.handlers.blur?.();

  assert.deepEqual(emitted, []);
  assert.deepEqual(draft, {
    director: false,
    proactive: false,
    pi_project_mcp: false,
    director_wake_secs: "240",
  });
  assert.equal(draw({ ...MODEL_API.values, ...draft }).row("director").children[0].children[0].checked, false);
  draft = foldDraft(draft, { reset: true });
  assert.equal(draw({ ...MODEL_API.values, ...draft }).row("director").children[0].children[0].checked, true);
  assert.equal(draw({ ...MODEL_API.values, ...draft })("director_wake_secs").value, "180");
});

test("Apply and Cancel report a change only when the draft differs from the store", () => {
  const values = MODEL_API.values;
  for (const press of ["director_apply", "director_cancel"]) {
    assert.equal(pressFeedback(press, {}, values), null, press);
    assert.equal(pressFeedback(press, { director: values.director }, values), null, `${press}, toggled back`);
  }
  const toggled = { director: !values.director };
  assert.equal(pressFeedback("director_apply", toggled, values), "Changes applied.");
  assert.equal(pressFeedback("director_cancel", toggled, values), "Changes discarded.");
});

test("a batched row draws the draft it is handed, the key field included", () => {
  const control = draw({ ...MODEL_API.values, director_model: "gpt-5", director_api_key: "sk-draft" });

  assert.equal(control("director_model").value, "gpt-5");
  assert.equal(control("director_api_key").value, "sk-draft");
});

test("registration Harness previews without saving and Cancel restores the picker", () => {
  const emitted = [];
  let draft = {};
  const control = draw(MODEL_API.values, (payload) => emitted.push(payload), (id, value) => {
    draft[id] = value;
  });
  const picker = control("byo_harness");
  picker.value = "hermes";
  picker.handlers.change();

  assert.deepEqual(emitted, [{ pick: "byo_harness", value: "hermes" }]);
  assert.deepEqual(draft, { byo_harness: "hermes" });

  const preview = processResponse({
    action: "preview_byo",
    snippet: "hermes mcp add fidget",
    steps: "Run the command.",
    token: "secret-token",
  });
  draft = foldDraft(draft, preview, emitted[0]);
  const shown = draw({ ...MODEL_API.values, ...draft });
  assert.equal(shown("byo_harness").value, "hermes");
  assert.equal(shown.row("byo_snippet").children[0].textContent, "hermes mcp add fidget");
  assert.equal(shown.row("byo_token").children[0].textContent, "secret-token");

  draft = foldDraft(draft, { reset: true });
  assert.equal(draw({ ...MODEL_API.values, ...draft })("byo_harness").value, "claude");
});

test("a registration preview that lands after Cancel is dropped", () => {
  const emitted = [];
  let draft = {};
  const control = draw(MODEL_API.values, (payload) => emitted.push(payload), (id, value) => {
    draft[id] = value;
  });
  const picker = control("byo_harness");
  picker.value = "hermes";
  picker.handlers.change();
  const [pick] = emitted;

  draft = foldDraft(draft, { reset: true });
  const late = processResponse({ action: "preview_byo", snippet: "hermes mcp add fidget", steps: "Run it.", token: "t" });
  draft = foldDraft(draft, late, pick);

  assert.deepEqual(draft, {});
  assert.deepEqual(emitted, [pick], "a pick writes nothing; only Apply saves");
  const shown = draw({ ...MODEL_API.values, ...draft });
  assert.equal(shown("byo_harness").value, MODEL_API.values.byo_harness);
  assert.equal(shown.row("byo_snippet").children[0].textContent, MODEL_API.values.byo_snippet);
});

test("an unbatched popup saves its pick at once and stages nothing", () => {
  const emitted = [];
  const staged = [];
  const chat = MODEL_API.form.tabs.find((tab) => tab.title === "Chat");
  const control = draw(MODEL_API.values, (payload) => emitted.push(payload), (id) => staged.push(id), chat);
  const select = control("chat_ui");
  const other = select.children.find((option) => option.attributes.value !== select.value);
  select.value = other.attributes.value;
  select.handlers.change();

  assert.deepEqual(emitted, [{ pick: "chat_ui", value: other.attributes.value }]);
  assert.deepEqual(staged, []);
});

// The regression in #995, as a tab switch does it: the page redraws the panel
// from `{ ...snapshot, ...draft }`, so a batched edit survives only if it was
// staged out of the widget and drawn back in.
test("the Pi file row stays hidden until the staged source is Pi", () => {
  const model = draw(MODEL_API.values, () => {}, () => {});
  assert.equal(model.row("pi_project_mcp").hidden, true);

  const pi = draw({ ...MODEL_API.values, harness: "Harness · pi" }, () => {}, () => {});
  const row = pi.row("pi_project_mcp");
  assert.equal(row.hidden, false);
  const status = row.children.find((child) => child.attributes?.class === "set-status");
  assert.match(status.attributes.text ?? status.textContent, /pi-mcp-adapter/);
  assert.match(status.attributes.text ?? status.textContent, /FIDGET_MCP_TOKEN/);
});

test("a staged Model survives the redraw a tab switch performs", () => {
  let draft = {};
  const first = draw(MODEL_API.values, () => {}, (id, value) => (draft = { ...draft, [id]: value }));
  const model = first("director_model");
  model.value = "gpt-5";
  model.handlers.input?.();

  const again = draw({ ...MODEL_API.values, ...draft });

  assert.deepEqual(draft, { harness_model: "gpt-5", director_model: "gpt-5" });
  assert.equal(again("director_model").value, "gpt-5");
  assert.equal(again("director_base_url").value, "https://api.openai.com", "an untouched row still draws the snapshot");
});

// #1427: one setting drawn twice. Only the section the picker names is live,
// so the other row has to show what the live one holds.
test("a Model typed under Harness stages and draws the Model / API row too", () => {
  let draft = {};
  const control = draw(
    { ...MODEL_API.values, harness: "Harness · grok" },
    () => {},
    (id, value) => (draft = { ...draft, [id]: value }),
  );
  const model = control("harness_model");
  model.value = "grok-4.6";
  model.handlers.input();

  assert.deepEqual(draft, { harness_model: "grok-4.6", director_model: "grok-4.6" });
  assert.equal(control("director_model").value, "grok-4.6");
});

test("the section the picker does not name says why it is off", () => {
  assert.deepEqual(draw(MODEL_API.values).notes().filter((note) => note.startsWith("Not in use")), [
    "Not in use: the AI source is Model API.",
  ]);
  assert.deepEqual(
    draw({ ...MODEL_API.values, harness: "Harness · grok" }).notes().filter((note) => note.startsWith("Not in use")),
    ["Not in use: the AI source is a Harness."],
  );
});

test("a Base URL shortcut fill is drawn after the redraw, and Cancel drops it", () => {
  const filled = foldDraft({}, { fill: { id: "director_base_url", value: "https://api.anthropic.com" } });
  const drawnFilled = draw({ ...MODEL_API.values, ...filled });
  assert.equal(drawnFilled("director_base_url").value, "https://api.anthropic.com");

  const cancelled = foldDraft(filled, { reset: true });
  assert.deepEqual(cancelled, {});
  assert.equal(draw({ ...MODEL_API.values, ...cancelled })("director_base_url").value, "https://api.openai.com");
});

test("Clear key blanks the key's draft entry and stages the delete", () => {
  const draft = { director_api_key: "sk-draft", director_model: "gpt-5" };
  assert.deepEqual(foldDraft(draft, { clearKey: true }), { director_model: "gpt-5", clear_key: true });
  assert.deepEqual(foldDraft(draft, true), draft, "a plain refresh keeps the draft");
  assert.deepEqual(foldDraft(draft, false), draft, "a no-op answer keeps the draft");
});

// What the page sends for Apply after Clear key. The Rust side parses the
// same file, so the two cannot drift apart unseen.
const APPLY_AFTER_CLEAR = JSON.parse(
  readFileSync(new URL("./fixtures/settings-apply-clear-key.json", import.meta.url), "utf8"),
);

function pressApply(draft) {
  const emitted = [];
  draw({ ...MODEL_API.values, ...draft }, (payload) => emitted.push(payload))("director_apply").handlers.click();
  return emitted[0];
}

test("Apply after Clear key tells Rust to delete the key", () => {
  const draft = foldDraft({}, processResponse({ action: "clear_key" }));
  assert.deepEqual(pressApply(draft), APPLY_AFTER_CLEAR);
  assert.equal(pressApply(draft).draft.clear_key, true);
});

test("Cancel after Clear key drops the staged delete", () => {
  const cleared = foldDraft({}, processResponse({ action: "clear_key" }));
  const cancelled = foldDraft(cleared, { reset: true });
  assert.equal(pressApply(cancelled).draft.clear_key, false);
});

test("a key typed after Clear key is sent to replace the stored one", () => {
  let draft = foldDraft({}, processResponse({ action: "clear_key" }));
  const key = draw({ ...MODEL_API.values, ...draft }, () => {}, (id, value) => (draft = { ...draft, [id]: value }))(
    "director_api_key",
  );
  key.value = "sk-new";
  key.handlers.input();

  const sent = pressApply(draft).draft;
  assert.equal(sent.director_api_key, "sk-new");
  assert.equal(sent.clear_key, true, "Rust lets the typed key beat the staged clear");
});

test("a fresh snapshot prunes the draft entries it already holds", () => {
  const draft = { director_model: "gpt-4o-mini", harness: "Claude Code" };
  assert.deepEqual(pruneDraft(draft, MODEL_API.values), { harness: "Claude Code" });
});

test("Apply sends every row the form marks batched, and only those", () => {
  const development = MODEL_API.form.tabs.find((tab) => tab.title === "Development");
  const timeout = development.sections.flatMap((s) => s.rows).find((row) => row.id === "director_timeout_secs");
  const withTimeout = (batched) => ({
    ...AI,
    sections: [...AI.sections, { heading: "Limits", rows: [{ ...timeout, batched }] }],
  });
  const apply = (tab) => {
    const emitted = [];
    draw({ ...MODEL_API.values, director_timeout_secs: "90" }, (payload) => emitted.push(payload), () => {}, tab)(
      "director_apply",
    ).handlers.click();
    return emitted[0].draft;
  };
  const drawn = {
    director: true,
    proactive: true,
    director_wake_secs: "180",
    harness: "Model API",
    pi_project_mcp: true,
    harness_command: "",
    harness_model: "gpt-4o-mini",
    byo_harness: "claude",
    director_base_url: "https://api.openai.com",
    director_model: "gpt-4o-mini",
    director_api_key: "",
    clear_key: false,
  };
  assert.deepEqual(apply(withTimeout(false)), drawn, "an unbatched row saves on blur instead");
  assert.deepEqual(apply(withTimeout(true)), { ...drawn, director_timeout_secs: "90" });
});

const DEVELOPMENT = MODEL_API.form.tabs.find((tab) => tab.title === "Development");

// What the page sends when Reasoning effort's picker lands on high. The Rust
// side parses the same file, so the two cannot drift apart unseen (#1426).
const PICK_HIGH_EFFORT = JSON.parse(
  readFileSync(new URL("./fixtures/settings-pick-reasoning-effort.json", import.meta.url), "utf8"),
);

function pick(tab, id, value) {
  const emitted = [];
  const picker = draw(MODEL_API.values, (payload) => emitted.push(payload), () => {}, tab)(id);
  picker.value = value;
  picker.handlers.change();
  return emitted;
}

test("a shortcut pick names the row it fills the way settings_event reads it", () => {
  assert.deepEqual(pick(DEVELOPMENT, "director_reasoning_effort_pick", "high"), [PICK_HIGH_EFFORT]);
  assert.deepEqual(pick(AI, "director_base_url_pick", "xAI (https://api.x.ai)"), [
    { pick: "director_base_url_pick", value: "xAI (https://api.x.ai)", fills: { row: "director_base_url" } },
  ]);
});

// The title, the picker, and the field, in the order a reader meets them.
function rowReading(row) {
  const walk = (node) => (node.children ?? []).flatMap((child) => [child, ...walk(child)]);
  return walk(row)
    .filter((node) => ["span", "select", "input"].includes(node.tagName))
    .map((node) => (node.tagName === "span" ? node.textContent : `${node.tagName}#${node.dataset.id ?? node.id}`));
}

test("a shortcut picker draws under the title of the row it fills", () => {
  const effort = draw(MODEL_API.values, () => {}, () => {}, DEVELOPMENT);
  assert.deepEqual(rowReading(effort.row("director_reasoning_effort")), [
    "Reasoning effort",
    "select#director_reasoning_effort_pick",
    "input#set-f-director_reasoning_effort",
  ]);
  assert.equal(effort.row("reasoning_effort_pick"), undefined, "no picker row of its own above the title");

  const endpoint = draw(MODEL_API.values);
  assert.deepEqual(rowReading(endpoint.row("director_base_url")), [
    "Base URL",
    "select#director_base_url_pick",
    "input#set-f-director_base_url",
  ]);
  assert.equal(endpoint.row("base_url_pick"), undefined);
});
