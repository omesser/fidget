import { t } from "./i18n.js";

// The window-names notice. Split from chat.js, which reads `window.__TAURI__`
// at import, so node can load this.
const HEADING = "Window names are off";

export const BODY =
  "The fidget knows where your windows are, not what they are. One switch in Settings turns on titles and application names together.";

const BUTTONS = [
  { action: "open-settings", label: "Open Settings" },
  { action: "dismiss", label: "Don't show this again" },
];

const HIDDEN = { visible: false, heading: "", body: "", buttons: [] };

// Built per view, so a language change reaches the next draw.
function shown() {
  return {
    visible: true,
    heading: t(HEADING),
    body: t(BODY),
    buttons: BUTTONS.map((button) => ({ ...button, label: t(button.label) })),
  };
}

function viewOf(hint) {
  return hint === "due" ? shown() : HIDDEN;
}

export function createNamesNotice({ act }) {
  let generation = 0;
  let hint = null;

  function receive(payload) {
    const nextHint = payload?.hint;
    const nextGen = payload?.generation;
    if (typeof nextHint !== "string" || typeof nextGen !== "number") {
      return { changed: false, view: viewOf(hint) };
    }
    if (nextGen < generation || (nextGen === generation && nextHint === hint)) {
      return { changed: false, view: viewOf(hint) };
    }

    generation = nextGen;
    const changed = nextHint !== hint;
    hint = nextHint;
    return { changed, view: viewOf(hint) };
  }

  return {
    receive,

    async press(action) {
      return receive(await act(action));
    },
  };
}
