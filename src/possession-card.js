// The bindings card and the charge bar. The page paints this. The Shell
// decides whether the sprite is possessed, whether focus has left, and how
// full the bar is.

export const POSSESSION_HEADING = "Manually drive the fidget on your screen";

export const PAUSED_HEADING = "Paused";

export const FOCUS_PRESSED_LABEL = "Focused";

export const FOCUS_RAISED_LABEL = "Focus";

// Both states name the action, since the visible label is one word.
export const FOCUS_ARIA_LABEL = "Click to focus";

// The button carries the Esc hint, so the bindings list does not repeat it.
export const RELEASE_LABEL = "Release Possession (Esc)";

// Clear of the screen edge, on the display the sprite stands on. The card
// stays put while the sprite walks, so the background under it stays visible.
export const POSSESSION_INSET = 16;

// Digit rows are the list the Shell already mapped, in that order. The card
// only numbers them.
const KEYS = [
  ["← →", "Walk"],
  ["↑", "Climb"],
  ["↓", "Drop"],
  ["Space", "Jump"],
];

// Walk and Jump are written as labels. A Behavior name is a key (`nod`).
function behaviorLabel(name) {
  if (!name) return name;
  return name.charAt(0).toUpperCase() + name.slice(1);
}

function bindingRows(behaviors) {
  const digits = [...(behaviors ?? [])].map((name, index) => [
    String(index + 1),
    behaviorLabel(name),
  ]);
  return [...KEYS, ...digits];
}

// Null while the sprite is not possessed. Paused keeps the card and replaces
// the explanation with the paused line. The whole paused card is the resume
// click, so it has no release button. The focus button shows which it is.
export function possessionCard({ possessed, paused, behaviors }) {
  if (!possessed) return null;
  const keys = bindingRows(behaviors);
  if (paused) {
    return {
      paused: true,
      dimmed: true,
      heading: PAUSED_HEADING,
      keys,
      release: null,
      focus: { label: FOCUS_RAISED_LABEL, pressed: false },
    };
  }
  return {
    paused: false,
    dimmed: false,
    heading: POSSESSION_HEADING,
    keys,
    release: RELEASE_LABEL,
    focus: { label: FOCUS_PRESSED_LABEL, pressed: true },
  };
}

// Null hides the bar. A number is the fill, clamped to 0..1.
export function chargeFill(charge) {
  if (charge == null || Number.isNaN(charge)) return null;
  return Math.min(1, Math.max(0, charge));
}

// The card takes the resume click only while paused. Otherwise a sprite
// perched under it cannot be grabbed, so only the buttons' own rects take a
// click. The bar is only drawn.
export function possessionRects(card, bar, release, focus, paused) {
  const rects = [];
  if (card) rects.push({ rect: card, clickable: Boolean(paused) });
  if (release) rects.push({ rect: release, clickable: true });
  if (focus) rects.push({ rect: focus, clickable: true });
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
  return JSON.stringify([Boolean(model.paused), model.heading, model.keys, model.release, model.focus]);
}

function focusButton({ label, pressed }) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "possession-focus";
  button.setAttribute("aria-pressed", pressed);
  button.setAttribute("aria-label", FOCUS_ARIA_LABEL);
  const icon = document.createElement("span");
  icon.className = "possession-focus-icon";
  const text = document.createElement("span");
  text.className = "possession-focus-label";
  text.textContent = label;
  button.append(icon, text);
  return button;
}

// One press handler, so a press on the focus button inside a paused card asks
// for focus once. A press here is not a press on the art, so it must not poke.
export function wirePossessionCard(card, invoke) {
  const report = (command) => (err) => console.error(command, err);
  card.addEventListener("pointerdown", (event) => {
    event.stopPropagation();
    const onFocus = Boolean(event.target.closest(".possession-focus"));
    if (!onFocus && !card.classList.contains("paused")) return;
    invoke("overlay_request_focus").catch(report("overlay_request_focus"));
  });
  card.addEventListener("pointerup", (event) => event.stopPropagation());
  // The release button presses Esc, so Possession ends on the path the key takes.
  card.addEventListener("click", (event) => {
    event.stopPropagation();
    if (!event.target.closest(".possession-release")) return;
    for (const down of [true, false]) {
      invoke("overlay_possession_key", { key: "escape", down }).catch(report("overlay_possession_key"));
    }
  });
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
      const footer = document.createElement("div");
      footer.className = "possession-footer";
      if (model.release) {
        const release = document.createElement("button");
        release.type = "button";
        release.className = "possession-release";
        release.textContent = model.release;
        footer.append(release);
      }
      footer.append(focusButton(model.focus));
      card.append(footer);
    }
  }
  const shown = fill != null && model && !model.paused;
  bar.classList.toggle("visible", shown);
  const inner = bar.querySelector(".possession-charge-fill");
  if (inner) inner.style.width = shown ? `${fill * 100}%` : "0";
  return rebuilt;
}

// The usable area when the shell has named one. The overlay frame includes
// the menu bar, and a card inset from that frame covers the Apple menu.
export function possessionBounds(work, frame) {
  if (work && work.width > 0 && work.height > 0) return work;
  return frame;
}

// Top-left of `bounds`, the usable area of the display the sprite is on.
// Where the sprite stands on that display does not move the card.
export function placePossession(cardSize, barHeight, gap, bounds) {
  const x = Math.round(bounds.x + POSSESSION_INSET);
  const y = Math.round(bounds.y + POSSESSION_INSET);
  return {
    card: { x, y },
    bar: {
      x,
      y: Math.round(y + cardSize.height + gap),
      width: cardSize.width,
      height: barHeight,
    },
  };
}
