// This file exists because chat.js touches `window.__TAURI__` at import and
// node tests cannot load it. The log is the history.

export function createComposerRecall(log) {
  let place = { at: "draft" };

  return {
    consume(event, field) {
      if (!bareVerticalArrow(event)) return false;

      const rows = userRows(log);
      // The anchored row is gone. The draft is back, and this arrow moves the caret in it.
      if (place.at === "row" && !rows.includes(place.row)) {
        writeDraft(field, place.draft);
        place = { at: "draft" };
        return false;
      }

      const edge = caretEdges(field);
      if (!edge) return false;

      const up = event.key === "ArrowUp";
      if (up ? !edge.first : !edge.last) return false;

      if (place.at === "draft") {
        if (!up || rows.length === 0) return false;
        const draft = capture(field);
        const newest = rows[rows.length - 1];
        writeCaret(field, saidText(newest), 0);
        place = { at: "row", row: newest, draft };
        return taken(event);
      }

      const at = rows.indexOf(place.row);
      if (up) {
        if (at <= 0) return taken(event);
        const previous = rows[at - 1];
        writeCaret(field, saidText(previous), 0);
        place = { at: "row", row: previous, draft: place.draft };
        return taken(event);
      }

      if (at === rows.length - 1) {
        writeDraft(field, place.draft);
        place = { at: "draft" };
        return taken(event);
      }

      const next = rows[at + 1];
      const text = saidText(next);
      writeCaret(field, text, text.length);
      place = { at: "row", row: next, draft: place.draft };
      return taken(event);
    },

    release() {
      place = { at: "draft" };
    },
  };
}

function taken(event) {
  event.preventDefault();
  return true;
}

function bareVerticalArrow(event) {
  if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return false;
  if (event.shiftKey || event.ctrlKey || event.altKey || event.metaKey) return false;
  if (event.isComposing) return false;
  return true;
}

function userRows(log) {
  const rows = [];
  const children = log.children;
  for (let i = 0; i < children.length; i++) {
    const child = children[i];
    if (child.classList.contains("row") && child.classList.contains("you")) {
      rows.push(child);
    }
  }
  return rows;
}

function saidText(row) {
  return row.querySelector(".said").textContent;
}

function capture(field) {
  return {
    value: field.value,
    selectionStart: field.selectionStart,
    selectionEnd: field.selectionEnd,
  };
}

function clamp(index, length) {
  if (index < 0) return 0;
  if (index > length) return length;
  return index;
}

function writeCaret(field, value, at) {
  field.value = value;
  field.setSelectionRange(at, at);
}

function writeDraft(field, draft) {
  const length = draft.value.length;
  field.value = draft.value;
  field.setSelectionRange(
    clamp(draft.selectionStart, length),
    clamp(draft.selectionEnd, length),
  );
}

function caretEdges(field) {
  if (field.selectionStart !== field.selectionEnd) return null;
  if (!field.clientWidth || !field.ownerDocument) return logicalEdges(field);
  return mirrorEdges(field);
}

function logicalEdges(field) {
  const value = field.value;
  const at = field.selectionStart;
  return {
    first: !value.slice(0, at).includes("\n"),
    last: !value.slice(at).includes("\n"),
  };
}

function mirrorEdges(field) {
  const value = field.value;
  const at = field.selectionStart;
  const lineStart = value.slice(0, at).lastIndexOf("\n") + 1;
  const newline = value.indexOf("\n", at);
  const lineEnd = newline === -1 ? value.length : newline;
  const onFirst = lineStart === 0;
  const onLast = lineEnd === value.length;
  if (!onFirst && !onLast) return { first: false, last: false };

  const visual = softEdges(field, value.slice(lineStart, lineEnd), at - lineStart);
  return { first: onFirst && visual.first, last: onLast && visual.last };
}

function softEdges(field, text, at) {
  const doc = field.ownerDocument;
  const computed = doc.defaultView.getComputedStyle(field);
  // A wrapped draft has no newline, and Up would replace it if only `\n` counted.
  const mirror = doc.createElement("div");
  const style = mirror.style;
  style.position = "fixed";
  style.visibility = "hidden";
  style.left = "0";
  style.top = "0";
  style.margin = "0";
  style.boxSizing = "content-box";
  style.padding = "0";
  style.border = "0";
  style.whiteSpace = computed.whiteSpace || "pre-wrap";
  style.overflowWrap = computed.overflowWrap || "normal";
  style.wordBreak = computed.wordBreak || "normal";
  style.font = computed.font;
  style.fontStyle = computed.fontStyle;
  style.fontVariant = computed.fontVariant;
  style.fontWeight = computed.fontWeight;
  style.fontStretch = computed.fontStretch;
  style.fontSize = computed.fontSize;
  style.fontFamily = computed.fontFamily;
  style.lineHeight = computed.lineHeight;
  style.letterSpacing = computed.letterSpacing;
  const width = field.clientWidth - px(computed.paddingLeft) - px(computed.paddingRight);
  style.width = `${Math.max(0, width)}px`;

  // One character. A span of the remainder starts on the next line at a wrap.
  const marker = doc.createElement("span");
  const ch = text.slice(at, at + 1);
  marker.textContent = ch || "\u200b";
  mirror.append(doc.createTextNode(text.slice(0, at)), marker);
  if (ch) mirror.append(doc.createTextNode(text.slice(at + 1)));

  const parent = doc.body ?? doc.documentElement;
  parent.append(mirror);
  try {
    const line = px(computed.lineHeight) || px(computed.fontSize) * 1.2 || 1;
    const top = marker.getBoundingClientRect().top - mirror.getBoundingClientRect().top;
    return {
      first: top <= line / 2,
      last: mirror.scrollHeight - top - line <= line / 2,
    };
  } finally {
    mirror.remove();
  }
}

function px(value) {
  const n = Number.parseFloat(value);
  return Number.isFinite(n) ? n : 0;
}
