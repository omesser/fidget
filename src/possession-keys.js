// The keys possession listens to. Up and Down are a later climb. Digits name
// Behaviors. Letters stay free, so A and D do not collide with WASD. A repeat
// is the platform holding the key down.

const NAMES = {
  ArrowLeft: "left",
  ArrowRight: "right",
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
export function possessionEdge(key, repeat, down, code) {
  if (repeat) return null;
  const digit = digitFromCode(code);
  if (digit) return { key: digit, down };
  const name = NAMES[key];
  if (!name) return null;
  return { key: name, down };
}
