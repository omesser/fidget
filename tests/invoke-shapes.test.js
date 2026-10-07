// Every `invoke` the renderer makes must name a command the Shell registers and
// pass one object keyed by that command's parameters. A mismatch is silent at
// runtime: Tauri rejects the call, and most call sites swallow the rejection.

import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

// Arguments the Shell fills in itself. JS never sends them.
const INJECTED = /\b(AppHandle|Window|WebviewWindow|Webview|State)\b/;

// A command name held in a variable cannot be read off the call. Each such
// site names the commands it is called with, and those are checked instead.
const DYNAMIC = {
  "src/main.js:command": ["overlay_primary", "overlay_secondary"],
};

// Splits on commas outside brackets, strings and template literals. Rust
// generics nest in angle brackets; JS uses them only as operators.
function splitTop(text, angles = false) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    if (c === '"' || c === "'" || c === "`") {
      i = skipString(text, i);
    } else if ("([{".includes(c) || (angles && c === "<")) {
      depth += 1;
    } else if (")]}".includes(c) || (angles && c === ">" && text[i - 1] !== "-")) {
      depth -= 1;
    } else if (c === "," && depth === 0) {
      parts.push(text.slice(start, i).trim());
      start = i + 1;
    }
  }
  const last = text.slice(start).trim();
  if (last) parts.push(last);
  return parts;
}

function skipString(text, i) {
  const quote = text[i];
  for (let j = i + 1; j < text.length; j += 1) {
    if (text[j] === "\\") j += 1;
    else if (quote === "`" && text[j] === "$" && text[j + 1] === "{") j = skipBlock(text, j + 1);
    else if (text[j] === quote) return j;
  }
  return text.length;
}

function skipBlock(text, open) {
  let depth = 0;
  for (let j = open; j < text.length; j += 1) {
    const c = text[j];
    if (c === '"' || c === "'" || c === "`") j = skipString(text, j);
    else if ("([{".includes(c)) depth += 1;
    else if (")]}".includes(c) && --depth === 0) return j;
  }
  return text.length;
}

// Rust lifetimes (`'_`) are not quotes, and a signature holds no strings, so
// the parameter list ends at the first unmatched `)`.
function signatureParams(rust, start) {
  let depth = 0;
  for (let i = start; i < rust.length; i += 1) {
    if (rust[i] === "(") depth += 1;
    else if (rust[i] === ")" && depth-- === 0) return rust.slice(start, i);
  }
  return rust.slice(start);
}

function commandsOf(rust) {
  const handler = rust.match(/generate_handler!\[([^\]]*)\]/);
  const registered = new Set(handler[1].split(",").map((name) => name.trim()).filter(Boolean));
  const commands = new Map();
  for (const found of rust.matchAll(/#\[tauri::command\]\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)\s*\(/g)) {
    const open = found.index + found[0].length;
    const params = splitTop(signatureParams(rust, open), true)
      .map((param) => param.match(/^(?:mut\s+)?(\w+)\s*:\s*(.+)$/s))
      .filter((param) => param && !INJECTED.test(param[2]))
      .map(([, name, type]) => ({
        key: name.replace(/_(\w)/g, (_, c) => c.toUpperCase()),
        optional: /^Option</.test(type.trim()),
      }));
    if (registered.has(found[1])) commands.set(found[1], params);
  }
  return commands;
}

// Every `invoke(` in code, skipping strings and comments, with its arguments.
function invokesOf(file, source) {
  const calls = [];
  for (let i = 0; i < source.length; i += 1) {
    const c = source[i];
    if (c === '"' || c === "'" || c === "`") {
      i = skipString(source, i);
    } else if (c === "/" && source[i + 1] === "/") {
      i = source.indexOf("\n", i);
      if (i < 0) break;
    } else if (c === "/" && source[i + 1] === "*") {
      i = source.indexOf("*/", i) + 1;
    } else if (source.startsWith("invoke(", i) && !/[\w$]/.test(source[i - 1] ?? "")) {
      const open = i + "invoke".length;
      const args = splitTop(source.slice(open + 1, skipBlock(source, open)));
      const line = source.slice(0, i).split("\n").length;
      calls.push({ where: `${file}:${line}`, file, args });
      i = open;
    }
  }
  return calls;
}

function keysOf(arg) {
  if (!arg.startsWith("{") || !arg.endsWith("}")) return null;
  const keys = [];
  for (const prop of splitTop(arg.slice(1, -1))) {
    const key = prop.match(/^(?:"(\w+)"|'(\w+)'|([A-Za-z_$][\w$]*))\s*(?::|$)/);
    if (!key) return null;
    keys.push(key[1] ?? key[2] ?? key[3]);
  }
  return keys;
}

function shapeProblems(where, name, args, commands) {
  const params = commands.get(name);
  if (!params) return [`${where}: "${name}" is not a registered command`];
  if (args.length > 1) return [`${where}: "${name}" takes one object, not ${args.length} arguments`];
  const keys = args.length === 0 ? [] : keysOf(args[0]);
  if (keys === null) return [`${where}: "${name}" needs an object literal keyed by its parameters`];
  const problems = [];
  for (const key of keys) {
    if (!params.some((param) => param.key === key)) {
      problems.push(`${where}: "${name}" has no parameter "${key}"`);
    }
  }
  for (const param of params) {
    if (!param.optional && !keys.includes(param.key)) {
      problems.push(`${where}: "${name}" is missing "${param.key}"`);
    }
  }
  return problems;
}

function check(sources, rust) {
  const commands = commandsOf(rust);
  const problems = [];
  const checked = new Set();
  for (const [file, source] of sources) {
    for (const { where, args } of invokesOf(file, source)) {
      const [first, ...rest] = args;
      const literal = first?.match(/^"(\w+)"$|^'(\w+)'$/);
      const names = literal ? [literal[1] ?? literal[2]] : DYNAMIC[`${file}:${first}`];
      if (!names) {
        problems.push(`${where}: the command name ${first} cannot be read off the call`);
        continue;
      }
      for (const name of names) {
        checked.add(name);
        problems.push(...shapeProblems(where, name, rest, commands));
      }
    }
  }
  return { problems, checked };
}

const RUST = `
#[tauri::command]
fn greet(app: tauri::AppHandle, first_name: String, title: Option<String>, state: tauri::State<'_, Things>) {}
#[tauri::command]
async fn ping(state: tauri::State<'_, Things>) -> bool { true }
tauri::generate_handler![greet, ping]
`;

test("the checker passes object-keyed calls and rejects the shapes Tauri drops", () => {
  const problems = (code) => check([["fake.js", code]], RUST).problems;
  assert.deepEqual(problems('invoke("greet", { firstName, title: (a) => a > 1 }); invoke("ping");'), []);
  assert.deepEqual(problems('invoke("greet", "Ada", "Dr");'), [
    'fake.js:1: "greet" takes one object, not 2 arguments',
  ]);
  assert.deepEqual(problems('invoke("gone", {});'), [
    'fake.js:1: "gone" is not a registered command',
  ]);
  assert.deepEqual(problems('invoke("greet", { first_name });'), [
    'fake.js:1: "greet" has no parameter "first_name"',
    'fake.js:1: "greet" is missing "firstName"',
  ]);
  assert.deepEqual(problems("invoke(name, {});"), [
    "fake.js:1: the command name name cannot be read off the call",
  ]);
  assert.deepEqual(problems('// invoke("gone", {})\nconst s = "invoke(1, 2)";'), []);
});

test("every renderer invoke names a registered command with its parameters as keys", () => {
  const src = join(root, "src");
  const sources = readdirSync(src)
    .filter((name) => name.endsWith(".js"))
    .map((name) => [`src/${name}`, readFileSync(join(src, name), "utf8")]);
  const rust = readFileSync(join(root, "src-tauri/src/main.rs"), "utf8");
  const { problems, checked } = check(sources, rust);
  assert.deepEqual(problems, []);
  for (const name of ["overlay_report_qm_draft", "overlay_trace_bubble", "names_hint_act"]) {
    assert.ok(checked.has(name), `${name} was found and checked`);
  }
});
