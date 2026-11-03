
// The Chat UI design class on the root element. Split from chat.js, which
// reads `window.__TAURI__` at import, so node can load this.
const DESIGNS = ["minimal", "terminal", "glass"];

// Allowlist known Chat UI designs. Unknown strings from settings or events
// must not become class names.
export function normalizeChatUi(value) {
  return DESIGNS.includes(value) ? value : "minimal";
}

export function applyChatUiClass(root, design) {
  root.classList.remove(...DESIGNS.map((name) => `chat-ui-${name}`));
  root.classList.add(`chat-ui-${normalizeChatUi(design)}`);
}

// Platform selectors let Glass tune its tint when a renderer lacks desktop blur.
export function applyPlatform(root, platform) {
  if (platform === "linux" || platform === "macos" || platform === "windows") {
    root.dataset.platform = platform;
  } else {
    delete root.dataset.platform;
  }
}
