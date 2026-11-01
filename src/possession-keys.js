// The keys possession listens to. Digits name Behaviors. Letters stay free,
// so A and D do not collide with WASD. A repeat is the platform holding the
// key down.

const NAMES = {
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
  " ": "space",
  Spacebar: "space",
  Escape: "escape",
};

// `event.key` is the character the layout produces. On AZERTY the top row
// is `&` and `é`, so a digit is the physical key (`Digit1`, `Numpad1`).
function digitFromCode(code) {
  if (typeof code !== "string") return null;
  const match = /^(?:Digit|Numpad)([1-9])$/.exec(code);
  return match ? match[1] : null;
}

// `down` is true for a keydown. A repeat, or any other key, is not an edge.
// The pill's field keeps its keydowns, but not its keyups: the pill can take
// focus while a key is held, and the walk must still end when it comes up.
export function possessionEdge(key, repeat, down, code, inField = false) {
  if (repeat || (down && inField)) return null;
  const digit = digitFromCode(code);
  if (digit) return { key: digit, down };
  const name = NAMES[key];
  if (!name) return null;
  return { key: name, down };
}
