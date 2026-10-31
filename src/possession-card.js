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

function stack(cardX, cardY, cardSize, barY, barHeight) {
  return {
    card: { x: cardX, y: cardY },
    bar: { x: cardX, y: barY, width: cardSize.width, height: barHeight },
  };
}

function shift(placed, dx, dy) {
  return {
    card: { x: placed.card.x + dx, y: placed.card.y + dy },
    bar: { ...placed.bar, x: placed.bar.x + dx, y: placed.bar.y + dy },
  };
}

function union(placed, cardSize) {
  const top = Math.min(placed.card.y, placed.bar.y);
  const bottom = Math.max(placed.card.y + cardSize.height, placed.bar.y + placed.bar.height);
  return { x: placed.card.x, y: top, width: cardSize.width, height: bottom - top };
}

function overlaps(a, b) {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}

function clampTo(placed, cardSize, bounds) {
  const box = union(placed, cardSize);
  const x = Math.max(bounds.x, Math.min(box.x, bounds.x + bounds.width - box.width));
  const y = Math.max(bounds.y, Math.min(box.y, bounds.y + bounds.height - box.height));
  return shift(placed, x - box.x, y - box.y);
}

// Above the sprite, with the bar between the card and the art. Near the top
// of the display the stack sits below the sprite instead, and a speech bubble
// in the way pushes the stack clear of it. Both stay inside the display.
export function placePossession(sprite, cardSize, barHeight, gap, bounds, speech) {
  const cardX = sprite.x + sprite.width / 2 - cardSize.width / 2;
  const above = sprite.y - gap - barHeight - gap - cardSize.height;
  const belowCard = sprite.y + sprite.height + gap + barHeight + gap;
  const belowBar = sprite.y + sprite.height + gap;
  let placed =
    bounds && above < bounds.y
      ? stack(cardX, belowCard, cardSize, belowBar, barHeight)
      : stack(cardX, above, cardSize, above + cardSize.height + gap, barHeight);

  if (speech && overlaps(union(placed, cardSize), speech)) {
    const clearAbove = speech.y - gap - barHeight - gap - cardSize.height;
    placed =
      !bounds || clearAbove >= bounds.y
        ? stack(cardX, clearAbove, cardSize, clearAbove + cardSize.height + gap, barHeight)
        : stack(
            cardX,
            speech.y + speech.height + gap + barHeight + gap,
            cardSize,
            speech.y + speech.height + gap,
            barHeight,
          );
  }

  if (bounds) placed = clampTo(placed, cardSize, bounds);
  return {
    card: { x: Math.round(placed.card.x), y: Math.round(placed.card.y) },
    bar: {
      x: Math.round(placed.bar.x),
      y: Math.round(placed.bar.y),
      width: placed.bar.width,
      height: placed.bar.height,
    },
  };
}
