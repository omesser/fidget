// Every English string the app sends through a catalog, read out of the source.
// Shared by the catalog test and by whoever adds a language and needs the list.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");

// Strings that reach a catalog by value rather than as a literal at the call:
// popup titles the page translates for display, and lookup tables.
export const INDIRECT = [
  "Presence",
  "Character",
  "AI",
  "Chat",
  "Privacy",
  "Caffeinate",
  "Development",
  "System",
  "Light",
  "Dark",
  "Minimal",
  "Terminal",
  "Glass",
  "Custom",
  "Model API",
  "Harness · Custom",
  "low",
  "medium",
  "high",
  "No answer came back.",
  "Connect an AI to talk to me",
  "No AI connected yet.",
  "Window names are off",
  "The fidget knows where your windows are, not what they are. One switch in Settings turns on titles and application names together.",
  "Open Settings",
  "Don't show this again",
  "The Harness asked for permission without saying what for.",
  "You asked something else",
  "You poked me",
  "You threw me",
  "You summoned me",
  "You picked me up",
  "I perched",
  "You started driving me",
  "You stopped driving me",
  "My next thought started",
  "just poked",
  "just summoned",
  "just thrown",
  "just grabbed",
  "just dropped",
  "just spoken to",
  "At least one caffeinate flag has to stay on.",
  "a new session was started",
  "settings changed what answers",
  "the Instance Prompt changed",
  "something can answer now",
  "the Character changed",
];

// A Rust string literal's value: escapes read, and a `\` line continuation
// eating the newline and the indentation after it.
function rustLiteral(body) {
  return body
    .replace(/\\\r?\n\s*/g, "")
    .replace(/\\x([0-9a-fA-F]{2})/g, (_, hex) => String.fromCharCode(parseInt(hex, 16)))
    .replace(/\\(.)/g, (_, c) => ({ n: "\n", t: "\t", r: "\r", 0: "\0" })[c] ?? c);
}

function jsLiteral(body) {
  return body.replace(/\\(.)/g, (_, c) => ({ n: "\n", t: "\t" })[c] ?? c);
}

const LITERAL = /"((?:[^"\\]|\\[\s\S])*)"/g;

// The text from `open` (just past a `(`) to its matching `)`, skipping strings.
function argumentsAt(source, open) {
  let depth = 1;
  for (let i = open; i < source.length; i += 1) {
    const c = source[i];
    if (c === '"') {
      i += 1;
      while (i < source.length && source[i] !== '"') i += source[i] === "\\" ? 2 : 1;
    } else if (c === "(") depth += 1;
    else if (c === ")" && --depth === 0) return source.slice(open, i);
  }
  return "";
}

function withoutTests(source) {
  const cut = source.search(/^#\[cfg\(test\)\]\s*\n\s*(pub(\(crate\))? )?mod \w*tests/m);
  return cut === -1 ? source : source.slice(0, cut);
}

function rustKeys(file, keys) {
  // A `cfg!` names a platform, not a word to translate.
  const source = withoutTests(fs.readFileSync(file, "utf8")).replace(/target_os = "\w+"/g, "");
  const form = file.endsWith(path.join("settings", "form.rs"));
  // tr: every literal in the argument, so `tr(if mac { "a" } else { "b" })`
  // counts both. trf: the template only; the rest are placeholder names.
  for (const call of source.matchAll(/\b(tr|trf)\(/g)) {
    const args = argumentsAt(source, call.index + call[0].length);
    const literals = [...args.matchAll(LITERAL)].map((m) => rustLiteral(m[1]));
    for (const literal of call[1] === "trf" ? literals.slice(0, 1) : literals) keys.add(literal);
  }
  // The form's row helpers translate the labels and help they are handed.
  if (form) {
    for (const call of source.matchAll(/\b(env_row_parts|switch_row_parts|harness_env_row_parts|flag_row|row)\(/g)) {
      const args = argumentsAt(source, call.index + call[0].length);
      for (const m of args.matchAll(LITERAL)) keys.add(rustLiteral(m[1]));
    }
  }
}

function jsKeys(file, keys) {
  const source = fs.readFileSync(file, "utf8");
  for (const call of source.matchAll(/\bt\(\s*"((?:[^"\\]|\\.)*)"/g)) keys.add(jsLiteral(call[1]));
}

// `data-i18n` text with its slot children as `{slot}`, and `data-i18n-attr`
// values. An element marked `data-i18n` holds text and slot spans only, so its
// text runs to the first closing tag of its own name.
function htmlKeys(file, keys) {
  const source = fs.readFileSync(file, "utf8");
  for (const open of source.matchAll(/<(\w+)\b[^>]*\sdata-i18n(?=[\s>])[^>]*>/g)) {
    const from = open.index + open[0].length;
    const text = source
      .slice(from, source.indexOf(`</${open[1]}>`, from))
      .replace(/<span[^>]*data-i18n-slot="(\w+)"[^>]*><\/span>/g, "{$1}")
      .replace(/\s+/g, " ")
      .trim();
    if (text) keys.add(text);
  }
  for (const m of source.matchAll(/<[^>]*data-i18n-attr="([^"]+)"[^>]*>/g)) {
    for (const attr of m[1].split(",").map((name) => name.trim())) {
      const value = m[0].match(new RegExp(`\\s${attr}="([^"]*)"`));
      if (value) keys.add(value[1]);
    }
  }
}

function filesIn(dir, ext) {
  return fs
    .readdirSync(dir, { recursive: true })
    .map((name) => path.join(dir, String(name)))
    .filter((file) => file.endsWith(ext) && !file.includes(`${path.sep}vendor${path.sep}`));
}

export function catalogKeys() {
  const keys = new Set(INDIRECT);
  for (const file of filesIn(path.join(ROOT, "src-tauri", "src"), ".rs")) rustKeys(file, keys);
  for (const file of filesIn(path.join(ROOT, "src"), ".js")) jsKeys(file, keys);
  for (const file of filesIn(path.join(ROOT, "src"), ".html")) htmlKeys(file, keys);
  return keys;
}

export function readCatalog(lang) {
  return JSON.parse(fs.readFileSync(path.join(ROOT, "src", "locales", `${lang}.json`), "utf8"));
}
