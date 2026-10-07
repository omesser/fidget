import { emptyJsonBlock, onlyEmptyJson, stripUnsafe } from "./markdown.js";

// What a permission ask says, as parts the consent row draws. Its own
// module because chat.js reaches window.__TAURI__ as it loads and cannot be
// imported outside a webview; this can, so it has a test.

// Everything but the copy here is untrusted: `title`, `content`, `input` and
// `locations` come from the Harness, and an MCP server can steer all four.
// This file produces no markup. The row draws content through `drawReply`.

// How long an elicitation question may be, in characters. A longer one spends
// the row and pushes Decline off the window.
const DETAIL_LIMIT = 600;

// An ask that carried nothing to describe itself. Said as a sentence, so it
// reads as an absence the Harness is responsible for rather than as detail
// this window dropped.
const SILENT = "The Harness asked for permission without saying what for.";

// One untrusted string on one line. A newline or bidi mark could forge a line
// the Harness never sent. `\p{C}` is every control and format character; `\s`
// finishes the whitespace.
export function flat(text) {
  return String(text)
    .replace(/\p{C}+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export function clamp(text, limit) {
  return text.length > limit ? `${text.slice(0, limit - 1)}…` : text;
}

// What the row draws, in order: `{ kind, text }`. `markdown` is Harness
// content, or `input` as a JSON fence when there is no content. `metadata`
// is the `kind · paths` tail. `drawReply` draws the markdown.
export function askSays(ask) {
  const title = stripUnsafe(ask?.title ?? "");
  const kind = stripUnsafe(ask?.kind ?? "");
  const content = (ask?.content ?? []).map((text) => String(text ?? "")).filter((text) => text.length > 0);
  const paths = (ask?.locations ?? []).map((where) => stripUnsafe(where)).filter((text) => text.length > 0);
  // An empty fence is not a body. Dropping it here keeps the sentence, and
  // keeps `input` from standing in for text the Harness did send.
  const visible = content.filter((text) => !onlyEmptyJson(text));
  // Content first, the arguments as the fallback, never both: a tool that
  // sends its question as content usually repeats it in `input`.
  const details = content.length > 0
    ? visible.map((text) => ({ kind: "markdown", text }))
    : inputMarkdown(ask?.input);
  // `other` is `ToolKind::Other`, which says only that the Harness declined to
  // classify the call. Every other kind separates reading from writing from
  // running, the distinction the answer turns on, so it rides at the end.
  const about = [kind === "other" ? "" : kind, paths.join(", ")].filter(Boolean).join(" · ");

  // A kind on its own is not an answer to "what am I approving": it names a
  // category, and the whole bug was a row that offered one in place of the
  // question. A path on its own is a fact worth drawing.
  if (!title && details.length === 0 && paths.length === 0) {
    return [{ kind: "prose", text: SILENT }];
  }
  return [
    { kind: "title", text: title },
    ...details,
    { kind: "metadata", text: about },
  ].filter(({ text }) => text.length > 0);
}

// No content, so the arguments are the body. An empty payload is no body,
// by the same check the ask row uses when it draws a fence.
function inputMarkdown(input) {
  if (input === undefined) {
    return [];
  }
  const body = JSON.stringify(input, null, 2);
  if (body === undefined || emptyJsonBlock(body)) {
    return [];
  }
  return [{ kind: "markdown", text: "```json\n" + body + "\n```" }];
}

// An elicitation form's question. The Harness wrote `message`, and a newline
// inside it must not forge a line.
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
