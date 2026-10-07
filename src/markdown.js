// Markdown in a reply, drawn as formatting instead of as its punctuation. Only
// `marked`'s token API is used, never its HTML renderer: a reply is untrusted
// text, so every piece reaches the DOM via `createElement` and `textContent`.

import { Lexer } from "./vendor/marked.esm.js";

// Bidi controls can reverse a line. U+200B, word joiner and BOM can hide one.
// Newline and tab stay, because markdown uses them. ZWJ and ZWNJ stay too:
// joined emoji and some scripts need them.
const UNSAFE =
  /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F\u200B\u202A-\u202E\u2060\u2066-\u2069\uFEFF]/g;

export function stripUnsafe(text) {
  return String(text ?? "").replace(UNSAFE, "");
}

// GFM, because that is what harnesses emit: tables and task lists from Claude
// Code, fenced code with an info string from Codex, and Zed's ACP client turns
// the same extensions on. CommonMark alone would draw a table as a run of pipes.
const FLAVOUR = { gfm: true, breaks: false, pedantic: false };

// Schemes a link target may carry; an allowlist means `javascript:` or `data:`
// never reaches the DOM. Checked as a scheme rather than searched for as a
// string, so `JaVaScRiPt:` and `java<tab>script:` fail without their own rule.
const SCHEMES = new Set(["http:", "https:", "mailto:"]);

function target(href) {
  const trimmed = (href ?? "").trim();
  const scheme = /^[a-z][a-z0-9+.-]*:/i.exec(trimmed);
  return scheme && SCHEMES.has(scheme[0].toLowerCase()) ? trimmed : null;
}

// Whatever `marked` grew a token for and this file does not draw reads as the
// source the model wrote, so an unsupported construct is a line that looks
// unformatted rather than a line that vanished.
function asSource(token, parent, doc) {
  parent.append(doc.createTextNode(token.raw ?? token.text ?? ""));
}

function inline(tokens, parent, doc) {
  for (const token of tokens) {
    switch (token.type) {
      case "text":
      case "escape":
        parent.append(doc.createTextNode(token.text));
        break;
      case "strong":
      case "em":
      case "del":
        wrap(token.type, token, parent, doc);
        break;
      case "codespan": {
        const code = doc.createElement("code");
        code.textContent = token.text;
        parent.append(code);
        break;
      }
      case "br":
        parent.append(doc.createElement("br"));
        break;
      case "link": {
        const where = target(token.href);
        // A refused target leaves the link text behind rather than the row:
        // the words a model wrote are still what it said.
        const node = doc.createElement("span");
        if (where) {
          node.className = "md-link";
          node.dataset.href = where;
          node.title = where;
        }
        inline(token.tokens ?? [], node, doc);
        parent.append(node);
        break;
      }
      default:
        asSource(token, parent, doc);
    }
  }
}

function wrap(tag, token, parent, doc) {
  const node = doc.createElement(tag);
  inline(token.tokens ?? [], node, doc);
  parent.append(node);
}

function row(cells, tag, doc) {
  const node = doc.createElement("tr");
  for (const cell of cells) {
    const td = doc.createElement(tag);
    // `align` is one of three words `marked` derives from the delimiter row,
    // never text the model chose, and it names a class so the alignment stays
    // in the stylesheet with the rest of the table.
    if (cell.align) {
      td.className = `md-${cell.align}`;
    }
    inline(cell.tokens ?? [], td, doc);
    node.append(td);
  }
  return node;
}

function blocks(tokens, parent, doc, skipEmptyJson) {
  for (const token of tokens) {
    switch (token.type) {
      // Blank lines, and a link definition that already produced its link.
      case "space":
      case "def":
        break;
      case "paragraph":
        wrap("p", token, parent, doc);
        break;
      case "text":
        // A tight list item's line, which `marked` hands over as a block-level
        // `text` token holding the inline ones. It belongs in the item with no
        // paragraph of its own, so it flattens into the parent.
        inline(token.tokens ?? [token], parent, doc);
        break;
      case "heading":
        wrap(`h${token.depth}`, token, parent, doc);
        break;
      case "blockquote": {
        const quote = doc.createElement("blockquote");
        blocks(token.tokens ?? [], quote, doc, skipEmptyJson);
        parent.append(quote);
        break;
      }
      case "hr":
        parent.append(doc.createElement("hr"));
        break;
      case "list": {
        const list = doc.createElement(token.ordered ? "ol" : "ul");
        if (token.ordered && token.start > 1) {
          list.start = token.start;
        }
        for (const item of token.items) {
          const li = doc.createElement("li");
          blocks(item.tokens ?? [], li, doc, skipEmptyJson);
          list.append(li);
        }
        parent.append(list);
        break;
      }
      case "checkbox": {
        // Drawn and disabled. A live box would claim this surface can change
        // what the reply says, and it cannot: the reply is a log line.
        const box = doc.createElement("input");
        box.type = "checkbox";
        box.checked = Boolean(token.checked);
        box.disabled = true;
        parent.append(box);
        break;
      }
      case "code": {
        // Set by the ask row. A reply keeps an empty fence, because the model wrote it.
        if (skipEmptyJson && emptyJsonBlock(token.text)) {
          break;
        }
        // A fence and a table are the two blocks that do not reflow, and the
        // Chat window opens 420 points wide. The scroll goes on a wrapper so the
        // overflow stays inside the row instead of widening it.
        const fence = doc.createElement("div");
        fence.className = "md-wide";
        const pre = doc.createElement("pre");
        const code = doc.createElement("code");
        // The info string is the model's, so it is filtered to a word before
        // it names a class — the conventional hook a highlighter would read.
        const lang = /^[\w+#.-]+/.exec(token.lang ?? "");
        if (lang) {
          code.className = `language-${lang[0]}`;
        }
        code.textContent = token.text;
        pre.append(code);
        fence.append(pre);
        parent.append(fence);
        break;
      }
      case "table": {
        const scroll = doc.createElement("div");
        scroll.className = "md-wide";
        const table = doc.createElement("table");
        const head = doc.createElement("thead");
        head.append(row(token.header ?? [], "th", doc));
        const body = doc.createElement("tbody");
        for (const cells of token.rows ?? []) {
          body.append(row(cells, "td", doc));
        }
        table.append(head, body);
        scroll.append(table);
        parent.append(scroll);
        break;
      }
      default:
        asSource(token, parent, doc);
    }
  }
}

// True for a blank code body or JSON `{}`, `[]`, `null`, or `""`.
export function emptyJsonBlock(text) {
  const body = String(text ?? "").trim();
  if (body === "") {
    return true;
  }
  try {
    const value = JSON.parse(body);
    if (value === null || value === "") {
      return true;
    }
    if (Array.isArray(value)) {
      return value.length === 0;
    }
    return typeof value === "object" && Object.keys(value).length === 0;
  } catch {
    return false;
  }
}

// The reply drawn in each row so far, so a chunk can be added to it. Keyed on
// the element because that is what the surface holds; a removed row takes its
// entry with it.
const drawn = new WeakMap();

// Draw the whole of `text` into `body`, replacing whatever is there, and put
// the caret back. Found and re-added with `querySelector` and `append`, never
// `insertBefore`: the caret may be nested in a block, not a direct child of `body`.
export function drawReply(body, text, doc = globalThis.document, options) {
  const caret = body.querySelector(".caret");
  drawn.set(body, text ?? "");
  body.replaceChildren();
  blocks(Lexer.lex(stripUnsafe(text ?? ""), FLAVOUR), body, doc, options?.skipEmptyJson);
  if (caret) {
    // Inside the last paragraph, so it blinks at the end of the line rather
    // than on one of its own. Anywhere else — a fence, a table, an empty
    // reply — it goes back to the row, where it is at least valid.
    const last = body.lastElementChild;
    (last && last.tagName === "P" ? last : body).append(caret);
  }
}

// One chunk of a streaming reply. The row is redrawn from the whole reply
// rather than appended to, because a chunk lands mid-marker: `**bo` then
// `ld**` are two things that are neither of them Markdown.
export function appendReply(body, chunk, doc = globalThis.document) {
  drawReply(body, (drawn.get(body) ?? "") + chunk, doc);
}

// Replace the reply with the full answer-so-far. Used for streaming Speech
// events where the Shell sends the complete text each time, not incremental chunks.
export function replaceReply(body, text, doc = globalThis.document) {
  drawReply(body, text, doc);
}
