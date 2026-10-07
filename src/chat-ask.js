// What a permission ask says, as parts the consent row draws. Its own
// module because chat.js reaches window.__TAURI__ as it loads and cannot be
// imported outside a webview; this can, so it has a test.

// Everything but the copy here is untrusted: `title`, `content`, `input` and
// `locations` come from the Harness, and an MCP server can steer all four. The
// caller writes each part with `textContent` and this file produces no markup.

// How much of an ask the row may draw, in characters: about eleven wrapped
// lines in a 420-point window, enough for a question and its arguments, short
// enough that the answer buttons stay on screen. `input` is arbitrary JSON.
const DETAIL_LIMIT = 600;

// How many arguments the row names, and how much of each value. A call with
// more arguments than this has a shape to summarize rather than a payload to
// print, and the count of what was left says the rest.
const ARGUMENTS = 6;
const VALUE_LIMIT = 120;

// How many paths the row names before it counts them instead. Three fits the
// line; the count is what matters past that, because a tool touching a dozen
// files is a different decision than one touching three.
const PATHS = 3;

// An ask that carried nothing to describe itself. Said as a sentence, so it
// reads as an absence the Harness is responsible for rather than as detail
// this window dropped.
const SILENT = "The Harness asked for permission without saying what for.";

// One untrusted string, flattened to plain text on one line: a newline, tab or
// bidi override inside a tool's arguments could forge a fact the Harness never
// sent. `\p{C}` is every control and format character; `\s` finishes the whitespace.
export function flat(text) {
  return String(text)
    .replace(/\p{C}+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export function clamp(text, limit) {
  return text.length > limit ? `${text.slice(0, limit - 1)}…` : text;
}

// The first few of a list, with a count of the rest rather than the rest.
function first(list, keep, noun) {
  return list.length > keep
    ? [...list.slice(0, keep), `and ${list.length - keep} more ${noun}`]
    : list;
}

// The arguments as `key: value` lines rather than a JSON dump: the names are
// what tell a reader what the tool will do with the values. Anything that is
// not an object has no names to show, so it goes as the one line of JSON it is.
function argumentLines(input) {
  // An empty list or string is no payload, same as an empty object.
  if (
    input === null ||
    input === undefined ||
    input === "" ||
    (Array.isArray(input) && input.length === 0)
  ) {
    return [];
  }
  if (typeof input !== "object" || Array.isArray(input)) {
    return [clamp(flat(JSON.stringify(input)), VALUE_LIMIT)];
  }
  const named = Object.entries(input).map(([key, raw]) => {
    const shown = typeof raw === "string" ? raw : (JSON.stringify(raw) ?? String(raw));
    return `${flat(key)}: ${clamp(flat(shown), VALUE_LIMIT)}`;
  });
  return first(named, ARGUMENTS, "arguments");
}

// The row's budget, spent over the parts in order and cut once with an
// ellipsis. Every part is one flat line, so joining and splitting on newline
// loses nothing and leaves the cut where the old single string had it.
function withinBudget(parts) {
  const lines = clamp(parts.map(({ text }) => text).join("\n"), DETAIL_LIMIT).split("\n");
  return lines.map((text, at) => ({ kind: parts[at].kind, text }));
}

// A harness wraps arguments in a fence. Flattening would keep the markers,
// and the row would draw them as text. The body is the ask. The markers are not.
const FENCE =
  /(`{3,})([^\n`]*)\r?\n([\s\S]*?)\r?\n\1[ \t]*|(`{3,})([A-Za-z0-9_+.#-]*)[ \t]*([^`]*?)\4/g;

function fencePieces(text) {
  const raw = String(text ?? "");
  const pieces = [];
  let at = 0;
  for (const match of raw.matchAll(FENCE)) {
    if (match.index > at) {
      pieces.push({ fence: false, text: raw.slice(at, match.index) });
    }
    pieces.push({ fence: true, body: match[3] ?? match[6] ?? "" });
    at = match.index + match[0].length;
  }
  if (at < raw.length || pieces.length === 0) {
    pieces.push({ fence: false, text: raw.slice(at) });
  }
  return pieces;
}

function readJson(body) {
  try {
    return { value: JSON.parse(String(body).trim()) };
  } catch {
    return null;
  }
}

// Content entries, unwrapped. A fence whose body is JSON uses the argument
// lines. Any other fence is one code line. Prose beside a fence stays prose.
function contentLines(ask) {
  const plainKind = ask?.kind === "execute" ? "code" : "prose";
  const lines = [];
  for (const entry of ask?.content ?? []) {
    const pieces = fencePieces(entry);
    if (!pieces.some((piece) => piece.fence)) {
      const text = flat(entry ?? "");
      if (text) {
        lines.push({ kind: plainKind, text });
      }
      continue;
    }
    for (const piece of pieces) {
      if (!piece.fence) {
        const text = flat(piece.text);
        if (text) {
          lines.push({ kind: "prose", text });
        }
        continue;
      }
      const parsed = readJson(piece.body);
      if (parsed) {
        for (const text of argumentLines(parsed.value)) {
          lines.push({ kind: "code", text });
        }
        continue;
      }
      const text = flat(piece.body);
      if (text) {
        lines.push({ kind: "code", text });
      }
    }
  }
  return lines;
}

// What the row draws, in order: `{ kind, text }` per line, `kind` one of
// title, code, prose or metadata. Code is the thing being approved and gets a
// `<code>` from the renderer; metadata is the `kind · paths` tail.
export function askSays(ask) {
  // The title is untrusted too, and a verbose one would spend the row's budget
  // before the question arrived, from a merely chatty server.
  const title = clamp(flat(ask?.title ?? ""), VALUE_LIMIT);
  const fromContent = contentLines(ask);
  // Content first, arguments as the fallback, never both: a tool that sends
  // its question as content usually repeats it in `input`. Content is a
  // command only when the ask runs something; an argument line always is.
  const details = fromContent.length > 0
    ? fromContent
    : argumentLines(ask?.input).map((text) => ({ kind: "code", text }));

  const paths = (ask?.locations ?? [])
    .map((where) => clamp(flat(where), VALUE_LIMIT))
    .filter(Boolean);
  const kind = flat(ask?.kind ?? "");
  // `other` is `ToolKind::Other`, which says only that the Harness declined to
  // classify the call. Every other kind separates reading from writing from
  // running, the distinction the answer turns on, so it rides at the end.
  const about = [kind === "other" ? "" : kind, first(paths, PATHS, "paths").join(", ")]
    .filter(Boolean)
    .join(" · ");

  // A kind on its own is not an answer to "what am I approving": it names a
  // category, and the whole bug was a row that offered one in place of the
  // question. A path on its own is a fact worth drawing.
  if (!title && details.length === 0 && paths.length === 0) {
    return [{ kind: "prose", text: SILENT }];
  }
  return withinBudget(
    [{ kind: "title", text: title }, ...details, { kind: "metadata", text: about }].filter(({ text }) => text),
  );
}

// An elicitation form's question. Same flattening as a permission ask: the
// Harness wrote `message`, and a newline inside it must not forge a line.
export function elicitSays(form) {
  const message = clamp(flat(form?.message ?? ""), DETAIL_LIMIT);
  return message || "The Harness asked a question without saying what for.";
}

// A form's answers, in order, with Decline last because the protocol treats
// it as a valid answer. A URL form's one yes is opening its link. A null
// `value` is Decline.
export function elicitChoices(form) {
  const choices = form?.url
    ? [{ name: "Open", value: "open", url: form.url }]
    : (form?.options ?? []).map((option) => ({ name: option.name || option.value, value: option.value }));
  return [...choices, { name: "Decline", value: null }];
}
