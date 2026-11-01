// The keys possession listens to. Up and Down are a later climb. Digits name
// Behaviors. Letters stay free, so A and D do not collide with WASD. A repeat
// is the platform holding the key down.

const NAMES = {
  ArrowLeft: "left",
  ArrowRight: "right",
  " ": "space",
  Spacebar: "space",
  Escape: "escape",
  1: "1",
  2: "2",
  3: "3",
  4: "4",
  5: "5",
  6: "6",
  7: "7",
  8: "8",
  9: "9",
};

// `down` is true for a keydown. A repeat, or any other key, is not an edge.
export function possessionEdge(key, repeat, down) {
  if (repeat) return null;
  const name = NAMES[key];
  if (!name) return null;
  return { key: name, down };
}
