import { mountChatAppearance } from "./chat-appearance.js";

// One interpreter of `form::describe()`, in two halves. `controls(tab, values)`
// is pure: the tab as a flat list of {role, id, label, value, frozen} in render
// order, the same rows `ax-settings.swift dump` reads, so tests need no window.
// `render(root, tab, values, emit, stage)` is the DOM half; a redraw is render()
// again. `stage` receives a batched row's edit, which no redraw may lose.

// `emit` routes events to the `settings_event` Tauri command.

// `values` is keyed by form row id and carries one scalar per row, except the
// Instances list, which carries the rows themselves. A list item draws as
// Name (character) and dismisses by id: the printed line is ambiguous between
// two fidgets of the same name and Character. #875.
function listItems(value) {
  const items = Array.isArray(value) ? value : [];
  return items.map((item) => ({ label: `${item.name} (${item.character})`, id: item.id }));
}

// The AI source picker's Model API title, as `form::HARNESS_OFF` spells it.
const MODEL_API = "Model API";
// Apply and Cancel sit in the Model / API section but commit the whole tab.
const FOOTER_ROW = "director_actions";
const OFF_NOTE = Object.freeze({
  harness: "Not in use: the AI source is Model API.",
  model_api: "Not in use: the AI source is a Harness.",
});

// A section serving the source the picker does not name is off, and its rows
// draw frozen (#1427). The picker's value is the staged one, so the tab follows
// a pick before Apply saves it.
function offSource(section, values) {
  if (!section.serves) return null;
  const picked = values.harness === MODEL_API ? "model_api" : "harness";
  return section.serves === picked ? null : section.serves;
}

function rowsOf(section, values) {
  if (!offSource(section, values)) return section.rows;
  return section.rows.map((row) => {
    if (row.id === FOOTER_ROW) return row;
    if (row.type === "Composite") {
      return { ...row, controls: row.controls.map((control) => ({ ...control, frozen: true })) };
    }
    return { ...row, frozen: true, editable: false };
  });
}

export function controls(tab, values) {
  const out = [];
  const push = (role, id, label, value, frozen) => out.push({ role, id, label, value, frozen });

  for (const section of tab.sections) {
    push("heading", null, section.heading, null, false);

    for (const row of rowsOf(section, values)) {
      switch (row.type) {
        case "Checkbox":
          push("checkbox", row.id, row.label, Boolean(values[row.id]), row.frozen);
          break;
        case "TextField":
          push("textfield", row.id, row.label ?? null, values[row.id] ?? "", row.frozen);
          break;
        case "SecureField":
          // A stored key never reaches a renderer. Whether one is set is the
          // row's placeholder, so the field itself has no value to draw.
          push("securefield", row.id, row.label ?? null, "", row.frozen);
          break;
        case "Popup":
          push("popup", row.id, row.label ?? null, choice(row, values), row.frozen);
          break;
        case "Multiline":
          push("textarea", row.id, row.label ?? null, values[row.id] ?? "", !row.editable);
          break;
        case "InspectBlock":
          push("statictext", row.id, row.label ?? null, values[row.id] ?? "", true);
          break;
        case "InspectPath":
          push("statictext", row.id, null, values[row.id] ?? "", true);
          break;
        case "List":
          for (const item of listItems(values[row.id])) {
            push("statictext", row.id, item.label, item.label, false);
            push("button", row.id, row.dismiss_label, item.label, false);
          }
          break;
        case "Composite":
          for (const control of row.controls) {
            if (control.type === "Button") {
              push("button", control.id, control.label, null, control.frozen);
            } else if (control.type === "Popup") {
              push("popup", control.id, null, choice(control, values), control.frozen);
            } else {
              push("textfield", control.id, null, values[control.id] ?? "", false);
            }
          }
          break;
        default:
          // A variant added to `FormRow` and not to this switch shows up as a
          // row the tests can name, rather than as a row that quietly vanishes.
          push("unknown", row.id ?? null, row.type, null, false);
      }
    }
  }

  return out;
}

function choice(row, values) {
  return values[row.id] ?? row.options[0] ?? "";
}

// --- DOM -------------------------------------------------------------------

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === null || value === undefined || value === false) continue;
    if (key === "text") node.textContent = value;
    else if (value === true) node.setAttribute(key, "");
    else node.setAttribute(key, value);
  }
  node.append(...children.filter((child) => child !== null && child !== undefined));
  return node;
}

// "What is this?" is a <details>: the browser owns open and closed, the closed
// state reserves no space, and a reader announces it as a disclosure.
function disclosure(text) {
  return text
    ? el("details", { class: "set-disclosure" }, el("summary", { text: "What is this?" }), el("p", { text }))
    : null;
}

function help(text) {
  return text ? el("p", { class: "set-help", text }) : null;
}

function status(text) {
  return text ? el("p", { class: "set-status", text }) : null;
}

// The Harness did not take this row's value. The Shell sends the sentence under
// `<row id>_notice` only while it holds, and says what Fidget put back. A
// refusal is something the user must see, so it is an alert, not a muted note.
function notice(text) {
  return text ? el("p", { class: "set-notice", role: "alert", text }) : null;
}

const PI_HARNESS = "Harness · pi";

function notes(row) {
  return [help(row.help), status(row.status), disclosure(row.disclosure)];
}

// The control sits inside its label, the way a checkbox row already does.
// WebKit publishes a label holding nothing but text as an AXStaticText carrying
// that text with its own text run beneath, so the row's name reached the
// accessibility tree twice and a dump read the copy as the row's control. A
// label holding a control is an AXGroup instead. #706.
// `body` is what sits under the title when it is more than the control alone.
function labelled(row, control, extra = [], body = control) {
  const id = `set-f-${row.id}`;
  control.id = id;
  return el(
    "div",
    { class: `set-row${row.frozen ? " set-is-frozen" : ""}`, "data-row": row.id },
    row.label ? el("label", { for: id }, el("span", { text: row.label }), body) : body,
    ...extra,
  );
}

function popup(row, values, frozen) {
  const select = el("select", { disabled: frozen });
  const value = values[row.id];
  for (const option of row.options) {
    select.append(el("option", { value: option, selected: option === value, text: option }));
  }
  select.dataset.id = row.id;
  return select;
}

// A popup that only fills in a text row, which is how `form.rs` describes the
// Base URL and Reasoning effort pickers. Drawn as a row of its own it sat
// above its field's title with no title of its own (#1426).
function shortcutOf(row) {
  const [control, ...rest] = row.type === "Composite" ? row.controls : [];
  return control?.type === "Popup" && control.fills && rest.length === 0 ? control : null;
}

function picker(control, values, emit) {
  const select = popup(control, values, control.frozen);
  select.addEventListener("change", () => emit({ pick: control.id, value: select.value, fills: control.fills }));
  return select;
}

function drawRow(row, values, emit, stage, tab, shortcut = null) {
  switch (row.type) {
    case "Checkbox": {
      const input = el("input", { type: "checkbox", disabled: row.frozen });
      input.checked = Boolean(values[row.id]);
      input.addEventListener("change", () => {
        if (row.batched) stage(row.id, input.checked);
        else emit({ set_bool: row.id, value: input.checked });
      });
      const node = el(
        "div",
        { class: `set-row set-check${row.frozen ? " set-is-frozen" : ""}`, "data-row": row.id },
        el("label", {}, input, el("span", { text: row.label })),
        ...notes(row),
      );
      if (row.id === "pi_project_mcp") node.hidden = values.harness !== PI_HARNESS;
      return node;
    }
    case "TextField": {
      const input = el("input", {
        type: "text",
        placeholder: row.placeholder,
        readonly: row.frozen,
        "aria-readonly": row.frozen ? "true" : null,
      });
      input.value = values[row.id] ?? "";
      // A batched row is a draft until Apply. A blur write would retarget
      // before Cancel could restore the row (#663), and a draft left in the
      // widget alone is gone on the next tab switch, which redraws the panel.
      // A field drawn twice, the Model under Harness and under Model / API,
      // stages and shows one value in both rows (#1427).
      if (row.batched) {
        input.addEventListener("input", () => {
          const root = input.closest?.('[role="tabpanel"]') ?? input.getRootNode();
          for (const twin of twins(tab, row)) {
            stage(twin.id, input.value);
            const other = twin.id === row.id ? null : root.querySelector?.(`[data-row="${twin.id}"] input`);
            if (other) other.value = input.value;
          }
        });
      } else {
        input.addEventListener("blur", () => emit({ set_text: row.id, value: input.value }));
      }
      // A frozen row is not the one in use, so a refusal is not its news.
      const refused = row.frozen ? null : notice(values[`${row.id}_notice`]);
      if (!shortcut) return labelled(row, input, [refused, ...notes(row)]);
      const line = el("div", { class: "set-shortcut" }, picker(shortcut.controls[0], values, emit), input);
      return labelled(row, input, [refused, help(shortcut.help), ...notes(row)], line);
    }
    case "SecureField": {
      const input = el("input", {
        type: "password",
        placeholder: row.placeholder,
        readonly: row.frozen,
        autocomplete: "off",
      });
      // Always batched (`FormRow::SecureField` carries no flag), so it only
      // stages: a key reaches the store through Apply's draft.
      input.value = values[row.id] ?? "";
      input.addEventListener("input", () => stage(row.id, input.value));
      return labelled(row, input, [status(row.status)]);
    }
    case "Popup": {
      const select = popup(row, values, row.frozen);
      if (row.batched) {
        select.addEventListener("change", () => {
          stage(row.id, select.value);
          if (row.id === "byo_harness") emit({ pick: row.id, value: select.value });
        });
      } else {
        select.addEventListener("change", () => emit({ pick: row.id, value: select.value }));
      }
      return labelled(row, select, notes(row));
    }
    case "Multiline": {
      const area = el("textarea", {
        rows: "4",
        readonly: !row.editable,
        "aria-readonly": row.editable ? null : "true",
      });
      area.value = values[row.id] ?? "";
      area.addEventListener("blur", () => emit({ set_text: row.id, value: area.value }));
      return labelled({ ...row, frozen: !row.editable }, area, [help(row.help), disclosure(row.disclosure)]);
    }
    case "InspectBlock": {
      // tabindex, because the block scrolls and a keyboard has to reach it.
      const block = el("pre", { class: "set-inspect", tabindex: "0", text: values[row.id] ?? "" });
      return el(
        "div",
        { class: "set-row", "data-row": row.id },
        row.label ? el("span", { class: "set-label", text: row.label }) : null,
        block,
        ...notes(row),
      );
    }
    case "InspectPath":
      return el(
        "div",
        { class: "set-row", "data-row": row.id },
        el("p", { class: "set-path", text: values[row.id] ?? "" }),
      );
    case "List": {
      const list = el("ul", { class: "set-list" });
      for (const item of listItems(values[row.id])) {
        const dismiss = el("button", {
          type: "button",
          text: row.dismiss_label,
          "data-item": item.label,
        });
        dismiss.addEventListener("click", () => emit({ dismiss: row.id, value: item.id }));
        list.append(el("li", {}, el("span", { text: item.label }), dismiss));
      }
      return el(
        "div",
        { class: "set-row", "data-row": row.id },
        list,
        help(row.help),
        disclosure(row.disclosure),
      );
    }
    case "Composite": {
      const line = el("div", { class: "set-controls" });
      // Only the page can read what these controls hold, and New spawns
      // under the name and Character beside it (#875).
      const fields = [];
      const typed = () => Object.fromEntries(fields.map((node) => [node.dataset.id, node.value]));
      for (const control of row.controls) {
        if (control.type === "Button") {
          const button = el("button", {
            type: "button",
            disabled: control.frozen,
            text: control.label,
            "data-id": control.id,
          });
          button.addEventListener("click", () => {
            const payload = { press: control.id, fields: typed() };
            if (control.id === "director_apply") {
              const root = button.closest('[role="tabpanel"]') ?? button.getRootNode();
              payload.draft = aiDraft(root, tab, values);
            }
            emit(payload);
          });
          line.append(button);
        } else if (control.type === "Popup") {
          const select = picker(control, values, emit);
          fields.push(select);
          line.append(select);
        } else {
          const input = el("input", { type: "text", placeholder: control.placeholder });
          input.dataset.id = control.id;
          input.value = values[control.id] ?? "";
          input.addEventListener("blur", () => emit({ set_text: control.id, value: input.value }));
          fields.push(input);
          line.append(input);
        }
      }
      return el(
        "div",
        { class: "set-row", "data-row": row.id },
        line,
        help(row.help),
        disclosure(row.disclosure),
      );
    }
    default:
      return el("div", { class: "set-row set-unknown", text: `unrendered row type ${row.type}` });
  }
}

// By the control's own id: a row's first control can be its shortcut picker.
function twins(tab, row) {
  return tab.sections
    .flatMap((section) => section.rows)
    .filter((other) => other.type === "TextField" && other.writes === row.writes);
}

function rowValue(root, id) {
  const control = root.querySelector?.(`#set-f-${id}`);
  return control ? control.value : "";
}

// Apply sends every row the form marks batched, so a new one needs no entry
// here. A cleared key draws as a blank field, and Rust reads a blank field as
// untouched, so the staged delete rides along as its own flag.
function aiDraft(root, tab, values) {
  const rows = tab.sections.flatMap((section) => section.rows);
  const draft = Object.fromEntries(
    rows
      .filter((row) => row.batched || row.type === "SecureField")
      .map((row) => [row.id, row.type === "Checkbox" ? rowChecked(root, row.id) : rowValue(root, row.id)]),
  );
  draft.clear_key = values.clear_key === true;
  return draft;
}

function rowChecked(root, id) {
  return Boolean(root.querySelector?.(`[data-row="${id}"] input`)?.checked);
}

// Every control this page draws, in render order, the footer's after the
// panel's. The list is a superset of what can hold focus (a label never does),
// which costs nothing because a position is only ever read back off the same
// list. Position is the one identity every control has: a row wrapper carries
// data-row, a labelled control an id, a Composite member data-id, a checkbox
// input and a summary nothing at all.
function drawnControls(root, footer) {
  return [...root.querySelectorAll(CONTROL_SELECTOR), ...(footer?.querySelectorAll(CONTROL_SELECTOR) ?? [])];
}

// A redraw is render() again, and replaceChildren() takes the focused control
// with it, so a commit left a keyboard user on the document and the next Tab
// started from the top (#937). The same tab redraws the same controls in the
// same order, so the position that held focus is handed back afterwards.
// Open disclosures close on rebuild the same way (#939), so their positions
// are recorded before replaceChildren() and reopened after.
export function render(root, tab, values, emit = () => {}, stage = () => {}) {
  const footer =
    typeof document !== "undefined" && typeof document.getElementById === "function"
      ? document.getElementById("set-footer")
      : null;
  const active = document.activeElement;
  const focused = active ? drawnControls(root, footer).indexOf(active) : -1;

  const allDetails = [
    ...(root.querySelectorAll?.("details") ?? []),
    ...(footer?.querySelectorAll?.("details") ?? []),
  ];
  const openPositions = allDetails
    .map((details, index) => (details.open ? index : -1))
    .filter((index) => index !== -1);

  root.replaceChildren();
  if (footer) footer.replaceChildren();

  for (const section of tab.sections) {
    const node = el("section", { class: "set-section" }, el("h2", { text: section.heading }));
    if (section.comment) node.append(el("p", { class: "set-comment", text: section.comment }));
    if (section.status) node.append(status(section.status));
    if (section.disclosure) node.append(disclosure(section.disclosure));
    const off = offSource(section, values);
    if (off) node.append(status(OFF_NOTE[off]));
    const shortcuts = new Map(
      rowsOf(section, values).filter(shortcutOf).map((row) => [shortcutOf(row).fills.row, row]),
    );
    for (const row of rowsOf(section, values)) {
      if (shortcutOf(row)) continue;
      if (row.type === "Composite" && row.id === FOOTER_ROW) {
        if (footer) {
          footer.append(drawRow(row, values, emit, stage, tab));
        } else {
          node.append(drawRow(row, values, emit, stage, tab));
        }
      } else {
        node.append(drawRow(row, values, emit, stage, tab, shortcuts.get(row.id)));
      }
    }
    root.append(node);
  }

  if (footer) {
    footer.style.display = footer.children.length > 0 ? "" : "none";
  }
  if (focused !== -1) drawnControls(root, footer)[focused]?.focus();

  const rebuiltDetails = [
    ...(root.querySelectorAll?.("details") ?? []),
    ...(footer?.querySelectorAll?.("details") ?? []),
  ];
  for (const position of openPositions) {
    if (rebuiltDetails[position]) rebuiltDetails[position].open = true;
  }
}

// Process a settings_event response into an outcome the page can act on.
// Exported for testing; the page wires it through invokeSettingsEvent.
export function processResponse(response) {
  switch (response.action) {
    case "refresh":
      return true;
    case "fill":
      return { fill: { id: response.id, value: response.value } };
    case "clear_key":
      return { clearKey: true };
    case "preview_byo":
      return { preview: {
        byo_snippet: response.snippet,
        byo_steps: response.steps,
        byo_token: response.token,
      } };
    case "reset":
      return { reset: true };
    case "run":
      return { run: response.operation };
    case "nothing":
    default:
      return false;
  }
}

// A tab switch redraws from the snapshot, so edits must stay in the draft
// keyed by row id until Apply or Cancel. A preview counts only while its pick
// is still the staged one, so a late answer after Cancel or a newer pick is
// dropped.
export function foldDraft(draft, outcome, payload) {
  if (!outcome || outcome === true) return draft;
  if (outcome.reset) return {};
  const next = { ...draft };
  if (outcome.clearKey) {
    delete next.director_api_key;
    next.clear_key = true;
  }
  if (outcome.fill) next[outcome.fill.id] = outcome.fill.value;
  if (outcome.preview && draft.byo_harness === payload?.value) Object.assign(next, outcome.preview);
  return next;
}

// A draft entry the store already holds is no draft: after a refresh it would
// mask a later external write to the same row.
export function pruneDraft(draft, values) {
  return Object.fromEntries(Object.entries(draft).filter(([id, value]) => value !== values[id]));
}

// The staged row keeps the preview it had until the new one arrives, so the
// snippet, steps, and token never draw empty in between.
export function stageDraft(draft, id, value) {
  return { ...draft, [id]: value };
}

// Apply and Cancel report only a draft that differs from the store.
export function pressFeedback(press, draft, values) {
  if (Object.keys(pruneDraft(draft, values)).length === 0) return null;
  return press === "director_cancel" ? "Changes discarded." : "Changes applied.";
}

export async function handleEvent(payload) {
  const response = await invokeSettingsEvent(payload);
  return processResponse(response);
}

// Native Settings copies in the window controller. The webview only gets
// Outcome::Run, so the page writes the string it already shows. #855.
const COPY_RUN_FIELDS = Object.freeze({
  copy_byo_snippet: "byo_snippet",
  copy_byo_token: "byo_token",
});

const COPY_PRESS_RUN = Object.freeze({
  byo_copy: "copy_byo_snippet",
  byo_copy_token: "copy_byo_token",
});

export async function writeRunClipboard(run, values, writeText) {
  const field = COPY_RUN_FIELDS[run];
  if (field === undefined) return;
  const text = values[field] ?? "";
  if (text === "") return;
  await writeText(text);
}

export async function applyEventOutcome(outcome, values, writeText) {
  if (!outcome) return false;
  await writeRunClipboard(outcome.run, values, writeText);
  return true;
}

export function copyRunForPress(payload) {
  return payload && typeof payload.press === "string" ? COPY_PRESS_RUN[payload.press] : undefined;
}

// --- The tab shell ---------------------------------------------------------

export function tabTitles(form) {
  return form.tabs.map((tab) => tab.title);
}

// Which tab a title selects. A title the form does not carry selects the first
// tab rather than nothing: a renamed tab would otherwise leave the page with a
// tablist and no panel.
export function selectTab(form, title) {
  const at = form.tabs.findIndex((tab) => tab.title === title);
  return at === -1 ? 0 : at;
}

// Wiring to Tauri command.
async function invokeSettingsEvent(payload) {
  if (typeof window.__TAURI_INTERNALS__ === "undefined") {
    return { action: "nothing" };
  }
  return await window.__TAURI_INTERNALS__.invoke("settings_event", { payload });
}

// What keeps a press instead of moving the window. `label` and `textarea` are
// here because this page renders both: a checkbox row is a <label> wrapping its
// input, and a Multiline row is a <textarea> whose drag has to select text.
export const CONTROL_SELECTOR = "input, textarea, select, button, summary, pre, label";

// Alt-drag gate predicate: drag begins only when modifier is held AND target is background.
export function shouldBeginDrag(event) {
  if (!event.altKey) return false;
  const isControl = event.target.closest(CONTROL_SELECTOR);
  return !isControl;
}

export function showError(message, onRetry) {
  const panel = document.querySelector('[role="tabpanel"]');
  if (!panel) return;
  panel.replaceChildren();
  const footer = document.getElementById("set-footer");
  if (footer) {
    footer.replaceChildren();
    footer.style.display = "none";
  }
  const errorDiv = document.createElement("div");
  errorDiv.className = "set-error";
  errorDiv.style.cssText = "padding: 2rem; text-align: center;";

  const errorText = document.createElement("p");
  errorText.textContent = message;
  errorText.style.marginBottom = "1rem";
  errorDiv.appendChild(errorText);

  if (onRetry) {
    const retryButton = document.createElement("button");
    retryButton.textContent = "Retry";
    retryButton.type = "button";
    retryButton.addEventListener("click", onRetry);
    errorDiv.appendChild(retryButton);
  }

  panel.appendChild(errorDiv);
}

// Snapshot + settings-refresh once Tauri is in the page; tab clicks still
// work without it so the shell does not sit dead in a non-Tauri load.
if (typeof document !== "undefined") {
  mountChatAppearance(document.documentElement);

  const tablist = document.querySelector('[role="tablist"]');
  const panel = document.querySelector('[role="tabpanel"]');

  let currentForm = null;
  let currentValues = null;
  let currentTabIndex = 0;
  let lastSnapshotPromise = null;
  let draft = {};
  let feedback = null;

  // The AI source pick decides which rows are live and whether the Pi row
  // shows, so it redraws the tab from the draft it just joined.
  function stage(id, value) {
    draft = stageDraft(draft, id, value);
    feedback = null;
    document.querySelector(".set-feedback")?.remove();
    if (id === "harness") renderCurrentTab();
  }

  async function loadSnapshot() {
    const currentLoad = (async () => {
      try {
        const snapshot = await window.__TAURI__.core.invoke("settings_snapshot");
        if (lastSnapshotPromise === currentLoad) {
          currentForm = snapshot.form;
          currentValues = snapshot.view;
          draft = pruneDraft(draft, currentValues);
          if (snapshot.reveal) {
            showReveal(snapshot.reveal);
          } else {
            renderCurrentTab();
          }
        }
      } catch (err) {
        if (lastSnapshotPromise === currentLoad) {
          showError("Could not load settings. Check that the app is running.", () => loadSnapshot());
        }
      }
    })();
    lastSnapshotPromise = currentLoad;
    return currentLoad;
  }

  // handleEvent already invokes settings_event; a truthy outcome (refresh,
  // fill, reset, clearKey, run) means the page's snapshot is stale. Copy
  // starts writeText in this click turn: awaiting settings_event first
  // drops the user gesture WebKit requires for the clipboard.
  async function emitEvent(payload) {
    const writeText = (text) => navigator.clipboard.writeText(text);
    const hinted = copyRunForPress(payload);
    const early =
      hinted !== undefined ? writeRunClipboard(hinted, { ...currentValues, ...draft }, writeText) : Promise.resolve();
    const staged = draft;
    try {
      const outcome = await handleEvent(payload);
      await early;
      draft = foldDraft(draft, outcome, payload);
      if (outcome?.reset) feedback = pressFeedback(payload.press, staged, currentValues);
      if (hinted !== undefined) {
        if (outcome) await loadSnapshot();
      } else if (await applyEventOutcome(outcome, { ...currentValues, ...draft }, writeText)) {
        await loadSnapshot();
      }
    } catch (err) {
      showError("Could not save changes. Check your connection.", () => {
        emitEvent(payload);
      });
    }
  }

  function renderCurrentTab() {
    if (!currentForm || !currentValues || !panel) return;
    const tab = currentForm.tabs[currentTabIndex];
    if (tab) {
      render(panel, tab, { ...currentValues, ...draft }, emitEvent, stage);
      if (feedback && tab.title === "AI") {
        document.getElementById("set-footer")?.append(el("p", {
          class: "set-feedback",
          role: "status",
          "aria-live": "polite",
          text: feedback,
        }));
      }
    }
  }

  // Points at a row. Does not check the box: the user still has to.
  function showReveal(reveal) {
    if (!currentForm || !panel) return;
    const tablist = document.querySelector('[role="tablist"]');
    const tabs = tablist ? Array.from(tablist.querySelectorAll('[role="tab"]')) : [];
    currentTabIndex = selectTab(currentForm, reveal.tab);
    for (let j = 0; j < tabs.length; j++) {
      tabs[j].setAttribute("aria-selected", String(j === currentTabIndex));
    }
    if (tabs[currentTabIndex]) {
      panel.setAttribute("aria-label", tabs[currentTabIndex].textContent);
    }
    renderCurrentTab();
    const row = panel.querySelector(`[data-row="${reveal.row}"]`);
    if (!row) return;
    row.scrollIntoView({ block: "nearest" });
    if (!row.hasAttribute("tabindex")) row.tabIndex = -1;
    row.focus();
  }

  if (tablist && panel) {
    const tabs = Array.from(tablist.querySelectorAll('[role="tab"]'));
    for (let i = 0; i < tabs.length; i++) {
      const tab = tabs[i];
      tab.addEventListener("click", () => {
        feedback = null;
        currentTabIndex = i;
        for (let j = 0; j < tabs.length; j++) {
          tabs[j].setAttribute("aria-selected", String(j === i));
        }
        panel.setAttribute("aria-label", tab.textContent);
        renderCurrentTab();
      });
    }
  }

  function initializeWithTauri() {
    const { listen } = window.__TAURI__.event;
    // The handle comes from getCurrentWebviewWindow, which is what this global
    // exports and what chat.js and main.js already call. Reaching for the
    // `window` module's name instead leaves both handlers below throwing.
    const settingsWindow = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();

    // Alt-drag to move the window, gated on modifier held and target is background.
    document.addEventListener("mousedown", (event) => {
      if (!shouldBeginDrag(event)) return;
      event.preventDefault();
      settingsWindow.startDragging();
    });

    // Escape closes the window.
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        settingsWindow.close();
      }
    });

    // Enter commits the active control (blur triggers its handler). Focus
    // comes straight back, so the redraw the commit triggers finds it there.
    document.addEventListener("keydown", (event) => {
      if (event.key === "Enter" && document.activeElement) {
        const active = document.activeElement;
        if (active.matches("input, textarea") && !active.matches('[type="checkbox"]')) {
          event.preventDefault();
          active.blur();
          active.focus();
        }
      }
    });

    // Tauri's listen() returns a Promise<UnlistenFn>. Window destruction does
    // not guarantee cleanup of window-scoped listeners, so we unlisten on unload.
    // Evidence: Tauri v2 docs state "listeners need to be manually unlistened"
    // and the returned unlisten function exists for this reason.
    let unlistenRefresh = null;
    listen("settings-refresh", () => {
      loadSnapshot();
    }).then((unlisten) => {
      unlistenRefresh = unlisten;
    });

    window.addEventListener("beforeunload", () => {
      if (unlistenRefresh) {
        unlistenRefresh();
      }
    });

    loadSnapshot();
  }

  function showTauriTimeoutError() {
    if (panel) {
      while (panel.firstChild) {
        panel.removeChild(panel.firstChild);
      }
      panel.setAttribute("role", "alert");
      panel.style.padding = "2rem";
      panel.style.color = "var(--txt)";
      const p = document.createElement("p");
      p.textContent =
        "Settings could not initialize: Tauri API is not available. " +
        "This is a packaging or WebView2 issue. Please restart the application.";
      panel.appendChild(p);
    }
  }

  function waitForTauri() {
    if (typeof window.__TAURI__ !== "undefined") {
      initializeWithTauri();
      return;
    }

    let attempts = 0;
    const maxAttempts = 50;
    const pollInterval = 100;

    const pollTimer = setInterval(() => {
      attempts++;

      if (typeof window.__TAURI__ !== "undefined") {
        clearInterval(pollTimer);
        initializeWithTauri();
        return;
      }

      if (attempts >= maxAttempts) {
        clearInterval(pollTimer);
        showTauriTimeoutError();
      }
    }, pollInterval);
  }

  waitForTauri();
}
