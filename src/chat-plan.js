// The agent's plan, as the rows the list above the composer draws. Its own
// module for the reason chat-ask.js gives: chat.js reaches window.__TAURI__ as
// it loads and cannot be imported outside a webview; this can, so it has a test.

// `content` is untrusted Harness text and gets the same `stripUnsafe` an ask
// does. `status` and `priority` pass through as the wire's own spellings, so
// CSS owns every appearance and nothing here decides how a status looks.

import { stripUnsafe } from "./markdown.js";

export function planSteps(entries) {
  if (!Array.isArray(entries)) {
    return [];
  }
  return entries.map((entry) => ({
    text: stripUnsafe(entry.content ?? ""),
    status: entry.status ?? "",
    priority: entry.priority ?? "",
  }));
}
