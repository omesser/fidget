// Draws what an ask says into the consent row. Apart from chat.js for the
// reason chat-ask.js gives: chat.js cannot load outside a webview, and this
// can, so the DOM shape has a test.

import { askSays } from "./chat-ask.js";
import { drawReply, drawTitle } from "./markdown.js";

// One element per plain part, written with `textContent` only.
const ELEMENT = {
  prose: ["div", "ask-prose"],
  metadata: ["div", "ask-metadata"],
};

export function drawAskDetails(body, ask) {
  const doc = body.ownerDocument;
  for (const { kind, text } of askSays(ask)) {
    // The Harness's own text, so a span it marked as code is drawn as code: in
    // a title only the spans, in content the Markdown it wrote. Both hosts carry
    // `.said.md`, which is where the code paint lives.
    if (kind === "title") {
      const host = doc.createElement("div");
      host.className = "ask-title said md";
      drawTitle(host, text, doc);
      body.append(host);
      continue;
    }
    if (kind === "markdown") {
      const host = doc.createElement("div");
      host.className = "ask-content said md";
      // Empty JSON is not a body on this card. A reply keeps that fence.
      drawReply(host, text, doc, { skipEmptyJson: true });
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
