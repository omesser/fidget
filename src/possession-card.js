// The bindings card and the charge bar. The page paints this. The Shell
// decides whether the sprite is possessed, whether focus has left, and how
// full the bar is.
import { placeQuickMessage } from "./quick-message.js";

export const POSSESSION_HEADING = "Manually drive the fidget on your screen";

export const RESUME_LABEL = "Click to resume";

// The keys this build binds. Up, Down and the digits are later.
const KEYS = [
  ["← →", "Walk"],
  ["Space", "Jump"],
  ["Esc", "Release Possession"],
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

// One card element keeps its last model. A charge tick must not rebuild it.
const painted = new WeakMap();

function modelStamp(model) {
  if (!model) return "";
  return JSON.stringify([Boolean(model.paused), model.heading, model.keys]);
}

// Paint `model` into `card`. `bar` is the track. `fill` is from `chargeFill`.
// Returns whether the card's nodes were rebuilt.
export function paintPossession(card, bar, model, fill) {
  const stamp = modelStamp(model);
  const rebuilt = painted.get(card) !== stamp;
  if (rebuilt) {
    painted.set(card, stamp);
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
  }
  const shown = fill != null && model && !model.paused;
  bar.classList.toggle("visible", shown);
  const inner = bar.querySelector(".possession-charge-fill");
  if (inner) inner.style.width = shown ? `${fill * 100}%` : "0";
  return rebuilt;
}

// One box, so the card takes the same seat as the quick-message pill.
export function placePossession(sprite, cardSize, barHeight, gap, bounds, speech, pill) {
  const stack = { width: cardSize.width, height: cardSize.height + gap + barHeight };
  const seat = placeQuickMessage(sprite, stack, bounds, speech, pill);
  const x = Math.round(seat.x);
  if (seat.inverted) {
    return {
      card: { x, y: Math.round(seat.y + barHeight + gap) },
      bar: { x, y: Math.round(seat.y), width: cardSize.width, height: barHeight },
    };
  }
  return {
    card: { x, y: Math.round(seat.y) },
    bar: {
      x,
      y: Math.round(seat.y + cardSize.height + gap),
      width: cardSize.width,
      height: barHeight,
    },
  };
}
