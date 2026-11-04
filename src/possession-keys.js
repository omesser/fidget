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
// The pill's field and an unpossessed overlay keep keydowns but pass keyups, so
// a walk still ends when its key comes up after focus moved away mid-hold.
export function possessionEdge(key, repeat, down, code, inField = false, possessed = true) {
  if (repeat || (down && (inField || !possessed))) return null;
  const digit = digitFromCode(code);
  if (digit) return { key: digit, down };
  const name = NAMES[key];
  if (!name) return null;
  return { key: name, down };
}
