import { emptyJsonBlock, onlyEmptyJson, stripUnsafe } from "./markdown.js";
import { t } from "./i18n.js";

// What a permission ask says, as parts the consent row draws. `title`,
// `content`, `marks`, `input`, and `locations` are untrusted Harness text. This
// file produces no markup.

// Said as a sentence, so the absence reads as the Harness's, not as detail
// this window dropped.
const SILENT = "The Harness asked for permission without saying what for.";

// What the row draws, in order. `markdown` is Harness content, or `input`
// as a JSON fence when there is no content.
export function askSays(ask) {
  const title = stripUnsafe(ask?.title ?? "");
  const kind = stripUnsafe(ask?.kind ?? "");
  const content = (ask?.content ?? []).map((text) => String(text ?? "")).filter((text) => text.length > 0);
  const paths = (ask?.locations ?? []).map((where) => stripUnsafe(where)).filter((text) => text.length > 0);
  // An empty fence is not a body. Dropping it keeps the sentence, and keeps
  // `input` from standing in for text the Harness did send.
  const visible = content.filter((text) => !onlyEmptyJson(text));
  // Content first, the arguments as the fallback, never both. A tool that
  // sends its question as content usually repeats it in `input`.
  const said = content.length > 0
    ? visible.map((text) => ({ kind: "markdown", text }))
    : inputMarkdown(ask?.input);
  // A mark names a block but says nothing, so `input` still stands in.
  const marks = (ask?.marks ?? []).map((text) => String(text ?? "")).filter((text) => text.length > 0);
  const details = [...said, ...marks.map((text) => ({ kind: "markdown", text }))];
  // `other` only means the Harness declined to classify the call. Every other
  // kind separates reading, writing, and running, so it rides at the end.
  const about = [kind === "other" ? "" : kind, paths.join(", ")].filter(Boolean).join(" · ");

  // A kind on its own is not an answer to "what am I approving". A path on
  // its own is a fact worth drawing.
  if (!title && details.length === 0 && paths.length === 0) {
    return [{ kind: "prose", text: t(SILENT) }];
  }
  return [
    { kind: "title", text: title },
    ...details,
    { kind: "metadata", text: about },
  ].filter(({ text }) => text.length > 0);
}

// No content, so the arguments are the body. An empty payload is no body,
// by the same check the ask row uses when it draws a fence.
export function inputMarkdown(input) {
  if (input === undefined) {
    return [];
  }
  const body = JSON.stringify(input, null, 2);
  if (body === undefined || emptyJsonBlock(body)) {
    return [];
  }
  return [{ kind: "markdown", text: "```json\n" + body + "\n```" }];
}

// The Harness's question, with its lines and length kept. `stripUnsafe`
// drops what could hide or reverse text.
export function elicitSays(form) {
  const message = stripUnsafe(form?.message ?? "");
  return message.trim() ? message : t("The Harness asked a question without saying what for.");
}

// Answers in order, Decline last, because the protocol treats it as a valid
// answer. A URL form's one yes opens its link. A null `value` is Decline.
export function elicitChoices(form) {
  const choices = form?.url
    ? [{ name: t("Open"), value: "open", url: form.url }]
    : (form?.options ?? []).map((option) => ({ name: option.name || option.value, value: option.value }));
  return [...choices, { name: t("Decline"), value: null }];
}
