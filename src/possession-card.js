// The bindings card and the charge bar. The page paints this. The Shell
// decides whether the sprite is possessed, whether focus has left, and how
// full the bar is.

export const POSSESSION_HEADING = "Manually drive the fidget on your screen";

export const RESUME_LABEL = "Click to resume";

// The keys this build binds. Up, Down and the digits are later.
const KEYS = [
  ["← →", "Walk"],
  ["Space", "Jump"],
  ["Esc", "Release"],
];

// Null while the sprite is not possessed. Paused keeps the card and replaces
// the explanation with the resume line.
export function possessionCard({ possessed, paused }) {
  if (!possessed) return null;
  if (paused) {
    return { paused: true, dimmed: true, heading: RESUME_LABEL, keys: KEYS };
  }
  return { paused: false, dimmed: false, heading: POSSESSION_HEADING, keys: KEYS };
}

// Null hides the bar. A number is the fill, clamped to 0..1.
export function chargeFill(charge) {
  if (charge == null || Number.isNaN(charge)) return null;
  return Math.min(1, Math.max(0, charge));
}

// The card takes the resume click. The bar is only drawn.
export function possessionRects(card, bar) {
  const rects = [];
  if (card) rects.push({ rect: card, clickable: true });
  if (bar) rects.push({ rect: bar, clickable: false });
  return rects;
}

function row(keys, does) {
  const item = document.createElement("li");
  const name = document.createElement("span");
  name.className = "possession-keys";
  name.textContent = keys;
  const what = document.createElement("span");
  what.textContent = does;
  item.append(name, what);
  return item;
}

// Paint `model` into `card`. `bar` is the track. `fill` is from `chargeFill`.
export function paintPossession(card, bar, model, fill) {
  card.classList.toggle("visible", Boolean(model));
  card.classList.toggle("paused", Boolean(model?.paused));
  card.replaceChildren();
  if (model) {
    const heading = document.createElement("p");
    heading.className = "possession-heading";
    heading.textContent = model.heading;
    card.append(heading);
    if (!model.paused) {
      const list = document.createElement("ul");
      for (const [keys, does] of model.keys) list.append(row(keys, does));
      card.append(list);
    }
  }
  const shown = fill != null && model && !model.paused;
  bar.classList.toggle("visible", shown);
  const inner = bar.querySelector(".possession-charge-fill");
  if (inner) inner.style.width = shown ? `${fill * 100}%` : "0";
}

// Where the card and the bar sit, in overlay pixels, centered on the sprite.
// The bar is the strip between the card and the art.
export function placePossession(sprite, cardSize, barHeight, gap) {
  const cardX = Math.round(sprite.x + sprite.width / 2 - cardSize.width / 2);
  const barY = sprite.y - gap - barHeight;
  const cardY = barY - gap - cardSize.height;
  const barX = cardX;
  return {
    card: { x: cardX, y: cardY },
    bar: { x: barX, y: barY, width: cardSize.width, height: barHeight },
  };
}
