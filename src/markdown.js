
// Markdown in a reply, drawn as formatting instead of its punctuation.
// Only `marked`'s token API is used, never its HTML renderer. A reply is
// untrusted, so every piece reaches the DOM through `textContent`.
import { Lexer } from "./vendor/marked.esm.js";

// Bidi controls can reverse a line. U+200B, word joiner, and BOM can hide
// one. Newline and tab stay, because Markdown uses them. ZWJ and ZWNJ stay
// too, because joined emoji and some scripts need them.
const UNSAFE =
  /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F\u200B\u202A-\u202E\u2060\u2066-\u2069\uFEFF]/g;

export function stripUnsafe(text) {
  return String(text ?? "").replace(UNSAFE, "");
}

// GFM, because that is what harnesses emit. CommonMark alone would draw
// a table as a run of pipes.
const FLAVOUR = { gfm: true, breaks: false, pedantic: false };

// Schemes a link may carry. An allowlist means `javascript:` or `data:`
// never reaches the DOM. Checked as a scheme, so a disguised spelling fails
// without its own rule.
const SCHEMES = new Set(["http:", "https:", "mailto:"]);

function target(href) {
  const trimmed = (href ?? "").trim();
  const scheme = /^[a-z][a-z0-9+.-]*:/i.exec(trimmed);
  return scheme && SCHEMES.has(scheme[0].toLowerCase()) ? trimmed : null;
}

// A token this file does not draw reads as the source the model wrote,
// so an unsupported construct looks unformatted rather than vanished.
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
        // A refused target leaves the link text. The words are still what it said.
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
    // `align` is one of three words `marked` takes from the delimiter row,
    // never text the model chose. It names a class so the stylesheet owns it.
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
        // A tight list item's line. `marked` hands it over as a block `text` token.
        // It belongs in the item with no paragraph of its own.
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
        // the reply, and it cannot. The reply is a log line.
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
        // A fence and a table do not reflow, and Chat opens 420 points wide.
        // The scroll goes on a wrapper so the overflow stays inside the row.
        const fence = doc.createElement("div");
        fence.className = "md-wide";
        const pre = doc.createElement("pre");
        const code = doc.createElement("code");
        // The info string is the model's, so it is filtered to a word before
        // it names a class. That is the hook a highlighter would read.
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

// True when every block is an empty JSON fence. The ask then has no body.
export function onlyEmptyJson(text) {
  const tokens = Lexer.lex(String(text ?? ""), FLAVOUR).filter((token) => token.type !== "space");
  return tokens.length > 0 && tokens.every((token) => token.type === "code" && emptyJsonBlock(token.text));
}

// The reply drawn in each row so far, so a chunk can be added to it.
// Keyed on the element the surface holds. A removed row drops its entry.
const drawn = new WeakMap();

// Draw the whole of `text` into `body` and put the caret back. Found
// and re-added with `querySelector` and `append`, never `insertBefore`.
// The caret may be nested in a block, not a direct child of `body`.
export function drawReply(body, text, doc = globalThis.document, options) {
  const caret = body.querySelector(".caret");
  drawn.set(body, text ?? "");
  body.replaceChildren();
  blocks(Lexer.lex(stripUnsafe(text ?? ""), FLAVOUR), body, doc, options?.skipEmptyJson);
  if (caret) {
    // Inside the last paragraph, so it blinks at the end of the line.
    // A fence, a table, or an empty reply puts it back on the row.
    const last = body.lastElementChild;
    (last && last.tagName === "P" ? last : body).append(caret);
  }
}

// One chunk of a streaming reply. The row is redrawn from the whole
// text, because a chunk can land mid-marker. `**bo` then `ld**` is
// neither of them Markdown.
export function appendReply(body, chunk, doc = globalThis.document) {
  drawReply(body, (drawn.get(body) ?? "") + chunk, doc);
}

// Replace the reply with the full answer so far. Streaming Dialogue
// sends the complete text each time, not a chunk.
export function replaceReply(body, text, doc = globalThis.document) {
  drawReply(body, text, doc);
}

// Thinking, as written, with only links spelled `[text](url)`.
// `marked` also links a bare URL, a `www.` host, and an email. In a
// mark those are an agent's uri, not a link it wrote.
export function drawThought(body, text, doc = globalThis.document) {
  body.replaceChildren();
  for (const token of Lexer.lexInline(stripUnsafe(text ?? ""), FLAVOUR)) {
    // An escape is how a mark spells a character Markdown would read.
    const written = token.type === "link" && token.raw.startsWith("[");
    if (written || token.type === "escape") {
      inline([token], body, doc);
    } else {
      asSource(token, body, doc);
    }
  }
}

// An ask's title. Only spans wrapped in backticks are code. No Markdown
// parser sees it, so `**`, `~~`, `#`, and link brackets stay characters.
export function drawTitle(body, text, doc = globalThis.document) {
  body.replaceChildren();
  for (const part of codeSpans(stripUnsafe(text ?? ""))) {
    if (part.code) {
      const code = doc.createElement("code");
      code.textContent = part.text;
      body.append(code);
    } else {
      body.append(doc.createTextNode(part.text));
    }
  }
}

// A run of N backticks closes at the next run of exactly N. An opener
// with no closer is text. A line ending inside is a space, and one space
// comes off each end when both ends have one.
function codeSpans(source) {
  const parts = [];
  let plain = "";
  let at = 0;
  while (at < source.length) {
    const open = source.indexOf("`", at);
    if (open < 0) {
      break;
    }
    const after = runEnd(source, open);
    const close = closer(source, after, after - open);
    if (close < 0) {
      plain += source.slice(at, after);
      at = after;
      continue;
    }
    plain += source.slice(at, open);
    if (plain) {
      parts.push({ code: false, text: plain });
    }
    plain = "";
    let inner = source.slice(after, close).replace(/\r\n|\r|\n/g, " ");
    if (inner.startsWith(" ") && inner.endsWith(" ") && /[^ ]/.test(inner)) {
      inner = inner.slice(1, -1);
    }
    parts.push({ code: true, text: inner });
    at = close + (after - open);
  }
  plain += source.slice(at);
  if (plain) {
    parts.push({ code: false, text: plain });
  }
  return parts;
}

function runEnd(source, from) {
  let end = from;
  while (source[end] === "`") {
    end += 1;
  }
  return end;
}

function closer(source, from, length) {
  let scan = from;
  while (scan < source.length) {
    const next = source.indexOf("`", scan);
    if (next < 0) {
      return -1;
    }
    const end = runEnd(source, next);
    if (end - next === length) {
      return next;
    }
    scan = end;
  }
  return -1;
}
