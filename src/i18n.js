// The interface language, as the Shell put it in force. `i18n.rs` injects the
// catalog before any module runs and pushes a new one on `locale-changed`, so
// the page and the Rust half of a screen read the same words. English is the
// source text: a string the catalog lacks reads as written.

const injected = globalThis.window?.__FIDGET_LOCALE__;
let lang = injected?.lang ?? "en";
let strings = injected?.strings ?? {};
const listeners = new Set();

export function locale() {
  return lang;
}

// `en` in the language in force, each `{name}` filled from `args`.
export function t(en, args) {
  const text = Object.hasOwn(strings, en) ? strings[en] : en;
  if (!args) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name) => (Object.hasOwn(args, name) ? String(args[name]) : whole));
}

// The English an element was written with. A child marked `data-i18n-slot`
// reads as `{slot}`, so a translation can move it within the sentence.
function sourceOf(node) {
  const text = [...node.childNodes]
    .map((child) => (child.nodeType === 1 ? `{${child.dataset.i18nSlot ?? ""}}` : child.textContent))
    .join("");
  return text.replace(/\s+/g, " ").trim();
}

// The translation laid back over the element, its slot children moved into
// place rather than rebuilt, so script that filled them keeps its nodes.
function fillSlots(node, template) {
  const slots = new Map([...node.children].map((child) => [child.dataset.i18nSlot, child]));
  if (slots.size === 0) {
    node.textContent = template;
    return;
  }
  const parts = template.split(/\{(\w+)\}/);
  node.replaceChildren(
    ...parts.map((part, index) => (index % 2 === 1 && slots.has(part) ? slots.get(part) : document.createTextNode(index % 2 === 1 ? `{${part}}` : part))),
  );
}

// Static markup names what to translate: `data-i18n` for the text, and
// `data-i18n-attr` for a comma list of attributes. The English is kept on the
// element, so a second language change starts from the source, not the first.
export function localize(root = globalThis.document) {
  if (!root?.querySelectorAll) return;
  if (root.documentElement) root.documentElement.lang = lang;
  for (const node of root.querySelectorAll("[data-i18n]")) {
    if (!node.dataset.i18n) node.dataset.i18n = sourceOf(node);
    fillSlots(node, t(node.dataset.i18n));
  }
  for (const node of root.querySelectorAll("[data-i18n-attr]")) {
    for (const attr of node.dataset.i18nAttr.split(",").map((name) => name.trim())) {
      const key = `i18nSource${attr.replace(/(^|-)(\w)/g, (_, __, c) => c.toUpperCase())}`;
      if (node.dataset[key] === undefined) node.dataset[key] = node.getAttribute(attr) ?? "";
      node.setAttribute(attr, t(node.dataset[key]));
    }
  }
}

// Called after a new catalog lands. A page redraws what it built in script.
export function onLocaleChange(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function setLocale(next) {
  lang = next?.lang ?? "en";
  strings = next?.strings ?? {};
  localize();
  for (const listener of listeners) listener(lang);
}

const events = globalThis.window?.__TAURI__?.event;
if (events) events.listen("locale-changed", (event) => setLocale(event.payload));
