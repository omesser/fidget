// The window-names notice. Its own module because chat.js reaches
// window.__TAURI__ as it loads and cannot be imported under node --test.

const HEADING = "Window names are off";

export const BODY =
  "The fidget knows where your windows are, not what they are. One switch in Settings turns on titles and application names together.";

const BUTTONS = [
  { action: "open-settings", label: "Open Settings" },
  { action: "dismiss", label: "Don't show this again" },
];

const HIDDEN = { visible: false, heading: "", body: "", buttons: [] };

const SHOWN = { visible: true, heading: HEADING, body: BODY, buttons: BUTTONS };

function viewOf(hint) {
  return hint === "due" ? SHOWN : HIDDEN;
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
