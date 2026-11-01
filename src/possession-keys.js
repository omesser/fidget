// The keys possession listens to. Digits are a later binding, and Up and
// Down are a later climb. A repeat is the platform holding the key down.

const NAMES = {
  ArrowLeft: "left",
  ArrowRight: "right",
  " ": "space",
  Spacebar: "space",
  Escape: "escape",
};

// `down` is true for a keydown. A repeat, or any other key, is not an edge.
export function possessionEdge(key, repeat, down) {
  if (repeat) return null;
  const name = NAMES[key];
  if (!name) return null;
  return { key: name, down };
}
