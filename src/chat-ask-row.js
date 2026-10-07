// Draws what an ask says into the consent row. Apart from chat.js for the
// reason chat-ask.js gives: chat.js cannot load outside a webview, and this
// can, so the DOM shape has a test.

import { askSays } from "./chat-ask.js";
import { drawReply } from "./markdown.js";

// One element per plain part. Harness content goes through `drawReply`, which
// writes with `textContent` only. The reply rules key off `.said.md`.
const ELEMENT = {
  title: ["div", "ask-title"],
  code: ["code", "ask-code"],
  prose: ["div", "ask-prose"],
  metadata: ["div", "ask-metadata"],
};

export function drawAskDetails(body, ask) {
  const doc = body.ownerDocument;
  for (const { kind, text } of askSays(ask)) {
    if (kind === "markdown") {
      const host = doc.createElement("div");
      host.className = "said md";
      drawReply(host, text, doc);
      if (host.children.length > 0) {
        body.append(host);
      }
      continue;
    }
    const [tag, className] = ELEMENT[kind];
    const node = doc.createElement(tag);
    node.className = className;
    node.textContent = text;
    body.append(node);
  }
}
